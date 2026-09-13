# Interprocedural view access

Status: consolidated implementation design. Compiler implementation is not yet shipped.
This is boundary 2 of [plan 52](52-readonly-view-provenance-plan.md).
The producer-validation owner established by [plan 55](55-mir-producer-validation-plan.md)
is a prerequisite, not this capability's implementation.

## Problem and capability boundary

A plain slice parameter can write literal bytes, and a byte slice returned by
an ordinary function can lose its read-only origin. Both source checking and
per-unit checking accept these compile-only negative witnesses at
`abee27a4229e4dab1db837c894ad7c88275aff8d`:

```align
fn write(bytes: slice<u8>) {
  mut view := bytes
  view[0] = 65
}
fn literal() -> slice<u8> = "xx".bytes()
fn main() {
  write("xx".bytes())
  mut result := literal()
  result[0] = 65
}
```

Returned `str` followed by `.bytes()` is already read-only under plan 52; it is
an existing positive rejection control, not a remaining counterexample.
Plans 57 and 60 close local UTF-8 and codec validation followed by overlapping
local mutation, including SSE next-event text formed on the post-action Buffer
generation.
Hidden callee writes and callee-created validation remain part of this boundary.

Never execute a negative witness. Separate the two failing operations in owners,
then pair each with a reader and an explicit owned copy. Indirect calls, captures,
mutable outputs and imported concrete generic bodies belong to the same repair.
There is no new source qualifier, parameter mode, implicit allocation or runtime
permission test. R48 remains settled; R46's new array-field write path is not
admitted by this plan.

The producer, interface transport, caller instantiation and all validation
entrypoints must ship together. This capability is expected to exceed 1,000
handwritten lines: splitting its metadata producer from its consumers would
leave both reproduced writes accepted and duplicate the cross-unit proof. Native
transfer classification is part of the same producer, not an optional follow-up.
No performance promise or additional benchmark gate is introduced.

## Decision and delivery boundary

Implement one source-qualified, interprocedural access analysis in MIR. Retain
the existing language, ownership, lifetime and unsafe contracts. The access
program crosses unit interfaces; the same analysis qualifies source checking,
publication, cached bodies and native emission. The complete chain is one
capability because an unused producer or an unchecked consumer closes neither
reproduced write.

The strategy is fixed by the following decisions. The exact APIs, wire fields
and native transfers below are their implementation ledger, not additional
research milestones.

| Decision | Selected rule and reason |
| --- | --- |
| State representation | A finite rooted heap, shared typed profiles and a relation over current facts. Componentwise unions lose real callback/argument correlations; execution histories and complete-world enumeration do not form the selected representation. |
| Termination | Bound entry roots before introducing protected input anchors; bound body roots from syntax plus those anchors. Normalize heap roles and current binding positions after every transfer. The finite-domain argument below owns the complete context bound. |
| Private descriptor identity | Keep descriptor ids in sealed private constructor bindings. Omit them uniformly from public access payloads; exported entry names retain their existing public identity. Renaming a reachable private descriptor cannot change consumer identity by itself. |
| Native proof metadata | Preserve source no-alias formation, trusted map_into scope assignment and the existing actual-MIR borrowed-element bounds owner. Project native ids out of access bytes; those bytes cannot certify native pointers or LLVM metadata. |
| Unsafe effects | Source qualification is conditional on draft §15's existing unsafe obligations. Arbitrary foreign writes and raw frees/stores are not proved safe. Ordinary source-safe writes remain checked even inside unsafe. No new foreign effect annotation or blanket invalidation rule is introduced. |
| DB bridge ownership | Retain the selected typed operation/witness record at the ten checked formation sites. Its 14 operations and 22 allowed witness combinations supply source effects; native pointer validity remains the existing wrapper obligation. |
| Artifact authority | Move the existing BuiltStaticArtifact carrier and pure runtime generation to align_interface, make that same carrier opaque and derive its fields from validated bytes. A second artifact type is unnecessary. |
| Verification | Port the bounded algebra controls into the implementation owners and retain source positive/negative pairs. Author models are evidence for individual operations, not passed compiler acceptance tests. |

The design review's carrier and DB-role objections were withdrawn after checking
the existing records. The private-id and native-identity issues are corrected
here because they had concrete cache/validation consequences. The finite-domain
argument is made explicit. A demand to prove arbitrary foreign mutation would
change the settled unsafe boundary and is not adopted. These dispositions close
the design decisions; no reviewer verdict substitutes for implementation evidence.

Work in this order within the capability:

1. Implement the finite domain, canonical projection and source evaluation
   records in MIR, with invariant-level owners for joins, recursive calls,
   observations and malformed records.
2. Connect immutable checked-source formation, interface transport and caller
   instantiation. Run the original write/result witnesses and their reader/copy
   controls through both frontends before expanding native adapters.
3. Connect the enumerated native/DB transfers, opaque artifact finalization and
   all emission/cache/partition consumers. Preserve the existing native owners.
4. Close each implementation matrix row, then run the change's owner checks and
   repository preflight. Publication is justified by those results, not by this
   document's completion.

This is implementation order, not a series of independently publishable dormant
metadata PRs. The schemas are specified once below. Model development history
and interrupted experiments remain in the saved investigation; they are not
additional normative requirements.

## Access contract

The following decisions govern the exact records, normalization rules, transfer
inventory and closure matrix below. Author models establish bounded algebra and
wire evidence; implementation and its source-level owner tests remain unshipped.

| Surface | Required contract |
| --- | --- |
| Analysis owner | MIR derives a typed access-only control-flow program from the actual structurally validated body. Source checking, interface publication, cached-body qualification and native emission use that owner. |
| Existing validation | Producer initialization, ownership, lifetime, no-alias, range, signature and cleanup validation remain separate prerequisites. Neither an absent read-only marker nor `XmlAccessProvenance::Owned` is backing-writability proof. |
| Local values | Point-specific descriptors distinguish fields, tuple positions and active tagged payloads. Completed loads are snapshots. Definite descriptor replacement replaces its selected path and preserves siblings. |
| Descriptor permission | Each selected view leaf carries `Writable`, `ReadOnly`, `Formal` or `Unavailable` alternatives separately from its addressed locations. A safe write requires exactly `Writable` on every reaching concrete alternative. Text-to-bytes always publishes `ReadOnly` without changing an older writable alias. |
| Addressed contents | Descriptors refer to finite abstract locations. Aliases observe content writes. A collection view's permission is separate from its contained view profiles. Dynamic element updates conservatively join contents. |
| Byte validation | Text and codec view leaves carry observation identities with backing dependencies and `Live`/`Ended` alternatives. Calls transport dependency closure, including caller observations reached backwards from byte backing. Ordered writes end overlapping observations; later validated-view use requires only `Live`. Owned text copies detach dependencies. |
| Inline arrays | Loading/storing a fixed array copies its inline element profiles. Forming a slice of a slot refers to that slot's storage; it does not copy the storage. Contained view descriptors continue to refer to their original backing. |
| Calls | Preserve ordered write observations, normal-return reachability, returned descriptors, changed caller places and heap-content effects. A later replacement cannot erase an earlier write obligation. |
| Higher-order calls | Preserve selected target and trailing capture mapping together. A formal callback is a conditional application to be instantiated by its caller. A genuinely unavailable actual target is not an empty effect. |
| Native operations | Use exact retained-input, fresh-storage and projection transfers. Owning an opaque native handle does not establish the writability of every view it returns. |
| Ownership and ABI | Compiler-only analysis metadata changes no source allocation, copy, move, nulling, replacement, Drop, runtime layout or native calling convention. Existing lifetime summaries must not become access summaries. |
| Interfaces | Export a closed parametric access program, including reachable private helpers without exposing new source-callable names. Bind the complete reachable layout and access-program graph into interface/cache identity. |
| Diagnostics | Reject a reachable write whose instantiated descriptor permission includes a read-only or unavailable alternative. Locate the source operation and relevant call; emit one root error per operation. Readers and explicit owned copies remain valid. |

The existing all-store producer equations deliberately join every store to a
slot. They cannot implement point-specific descriptor replacement. Reusing their
structural/native inventories is required; reusing their all-store union as the
new control-flow transfer is incorrect.

## State and evaluation strategy

Keep point-specific descriptor places and dynamic retained contents in distinct
domains. This is not a new complete memory-initialization proof. The existing
producer readiness proof establishes founded inputs and rejects self-founded
call/store cycles; it does not prove that every dynamic index has been written
on every control path. This capability preserves that prerequisite and adds
actual backing-origin checks, without claiming a stronger initialization theorem.

A point state contains the function-local slots and completed value definitions,
active tag alternatives, finite scalar control facts and exact addressed local
places. A definite local/field/fixed-index descriptor store replaces its selected
profile. Loads already completed retain their descriptor snapshots. A slice of a
fixed-array slot continues to refer to that slot's current contents.

Dynamic allocations, builder contents and other retained opaque carriers keep
weak content cells in the point state. A store joins its completed producer
profile into the selected dynamic cell; it cannot remove an older alternative
without an exact singleton place proof. Aliases in that state share the cell.
Later CFG states and call returns carry the changed graph. An earlier state is
immutable: no context-global content table may inject later descriptors or live
validation observations backwards into it. Collection permission remains separate
from contained profiles and unrelated siblings.

An empty content cell means no founded element descriptor is known, not a writable
descriptor. A known empty collection can return without reading an element.
Scalar zero/nonzero facts and descriptor lengths preserve the empty/absent branch;
a bounds-failure branch does not continue to an element read. A required
view-bearing read with no founded profile cannot pass. A suspended call waits for
its normal returns rather than inventing an empty or writable result. Existing
producer initialization remains the separate prerequisite; one weak element
summary does not claim that every runtime index has been initialized.

The existing `collect` lowering exposes its constructed length as the same counter
incremented after each element store. A zero-iteration path returns length zero;
a nonempty result path has a completed element producer. The author MIR trace of
`slice<i64>.map(fn value { text(bytes) }).to_array()` confirms that ordering through
`HeapAllocBuf`, `PtrStore` and `MakeDynArray`. This must remain an owner covering
empty and nonempty inputs, fallible callback returns, filters and repeat-site
validation. It is not a general theorem about arbitrary lowered loops.

Each normalized context and CFG point owns an immutable factored relation over
its current typed coordinates. Join grows that relation; changed point facts
schedule successors and selected calls, and a newly founded callee return
schedules its saved continuations. The worklist does not enumerate every complete
combination of independent field choices. Saved predecessor states remain
immutable. A clean verdict is available only after the worklist reaches a fixed point;
an early provisional writable result does not authorize publication. Writing
scalar bytes checks the completed descriptor permission, not prior scalar element
initialization.

### Factored point-state representation

A point state is an immutable typed graph plus a reduced decision relation over
its current facts. Join is relational union after complete frame alignment.
Permission is paired with its backing, a callable with its target and captures,
and an observation with its dependency and Live/Ended status. Do not replace
these relationships with independent field marginals. A branch selector may be
temporary within an image, but no selector or runtime execution ordinal survives
as historical state.

The required source precision has three controls: a selected record can pair a
writer with writable bytes or a reader with literal bytes; selecting one such
row from a mixed array preserves its own pair; selecting fields from different
rows of that mixed array must expose the unsafe combination. A branch choosing
an all-writer/writable array or an all-reader/literal array also preserves that
current homogeneous-world correlation across two reads. These compile-only
controls do not require exact representation of arbitrary collection worlds.

**Heap join.** Carry a symmetric may-coexist relation. A diagonal means Many.
Alternative executions contribute their own pairs; union adds no cross-arm pair
merely because both nodes occur in the union. Fresh allocation coexists with
currently possible objects. Heap quotienting maps the pairs and marks a merged
diagonal Many. Remove unreachable nodes and pairs together. An existing Many
node cannot recover singleton authority by losing an alias. A new distinct
allocation may be singleton. The relation is not a reachability root.

**Relational images.** Definite replacement existentially removes the old
destination after selecting its completed source, then constrains the new
destination. Saved values and siblings remain. Dynamic or Many destinations
use weak updates. Noninjective heap maps combine each output fact's preimages
within the same image. Injective maps use direct substitution; identity maps
reuse their relation. Do not build a large old/new equality product for an
identity or copy only to project it away afterward.

**Profile bindings.** Use the shared inline profile DAG. Copied values may share
a binding; equal marginal values do not make two independent selections the
same binding. Normalize each incoming frame and rename it apart before joining.
Zip profiles with a memo local to that join. Map an old binding directly to one
output representative, constrain additional output copies to that representative,
project unused coordinates, then union the arm images. A universal output arm
absorbs the other arm before unnecessary copy equalities are materialized.
Canonical live ordinals follow deterministic traversal of retained profile
positions. Unreachable state is one bottom with no binding partition.

**Collections.** Keep current collection-wide uniform flags and their guarded
values in the outer relation, plus a complete row relation with bound local
coordinates. Each independent selection instantiates those row coordinates into
the destination's completed-value positions. Preserve other snapshots and current
facts; projecting them merely to extract a row schema would lose correlation.
A false uniform flag means absence of a certificate, not proof of two different
elements. Zero/nonzero presence stays correlated with available row contents.

Insertion updates uniform facts and row alternatives in one image. A field is
uniform when the old collection was empty, or its old uniform value equals the
inserted field. Canonicalize unused uniform-value padding. Union old nonempty
row alternatives with the inserted complete row before removing temporary old
storage coordinates. This is a weak element operation; a whole-element update
uses the same conservative content rule unless an exact singleton indexed place
has independently been proved.

When coexisting heap cells merge, combine their complete row alternatives in
that world; a resulting field is uniform only when every nonempty input is
uniform and agrees. Alternative point states instead union their guarded worlds.
Never OR the uniform flags of coexisting cells. A retained snapshot may distinguish
two heap roles; releasing it requires content recomputation if the roles merge.

**Calls and observations.** Normalize the incoming closure using parameter roots
and the fixed UnknownObservers root before adding protected input anchors.
Preserve the saved caller relation. Relate normal-return input anchors to that
caller's old current facts before replacing changed places/contents/observations
and copying returned values; only then remove temporary callee coordinates.
A missing normal return is bottom. New return alternatives reschedule the
affected callers, including recursive calls. No provisional writable return
authorizes publication. Returned Ended facts cannot reset a caller's old view.

Rebuild reverse observation dependencies from retained forward roots before call
closure. Reverse-index entries and old decision variables are not ownership or
reachability roots. Retained known observers travel with their backing closure;
UnknownObservers covers retained observations whose backing is unqualified.
Revalidation creates a distinct current observation while preserving every
retained older observation. The explicit unsafe boundary below separately fixes
what is and is not claimed about arbitrary foreign mutation.

Normalize heap roles, merge coexisting contents, then canonicalize current
profile/row bindings. Project a coordinate when no current root, heap content,
row schema, active snapshot or protected input anchor retains it. Projection may
add conservative possibilities; it cannot establish uniformity, drop ReadOnly
or Ended, or restore singleton authority. The complete finite bound is specified
under tabulation below.

| Saved author evidence | Established result | Limit |
| --- | --- | --- |
| Point/heap joins | Rooted edge preservation, Many versus alternative Singleton, 200 join-algebra triples and 64 independent choices. | Bounded layouts; no production memory guarantee. |
| Relational updates | Concrete-oracle agreement for three-bit copy/overwrite and 1,024 two-cell update cases. | Not a complete source interpreter. |
| Profile DAG and binding join | Depth 4,096 remains shared; repeated sites stabilize; separate joins retain independent choices. | Inline binding algebra, not every native producer. |
| Scoped collection and heap composition | All 256 small world pairs are sound (234 exact); coexisting content union and retained-root role changes preserve the controls above. | Conservative for 22 world pairs; no exact arbitrary-set promise. |
| Anchored collection calls | 1,024 calls retain old snapshots and changed returns, including conditional Ended and shared callee inputs. | One bounded content layout. |
| Recursive tabulation and observer GC | Founded-return scheduling, late ReadOnly/Ended propagation and non-owning reverse dependencies. | Full source/heap/native composition is an implementation owner. |

Reduced decision relations can be exponential. The representation removes
unbounded histories and avoidable independent-state enumeration; it does not
promise polynomial space, a practical maximum context count, or a larger test
budget. A resource failure must remain a failure, never a clean verdict.

### Compact inline profiles and rooted path sets

The inline flattening below is a semantic definition, not permission to enumerate
every root-to-leaf type path. [Plan 19](19-hir-validation-ledger.md) accepts deep
acyclic type chains and shared header-mediated DAGs without an ambient type-depth
cap. A nested Result whose two payloads reference the same preceding definition
has exponentially many paths despite its compact type graph. This analysis must
retain that graph and the existing stack-bounded admission contract.

Store an inline value profile as an immutable, structurally interned typed DAG.
Its leaves hold scalar control facts, descriptors, callables or references to
addressed heap nodes. Equal inline subprofiles may share a node; that sharing is
not aliasing of their physical source places. Copying a value copies its profile
root. Replacing an exact field constructs a new path of profile nodes and shares
unaffected children, preserving the old completed value and all sibling fields.
Addressable slot/field identity is separate from profile-node identity. Heap
content cells hold profile roots; replacing one cell cannot mutate an immutable
profile shared by another cell or completed value.

Fixed-array profiles use normalized disjoint index runs with a default shared
child, not one node per type-level index. An exact index update splits only its
containing run and merges adjacent equal runs afterward. A dynamic index joins
all selected alternatives and uses the existing weak update rule. Neither a
large declared extent nor a shared type DAG may force eager element/path expansion.
All profile creation, selection, comparison and replacement use explicit work
items rather than native recursion. Follow header/callable references as graph
edges; never recursively inline those definitions.

The role alphabet's inline paths are likewise represented as finite typed path
languages. A role label is the tagged tuple `(root ordinal, initial inline path,
entry-or-last-edge)`, where a last edge contains its content layout and inline
path. Encode the two path components with disjoint separator/segment tags in a
canonical acyclic path DAG. Field/tuple/variant segments retain their exact
ordinals; fixed indices use normalized interval transitions. Thus two nested
siblings with the same final field name remain distinct. `entry` and the fixed
unknown-observer root are explicit alternatives, not empty or wildcard paths.

Role propagation unions these languages. A traversal pairs the reaching initial
root paths with the selected last-edge paths; it does not append an unbounded
history of heap traversals. Tagged concatenation and union use memoized explicit
node-pair worklists and canonical nonoverlapping transitions. Equality means
exact equality of the represented finite path set, not equality of the last
field name or a truncated path hash. The location bound below still uses the
conceptual finite alphabets; neither alphabet is materialized to allocate the
worklist. Analysis profiles and role languages are not serialized interfaces.

An isolated author model checks depths 2, 256 and 4,096, preserving exact selected
and deep sibling fields, earlier copies, path-set union and the product of initial
and last-edge paths. At depth 4,096 it retains 8,194 value nodes and 24,579 path
language nodes; it never enumerates the exponentially many represented paths.
A fixed extent of 2^31 uses three runs after one selected update and one run again
after restoration. These are author algorithm checks, not runtime benchmarks or
a proof of the combined heap/call interpreter. `view_access_layout_matrix` must
close deep/shared DAG and large-extent cases, with the combined role/alias owner
checking that compressed representation preserves the quotient's identities.

A second author model combines immutable descriptor profiles, compact root/edge
path languages and the recursive heap quotient. At depths 2, 64 and 4,096 it
collapses a six-location recursive chain to two locations while preserving each
descriptor's own target/permission pair, a selected read-only alias, writable
siblings and an earlier Copy profile. At depth 4,096 this case uses 8,202 profile
nodes and 12,305 path-language nodes; it does not enumerate inline paths. Nested
initial paths and nested last-edge paths with equal final field ordinals remain
distinct. Joining locations preserves `Many` and the union of node properties;
that property union does not move descriptor permission onto a heap node.

Anchored call/return cases use that same compact representation at all three
depths. They transport aliased arguments, one exact selected-cell replacement,
fresh returned backing, disjoint siblings and an older caller Copy. Two
descriptors to the same backing retain different permissions across the call.
Returning a singleton-looking value into a caller's `Many` input uses a weak join
and retains an old `Ended` alternative. These checks cover representation and
translation; they do not admit the modeled writes, prove typed formal-call
tabulation or establish collection initialization. Those remain separate owner
obligations below.

### Descriptor permission and byte-validation observations

A view profile pairs its addressed-location alternatives with permission
alternatives. Permission belongs to the published descriptor, not to the storage
node. Inline copies, subviews and call translation preserve that pairing. A new
owned byte-element copy gets fresh storage and `Writable`; copying a descriptor
preserves its permission. Text-to-bytes produces `ReadOnly` at the same backing
location. It neither changes the original writable byte alias nor detaches the
text's source lifetime. A `Formal` permission is a declaration obligation and
must be concretely instantiated before a closed write can pass.

The graph has distinct storage and validation-observation node layouts. Text
and codec-view leaves name zero or more validation observations in addition to
their byte backing. Each observation has an immutable UTF-8 or codec kind;
quotienting never merges different kinds. Codec batch/column/name/cell projections
preserve the envelope observation. Completed scalar accessors retain no such
obligation, and reopening creates a new observation without reviving old views.
Native/owned/static text that does not arise from mutable-byte validation needs
no such dependency; its always-valid text contract is preserved independently.
An observation records `Live`/`Ended` alternatives and edges to the completed
backing alternatives. Each backing has a reverse observer edge. These reverse
edges are analysis metadata, not runtime ownership or allocation.

Reverse observation edges do not keep an otherwise unreachable observation
alive. First determine retained observations from the point's ordinary value,
place, snapshot and protected-input roots through forward profile/dependency
edges. Rebuild the reverse index over that retained set; only then form the call
closure below. The distinguished UnknownObservers root is likewise derived from
retained unknown observations, not preserved as an independent history of every
validation ever created. Conditional presence remains conditional throughout.
Callee input anchors can retain a caller observation for returning its mutation,
but a stale reverse-index entry alone cannot serve as that anchor.

Before a call context is normalized, take the fixed point of argument-reachable
storage, contained profiles, observations and both directions of their dependency
edges. This includes a caller's validated text even when only its byte alias is
passed to a hidden writer. Use the callee's fixed parameter roots for this
closure; do not add caller-local names or unbounded caller observation ordinals
to the context key. Input anchors and the saved caller map retain every included
node. Updated observation validity returns through those same anchors. A callee
that creates text exports its new observation and backing edges with the result.

Unknown backing dependencies use the distinct `UnknownBacking` layout. Every
call closure additionally seeds one fixed `UnknownObservers` root selecting all
reaching unknown-backing nodes. This root has no caller-local ordinal. Its
observer edges include validations that may overlap a passed known byte view
even though no exact location edge can establish that alias. An admitted byte
write invalidates these observations conservatively. The root is absent only
when no reaching unknown observation exists; absence is not inferred from an
empty parameter list. Callee-created unknown observations return with their text
and become part of the caller's next dependency closure. Explicit owned copies
have independent dependencies. Formal declaration obligations remain distinct
from this concrete unknown-backing case.

A successful validation makes a new live observation of its completed input.
The error alternative carries none. A byte write ends every possibly overlapping
observation in the current point state; uncertainty cannot prove disjointness.
Ending validity does not end storage or invalidate raw byte descriptors. A validated codec accessor, text
read, native text consumer, or conversion checks observation validity at that
operation's position. Earlier completed scalar results do not become invalid
retroactively. Eager text operands remain subject to validation at their consuming
operation after all argument evaluation, matching plan 57.

A text-to-bytes operation first checks its text operand, then drops only its
UTF-8 and codec validation observations from the new byte profile. A later raw byte read does
not consume the old text. An owned text clone first checks the source and then
publishes independent text. Borrowed text derivatives preserve observations.
Subranges conservatively depend on the same backing, as in plan 57.

Observation status belongs to each point state. It is not a globally ended bit
that could retroactively invalidate an earlier CFG state. Joining reaching states
retains an ended alternative; a later write cannot clear it. New validation must
remain distinct from retained older observations. The existing quotient retains
result/local/input roots, merges observation nodes only within their own layout
and unions validity alternatives. Reused finite identities must never reset an
old observer to live. The creation/normalization rule below supplies observation recency. Full lowered materializers and formal declaration instantiation still need their
complete owners;
the storage-only model does not close those axes.

An author-only extension of the call/quotient model checks temporal, collection and
permission cases: local/hidden writes, callee-created text, revalidation and old
aliases, earlier completed reads, disjoint backing, validation/write ordering,
identity transport, raw-byte reuse, branches and recursive observation creation.
Fifteen codec cases cover caller/callee validation, local/hidden mutation, reopening,
completed scalar uses, borrowed text derivatives, owned copies and mixed UTF-8/codec
observations. Their kinds remain distinct during quotienting; a fresh observation
of either kind cannot revive an ended observation of the other kind. Twelve
concrete-callback cases cross both validation kinds with reader/writer/nonreturning
targets and caller-created/callback-created observations. Nonreturning callbacks
produce no invented continuation. These cases do not yet model formal audit state.
Separate descriptor nodes prove that a read-only text byte view can share storage
with a writable older alias. A recursive reader reaches four contexts and 24
states; recursive invalidation reaches three contexts and 19 states. These are
bounded model evidence, not runtime benchmarks or a complete compiler proof.
The model omits native error alternatives, source eager-operand lowering,
typed formal alias/output dependencies and full lowered materializer loops. Seven collection
cases additionally check retained callee validation, no backward content flow,
empty collections and repeated validation. A loop retaining new observations
reaches 24 states; one with hidden mutation reaches 33 states and rejects stale
retained text. These use point-specific weak contents, not the superseded global
may-content proposal. Six unknown-backing cases check direct/hidden writes, earlier text reads, returned
unknown observations and detached owned copies. Full compiler owners are required before publishing the implementation.

### Typed view conversions and native operands

Derive a conversion from the structurally checked stored/operand type and the
selected result type. `Use` preserves ordinary same-typed profiles. The existing
`xml_ty_is_view_retype` relation admits `string -> str`, text-to-byte slices,
owned-array-to-slice views, the admitted Option lift and request-context-to-header
views. Text-to-byte pairs apply the permission/observation rule above;
array-to-slice pairs preserve the collection's descriptor permission and contained
profiles. Header views select their native owner schema. None is a new copy.

`Operand::BorrowedPlace`, `BorrowedElementPlace` and `BorrowedFixedElementPlace` can carry a
retyped selected view without an intervening `Use`. Recover the actual stored
leaf from the checked slot/element projection before applying its requested view
type. Reuse producer validation's exact projection and retype relation, including
its owning-view exclusions. The requested `place.ty` alone is not evidence that
an original text-to-bytes conversion did not occur. Checked-MIR owners must
mutate both explicit `Use` and these embedded retypes.

Native inputs use the actual operation schema and actual operand type, not their
C ABI descriptor shape. `ProcessLiveKind::inputs` distinguishes `Text`, `Bytes`,
`Argv` and `OutBytes`; `FsTreeKind::inputs` likewise distinguishes text from raw
path components. Their existing MIR lowering retains each source operand's type.
A Bytes input reads bytes without requiring UTF-8. A Text input consumes text
validity; Argv consumes its selected text elements. OutBytes checks destination
permission and invalidates overlapping observations without reading unwritten
elements. Scalar/owner inputs use their exact declared role. Keep the producer's
shape, ownership and native scratch/result checks as prerequisites.

### Source-use timing and eager completion

A value use is not limited to a native UTF-8 dereference. Preserve the current
local observation contract: using stale text's length, an array carrying stale
text, or an ignored aggregate argument still rejects. Selecting an unrelated
record field remains independent. Check the forward contained-view/capture
profiles of the source-selected value. Reverse observer edges support mutation
invalidation; they do not impose validation obligations on an ordinary byte alias.

The current MIR is insufficient to recover every source use. At merged
`66d5c2b68554963ba607af0e703fe3c8cf2f2d4a`, a fixed array of records containing a
validated `bad` text field and independent `good` text field demonstrates this:
`write(alias); print(items[0].good)` and
`print(items[{ write(alias); 0 }].good)` emit identical MIR. Their local-write
controls differ: mutation before evaluating the array rejects, while mutation
inside the index followed by selection of `good` passes. Both hidden-write
versions currently pass whole-program and per-unit checking. The identical MIR
cannot distinguish the missing first rejection from the required second success.
Source spans, instruction order guesses and blanket whole-array validation at the
final `IndexField` cannot repair that loss.

Checked-HIR lowering must therefore retain compiler-only source-use events in
MIR before the relevant index or eager operand is evaluated. These are semantic
analysis operations with no runtime instruction, aggregate materialization,
allocation or copy. A source place can be observed without loading a Move
aggregate into SSA. The event must name a structurally validated typed value or
place, never an asserted validity bit. The exact event records and complete
lowering/consumer inventory below define this boundary.

Immediate use and eager completion are distinct operations. Immediate source use
checks the selected source profile at that position. A completed eager operand
retains its completed value profile and captures the observations that are live
at completion; subsequent overlapping writes end those captured observations.
The enclosing action checks the retained value and captured observations when
that action is reached. It must not reload a reassigned source local or capture
an already-ended unrelated observation as a new obligation. Ordinary local array
field selection uses the source array before evaluating its index, then selects
the exact result field. Borrowed indexed arguments additionally reserve their
source before index evaluation and check that reservation on completion, matching
`BorrowedIndex`; this reservation is not interchangeable with ordinary selection.

The existing semantic checker supplies the ordering owner:
`expr`/`expr_eager_worklist` validate child snapshots at the enclosing action;
`record_value_completion` captures live roots and adds borrowed-element source
roots; `finish_child_staging_frontier` removes validation-only child obligations
from non-borrowing results and `StrBytes`. Scalar results, explicit owned copies
and text-to-bytes therefore do not carry every intermediate validation obligation
into a later outer action. Control wrappers defer checks to reached operations;
an early exit does not invent a later enclosing use. Mutable actions exempt only
the exact destination's intended transition, not other operands sharing its old
observation. Reader actions retire their consumed input snapshots explicitly.
These rules must be translated into typed MIR event semantics, not reproduced by
an independent source-expression classifier with different defaults.

Source admission and exported access-program derivation run on initial validated
MIR with these events, before optimizations erase source-only uses. Whole-program
and per-unit checking share that phase. Preserve its derived source program as
the interface behavior record; an optimized native body cannot replace it with
weaker source obligations. Native emission still validates its actual MIR.
Existing compiler/artifact identity binds source program, body and emitted object
through the normal cache boundary. Remapping, inlining and CFG transformations
must preserve event identities and ordering until the source program is derived;
only the subsequent execution lowering may erase an already-discharged event.

The author temporal model exercises immediate source use, live-only completion
capture and enclosing checks for UTF-8/codec observations through local and hidden
writes. Before/index/later mutations select both dependent and independent fields;
scalar, byte-conversion, owned-copy and early-exit controls release the inner
obligation. These model cases do not establish the full lowering event inventory.
The fixed shared Copy-field computed-index route now uses the existing
bounds-checked field read and a call-local Copy slot (plan 56). Its receiver-shape
guard also rejects malformed internal HIR before slot resolution. Source timing
controls can use this route; the original ordinary fixed value-field projections
already establish the identical-MIR counterexample independently.

The closure matrix must cover the new MIR shape across type validation, value and
place remapping, serialization, printing, every analysis visitor, optimizations,
monomorphization and native lowering. Owner pairs must distinguish before-index,
inside-index and later-argument writes for fixed/dynamic ordinary and borrowed
projections, plus folded lengths, discarded values, aggregate/closure construction,
branch/loop joins, pipelines and early exits. The reproduced identical-MIR pair
is a required lowering regression: its access events must differ at the source
use while its generated runtime behavior remains unchanged.

### Source-event contract

This record closes the observation part of source event semantics; the
checked-HIR placement inventory below fixes its evaluation boundaries. Keep one
MIR statement variant containing a closed event enum. Its logical cases
are below; canonical bundle tags belong to the unified wire record, not a second
independently versioned source-event codec.

| Event | Fields | Transfer |
| --- | --- | --- |
| Use | `subject: AccessSubject` | Reject an ended alternative in any forward validation observation of the selected subject. Do not inspect reverse observer edges or unrelated projected siblings. No graph state changes. |
| CaptureLive | `snapshot: SnapshotId`, `subjects: sequence<AccessSubject>` | Bind the snapshot to the union of currently live forward observation identities of the named completed value and explicitly required storage sources. Retain references to those observations; do not recompute the subjects at a later check. An already-ended extra storage observation is not captured. This event is not an immediate validity check. |
| Rebind | `snapshot: SnapshotId`, `inputs: sequence<SnapshotId>` | Move the union of the named active capture sets into an inactive destination ordinal and release those inputs. Preserve already-ended identities; this is transport, not a fresh live-only capture. |
| Check | `snapshots: sequence<SnapshotId>` | Reject if any captured identity has an ended alternative at this point. An empty captured set passes. No snapshot is consumed implicitly. |
| Release | `snapshots: sequence<SnapshotId>` | Remove these validation-only roots from the active snapshot environment. Existing lifetime, storage-generation, staging-release and cleanup proofs are separate and are not removed by this operation. |

`SnapshotId` is a function-local `u32` ordinal allocated deterministically by the
lowering traversal, not an address of an HIR expression, source offset or runtime
generation. The function declares its snapshot count. Each dynamic execution of a
capture or rebind requires its destination ordinal to be inactive; every input,
check and release requires an active ordinal. Control-flow validation checks this discipline on every reachable
edge. Reaching a loop backedge requires release of snapshots local to that
iteration, while snapshots belonging to an enclosing eager operation remain
active. Recursive function invocations have separate local environments; they
cannot overwrite their caller's live roots. Return/unreachable discard the local
environment without inventing an enclosing action check. Explicit value return
checks precede the return transfer when that source action requires them.

A snapshot sequence is strictly increasing, duplicate-free and may be empty.
Subject sequences retain lowering order and may not contain structurally identical
duplicates; an empty subject sequence creates an active empty snapshot. `Rebind`, `Check`
and `Release` evaluate the full sequence against the pre-event environment;
malformation is rejected before any state mutation. At an ordinary CFG merge,
active ordinal sets must agree; captured observation alternatives join. Lowering
must therefore consolidate branch-local retained obligations with `Rebind` into
one common destination ordinal on each normal outgoing edge. This includes
required storage observations of completed children, not just the final selected
value. An empty input set is valid only when that reached branch completion
carries no retained validation obligation. A terminating branch emits no join
publication. Re-capturing only the joined result would lose invalidated child
storage observations and is forbidden.

`AccessSubject` names either an already-defined typed MIR value or a validated
place rooted in a slot/borrowed binding with its complete projection. It carries
no supplied writability or observation bit. Reading its analysis profile emits
no load, move, copy or initialization. The same structural projection owner used
by ordinary accesses derives its selected type and validates ids, active payload
requirements and index form. Subjects have no requested-type override. An
ordinary typed conversion retains its existing admitted retype rule and produces
a value subject without hiding its input use. Subject resolution preserves
completed value snapshots versus current place contents.
Its exact compact projection record is specified with the access program below.

Source use of a local fixed-array receiver can emit `Use(place)` before index
evaluation. Completion can emit `Use(selected value)` followed by
`CaptureLive(result and required storage)` without re-reading the receiver as a
new immediate source use. Later eager arguments invalidate captured identities
through ordinary local/call writes. Enclosing action checks run before its own
intentional destination transition; any post-action checks retain the existing
exact-destination exemption. Inner scalar/owned-copy/StrBytes completion releases
validation-only child snapshots before outer operands begin. Pipeline source
snapshots survive to their reached terminal action. These placements must be
mapped separately for ordinary `ElemField`, explicit `BorrowedIndex`, constructor
staging, reader actions and each control-expression lowering.

The temporal model now exercises `Rebind` across ended child observations,
non-result storage dependencies, scalar cutoff, loop-local release with an outer
snapshot, and selected/unknown branch alternatives. All 146 temporal cases pass; the imported quotient model separately checks
4,096 bounded graphs and five recursive-chain sizes.
A separate ordinal-liveness model has 16 positive/negative cases for missing,
duplicate, overwritten and unreachable invalid ids, branch consolidation,
terminating branches and loop release. These are author models, not completed
structural or checked-HIR lowering owners. Event instrumentation must not require storing
an exponentially expanded list of inline view paths; subjects select the compact
profile DAG already specified above.

### Source-event placement inventory

The baseline `MoveCheck` sites below establish placement requirements. This is
an inventory of explicit source-use and snapshot boundaries, not a replacement
for the exhaustive expression/native transfer classifier. Any shared recipe
extracted from these sites must be used by the existing local checker and MIR
lowering, with the HIR variant tripwire covering its dispatcher. Do not introduce
an independent permissive default for an unlisted expression kind.

The existing mutable-completion registry is a pure body scan, not dynamic borrow
state: `prepare_mutable_call_snapshots` visits HIR expressions and reads named
parameter modes, Fn parameter modes and closed native input kinds. Extract this
scan and the completion predicates into one compiler-internal
`SourceEvaluationIndex<'hir>` used by both MoveCheck and initial-MIR lowering.
The index borrows the final HIR and stores per-expression flags plus exact child
references; its expression-address lookup is invocation-local and is never
serialized, hashed or used as authority beside another HIR body.

Its per-expression record contains `value_snapshot: bool`,
`completion_snapshot: bool`, `defer_child_check: bool`,
`ownership_action: bool`, `drop_child_byte_observations: bool`,
`intentional_destinations: Vec<&'hir Expr>` and
`retired_reader: Option<&'hir Expr>`. The first two flags retain the existing
`value_snapshot_needed`/`completion_snapshot_needed` predicates, including
BorrowedIndex, exact mutable-place reservations, builder/header/carrier paths,
dynamic arrays and borrowed types. The cutoff is exactly `!value_snapshot` or
StrBytes; do not replace it with a blanket owned-result rule. Ownership-action
child retirement remains a separate flag, covering calls, constructors, LogNew,
XmlParse and Closure under the current dispatcher. The typed DbBridgeCall
replaces RawCall in that dispatcher.

The policy record is a total structural function after checked-HIR validation:

| Field | Exact rule |
| --- | --- |
| `value_snapshot` | BorrowedIndex, a registered mutable actual root, array-builder type, any storage header/carrier, expanded DynArray/DynStructArray type, or `ty_may_borrow`. These are OR alternatives. |
| `completion_snapshot` | `value_snapshot` OR membership in the mutable-call argument registry. |
| `defer_child_check` | Block, Arena, NamedArena, TaskGroup, Unsafe, If, Match, ElseUnwrap, Loop, or Binary And/Or. All other variants are false. |
| `ownership_action` | Call, CallFnValue, DbBridgeCall, StructLit, Tuple, ArrayLit, OptionSome, ResultOk, ResultErr, EnumValue, LogNew, XmlParse or Closure. All other variants are false. |
| `drop_child_byte_observations` | `!value_snapshot` OR StrBytes. This removes byte-validation obligations only; it does not release a retained lifetime or runtime owner. |
| `intentional_destinations` | Exact BorrowMut argument expressions in argument order for Call/CallFnValue; otherwise the exact destinations of the closed source-visible mutation record, in its declared input order. No match produces an empty vector. |
| `retired_reader` | The exact `reader` child of XmlNext, XmlName, XmlAttributeCount, XmlText, XmlAttributeName or XmlAttributeValue; all other variants produce None. |

Compute registry membership first, then these records in a bounded HIR walk;
record construction must not depend on the current borrow graph or worklist
iteration. Storage-header/carrier presence is a compact type-layout predicate,
not a flattened list of all inline paths. A malformed layout is an earlier
validation error, not an empty storage inventory. Explicit no-action variants
share a closed match arm: the HIR variant sweep still requires each new variant
to choose its structural policy rather than inheriting a wildcard default.

Registry construction marks all arguments of an ordinary/indirect call containing
BorrowMut or Out as completion candidates, and only exact BorrowMut actual
places as place reservations. It also preserves the current FsTree owner,
ProcessLive Owner/OutBytes, CryptoDigestUpdate receiver and XML reader-action
cases. In this registry, reader-action retirement means exactly XmlNext, XmlName,
XmlAttributeCount, XmlText, XmlAttributeName and XmlAttributeValue; it is not an
exemption for every operation on an I/O reader. Intentional destinations use
exact expression identity from BorrowMut parameters or the closed source-visible
mutation action. Another argument with the same backing remains subject to its
normal check.

Keep unresolved mode information explicit during generic declaration work. Final
source derivation requires every retained call's modes to be supplied by its
validated local/imported/extern declaration, concrete Fn type or the existing
closed builtin contract. Print, hash64 and hash128 intentionally remain HIR Calls
without declaration records; each has one ByValue argument under its existing
checked-HIR producer validation. Their closed handling must not become an
all-ByValue default for an unresolved name. The current MoveCheck named-mode map
contains local and imported functions; the shared resolver must also account
explicitly for the extern and builtin paths used by final HIR validation. Current
extern formation supplies an all-ByValue signature explicitly. Its absence from
MoveCheck's mutable-mode registry is therefore not evidence of an admitted extern
BorrowMut/Out hole, and this extraction introduces no new extern parameter mode.
Extract the shared selectors
without changing the existing earlier generic-declaration suspension behavior.
MIR-created temporary expressions carry their original lowering context; they
cannot mint a second source evaluation merely by copying an expression or span.
The index owns completion policy, while the source-use and control-flow placements
in the table below still own where events are emitted. Cached operand and borrowed
place shortcuts must consume that same policy even when they emit no ordinary
MIR load. The remaining exact extraction/dispatcher owner must cover all of these
consumers together.

| Source boundary | Existing semantic owner | Required placement |
| --- | --- | --- |
| Whole local read | `expr_inner(Local)` / `check_borrow_use` | Use its current whole profile before completing the expression. A moved/uninitialized local remains the existing earlier error. |
| Record field read | `check_borrow_projection_use` | Use the complete selected field path only; an ended sibling does not poison it. |
| Slot-backed native selection | `SoaColumn`, `ArrayGroupAgg`, `ArrayGroupAggMulti`, `ArrayDictEncode`, generated `IndexField` | Use the named base at the source action; these variants have no explicit receiver child to carry that event. |
| Ordinary indexed field with local receiver | `expr_inner(ElemField)` | Use the whole source base before the index; do not fabricate a completed whole-receiver child across index evaluation. Result completion selects its field and captures required live storage roots. |
| Other ordinary index/field receivers | `expr_inner(Index)` and the non-local `ElemField` arm | Preserve receiver completion before index evaluation, the exact projected result and the existing child frontier checks. Do not apply the local-receiver shortcut to an arbitrary receiver expression. |
| Explicit indexed Move borrow | `expr_inner(BorrowedIndex)` | Use the source, capture its required reservation before index, check/release it after a falling-through index, then publish the completed borrowed operand. Existing storage-generation and bounds reservations remain prerequisites. |
| JSON encoding | `expr_inner(JsonEncode)` and its eager `Work::Finish` | Use and reserve the source before eager `max_bytes`; check only the reached encoding action, retire its reservation, and publish independent output. Source move/replace checks remain in the existing mutable-place owner. |
| Field/element assignment destination | `Stmt::AssignField`, `AssignIndex`, `AssignElemField`, `AssignElem` and iterative statement walker | Use the existing whole root before index/RHS evaluation, then preserve their written order and clear completed operand snapshots after the assignment. Exact field self-assignment can erase runtime code but not its source-use obligation. |
| Pipeline receiver/captures | `begin_pipeline_source_snapshot`, `validate_pipeline_capture_use`, `finish_pipeline_source_snapshot` | Preserve once-only receiver/capture evaluation in preheader order, capture completed sources, and check at the reached terminal action. Early exits release local obligations without inventing a terminal use. |
| Eager completion | `expr`, `expr_eager_worklist`, `record_value_completion` | Check the required completed child snapshots, freeze the result before its own ownership action where required, then capture live result/storage/backing observations. Caller-installed mutable replacements are published after the call action. |
| Completion cutoff | `finish_child_staging_frontier` | Release validation-only child obligations when the completed result does not borrow or is `StrBytes`. Preserve separately required lifetime and staging-release facts. |
| Mutable action / reader action | `intentional_action_snapshot`, `retire_reader_action_input` | Enforce pre-action inputs and exact-destination transition rules; retire the exact consumed reader snapshot. Neither rule clears unrelated eager operand obligations. |
| Control expressions | `defers_child_snapshot_validation`, control worklists, `finish_control_worklist_expression` | Emit checks at reached branch operations; consolidate retained branch-child captures with `Rebind` on normal edges. Preserve short-circuit order and loop-result transfers. |
| Explicit return / accepted break | Statement walker and completed-value transfer helpers | Check the completed returned/break value before transfer; close only the scopes actually exited. Enclosing eager operands survive an inner loop break. |

`lower_expr` currently has eager-result memoization, out-of-line dispatch, a
recursive fallback and dedicated control/template/pipeline spines. A wrapper
around only `lower_expr_recursive` misses source uses. A cached completed child
must not emit its events a second time when an enclosing lowering asks for its
operand. Conversely, `array_source_slot` and borrowed-place lowering can skip
`lower_expr` for named receivers, so they must explicitly consume the same source
recipe at that evaluation boundary. These are required lowering inventory axes.

Semantic fixpoint/replay walks revisit expression objects. Event identities come
from one deterministic structural lowering traversal; a repeated semantic walk
must not append another source event or choose a different obligation from its
current inferred roots. Recipes describe subjects and ordering, while MIR derives
actual observation identities from the instantiated graph. Generic/imported HIR
revalidates and derives recipes after concrete substitution. Expression addresses
may remain temporary lookup keys inside a traversal, but cannot be serialized or
used as cross-pass identity evidence.

The lowering integration has the following distinct owners. A shared event frame
must start before dispatch, and its completed result must include the operand
and retained snapshot ids. Returning a cached operand reuses that same completed
frame; it cannot manufacture a second capture. Helpers that deliberately bypass
ordinary dispatch consume the corresponding source boundary explicitly.

| Lowering route | Concrete owner | Closure requirement |
| --- | --- | --- |
| Eager enter/exit and cache hit | `lower_expr`, `expression_uses_eager_worklist`, `eager_worklist_children` | Begin one frame per reached expression evaluation, preserve source child order, and finish it once. A parent consuming a cached child does not rerun its Use or CaptureLive events. |
| Recursive and out-of-line dispatch | `lower_expr_recursive`, `lower_out_of_line_expr` | Both paths consume the same recipe; a frame only around the recursive dispatcher is incomplete. Termination suppresses the parent completion and remaining operands. |
| Borrow-mode controls and hidden owners | `lower_expr_for_borrow`, `lower_borrowed_owned_with_chunks_plan` | Retain the existing eager-child exclusion for borrow-mode If/Match/Else/transparent scopes. Do not emit both borrowed and ordinary branch copies. Event subjects do not introduce another owner or duplicate arena/task-group framing. |
| Named place and field selection | `lower_local`, `lower_borrowed_place`, `array_source_slot`, `borrowed_array_base_place` | Emit whole or selected Use at the semantic source boundary even when the runtime operand is a place and no Load is emitted. Form completion from the selected value plus the recipe's required storage subjects. |
| Ordinary and borrowed indexing | `lower_index`, `lower_index_field`, borrowed-element lowering | Base Use precedes index evaluation. Preserve separate ordinary receiver use, borrowed reservation/completion, computed Copy-field call slots, bounds-failure edges and later eager operands. |
| Direct, indirect and raw calls | `lower_direct_call`, `lower_call_fn_value`, `lower_raw_call`, `lower_consumed_call_arg` | Evaluate callee/arguments in their existing order and physical modes. Check completed operands at the call action; transfer exact mutable destinations and publish actual returned/replacement profiles at the proper side of that action. |
| Try, else, map_err and matches | `lower_try`, `lower_else_unwrap`, `lower_map_err`, `lower_match`, `lower_match_enum`, `lower_match_binary` | Retain only reached payload/callback/fallback edges. Rebind retained child obligations at normal joins; an early return or unreachable arm cannot publish a join completion. |
| If, short circuit and loop | `lower_if`, `lower_short_circuit`, `lower_loop`, statement Break/Return lowering | Preserve condition-before-branch order, short-circuit suppression, break values, iteration-local releases and enclosing eager snapshots. CFG active sets must agree at each join and backedge. |
| Optimized structural spines | `lower_unary_spine`, `lower_plain_block_spine`, `lower_wildcard_match_spine`, `lower_template_spine` | Explicit worklist frames carry the same source completions as the ordinary nesting they replace. Do not add recursive instrumentation frames to these stack-bounded routes. |
| Blocks, scopes and statements | `lower_block`, `lower_block_for_borrow`, `lower_stmt`, arena/task-group helpers | Statement completion and scope exits release only the matching validation frames. Existing move/null/Drop and task wait order remains authoritative. |
| Pipeline planning and terminals | chunks/parallel-source helpers, reduce/collect/map_into/partition/sort/group lowerings | A terminal consumes the source expression recipe once, before later pipeline operands; its retained source capture survives through the reached terminal action. Fused runtime iteration is not a second evaluation of source syntax. Callback bodies have their own function-local source frames. |
| In-place aggregate construction | `store_value_at`, `store_array_elems`, declaration/assignment and JSON params callers | Enter and complete the original StructLit/ArrayLit frame even when only its fields are lowered. Use the initialized destination projection as its completion subject, freeze required observations before the next sibling, and preserve the existing nulling/cleanup commit point. |
| Scalar folding and slot-only native producers | fixed-array length paths, `lower_json_encode`, SoA/group/dictionary helpers | Preserve semantic input use even if the result is folded or a helper consumes only a local id. A missing receiver child in the HIR/MIR record does not remove the source action. |
| Typed descriptor and support producers | semantic descriptor role and resource Drop access-function derivation | These compiler-owned operations have exact typed transfer contracts, not fictitious source expression spans or user-source snapshots. Their ordinary called hook/package bodies retain their own source events. |

Recipe selection must distinguish `value_snapshot_needed` from
`completion_snapshot_needed`. The former includes borrowed indices, explicit
mutable places, builders, storage headers/carriers, owned arrays and types that
may borrow. The latter additionally freezes mutable-call argument completions
that are not themselves parent value snapshots. Neither predicate may be
replaced by `ty_may_borrow` alone. Extract their structural type/place/mode
criteria into the shared recipe owner; use compact type-layout queries rather
than expanding all inline storage paths. The source's exact reader-input
retirement and child-frontier cutoff are separate recipe outputs.

`store_value_at` recursively expands nested StructLit nodes, and the Copy-struct
arm of `store_array_elems` lowers fields without calling `lower_expr` on the
containing literal. Each such original source node still has its own enter and
completion boundary. Its subject is the exact typed destination slot/path after
all its required fields have completed; CaptureLive freezes that point's
observations without adding an aggregate load. A later field or element that
terminates suppresses completion of its unfinished ancestors. The normal
Move-struct arm already stages `lower_consumed_call_arg`; reuse those completed
frames rather than entering a duplicate path. Source snapshots do not commit,
null or release these runtime owners.

Pooled arrays are a separate closed case: their existing admission permits only
scalar constants or a numeric literal's negation. Their omitted scalar child
frames contain no view or validation obligation. Keep that typed restriction in
the owner; do not generalize this shortcut to a constant-looking aggregate or
a computed expression. The ArrayLit completion itself remains available for a
pipeline's source frame. `Len` still evaluates its receiver recipe even when a
fixed length becomes an immediate constant. SoaColumn, ArrayGroupAgg,
ArrayGroupAggMulti, ArrayDictEncode and IndexField retain the local checker's
whole-base Use, because their HIR stores the base id rather than a receiver Expr.
Do not narrow that use to the returned projection solely from the helper's
native result shape.

Likewise, the local checker's move-neutral, transparent-spine, non-borrowing
binary-tree and control-worklist fast paths cannot justify omitting an event
merely because the baseline local roots happened to be empty. A skip must follow
the concrete type and structural recipe and remain valid when the same function
is instantiated with caller observations. The compile-time HIR variant sweep
must cover that decision as well as the main dispatcher. The table above closes
route membership; extracting the exact exhaustive recipe records and checking
all source-action forms against them belong to the source-event owner matrix.
The exact structural record rules and bypass placements above constrain that
implementation; no local-root-empty optimization may change them.

### Lowering and optimization phase closure

The present lowering boundary already runs `simplify_known_drop_flags` and
`fuse_builder_writes` for each function before constructing the whole MIR program,
then computes `annotate_par_map_work`. The driver memoizes that returned program
in `lower_memoized`. Consequently, adding a source proof after the existing public
lowering call is too late unless that call's internal phases change.

The driver also performs a semantic materialization after lowering:
`install_static_descriptor_data` replaces descriptor constructor bodies, appends
row/decoder/query-metadata functions, removes dormant constructor monomorphs and
recanonicalizes types. The source placeholder may abort; analyzing it as the final
body would suppress real descriptor returns and calls. Source access must represent
the complete typed descriptor producer rather than its aborting placeholder.

Materializing SQL before source proof is not an available phase ordering. The
summary-only frontend path forms descriptor interface identity without reading
SQL. The test-mode per-unit path completes package-wide catalog/process-edge
validation before any producer resolves static input, acquires publication locks
or forms artifacts. Preserve those existing pre-side-effect boundaries. The
public descriptor contract is already available from semantic checking, separately
from its source/artifact payload; it must suffice for source access analysis.

The required order is: structurally validated initial MIR and source events with
complete typed descriptor access semantics; source access admission and canonical
interface behavior; required package/test validation before static input I/O;
existing SQL resolution and native descriptor materialization when an executable
is requested; discharged-event erasure and execution optimizations/work annotation;
actual resulting native-MIR validation. A summary-only check ends without requiring
an executable descriptor artifact. Whole-program, per-unit, located, combined-test
and cached routes must preserve this same distinction. Borrowed-element reservation
markers are not source events and survive for their existing bounds proof.

A semantic MIR descriptor producer supplies that pure typed behavior. It replaces
a complete validated constructor expression
using the joint checked-HIR/descriptor relation, and later materializes through the
existing artifact owner. An opaque copied metadata row or a guessed function-name
prefix is insufficient. The typed producer and its complete consumer migration
matrix below define the formation, remapping and phase checks.

`StaticData` is a native-materialization-only Rvalue at this boundary. The current
HIR-to-MIR lowerer constructs none; `install_static_descriptor_data` constructs
its records after resolving the private artifact. Initial source-program
derivation must reject a StaticData Rvalue rather than serialize a partial or
redacted native blob. The public access wire reserves its baseline tag without
an accepted payload. The new semantic descriptor producer has its own typed
public record; it is not encoded as StaticData. Final native validation still
validates actual StaticData bytes, alignments, relocation targets and their
referenced generated bodies under the existing native owners. An internal
actual-body analysis may inspect those private records, but cannot publish them
as a source bundle or substitute them for the earlier source proof.

The same phase inventory applies to the four ColumnBatch Rvalues (Create,
Append, Row and Soa) and the Finish/Drop statements: their only current producers
are those generated descriptor bodies in the driver. They have no direct checked
source expression that reaches initial lowering. Reserve and reject their public
wire tags as native-only, while the semantic descriptor's Batch roles supply the
source access behavior. Their exact native schemas and relocation/child-storage
validation remain required for final actual-body validation; excluding them from
the source codec does not remove those owners.

Source proof requires the complete callable graph, so it cannot run independently
inside the current per-function `lower_fn` loop before the other functions and
canonical type tables exist. Moving the peepholes after initial program formation
must preserve their current ordering, plan-record normalization and source-line
alignment. Erasing source statements removes matching `stmt_lines` entries, not
neighboring runtime statements or their locations. `par_map_function_work_units`
currently counts all statements; source events must be gone or explicitly excluded
before computing runtime work hints. Merely adding inert LLVM arms would otherwise
change parallel scheduling, and leaving events as permanent fusion barriers would
silently disable the existing builder optimization.

CodegenProgram owns private source-program and native-program fields and exposes
immutable inspection. Source admission retains semantic descriptor behavior
before artifact I/O. The interface-owned finalizer binds that source to generated
native bodies, event erasure and execution passes. Memo hits clone the complete
owner; no separately mutable body can replace one half. The exact source,
finalizer, emission and native-inspection APIs below define every caller route.
A Boolean inside the current mutable Program is not this ownership boundary.

The source record and optimized body need an exact compiler-owned phase binding.
The source/finalization and emission records below own that binding: a cloned
source bundle or a Boolean claiming that initial MIR was checked cannot authorize a
modified local body. Native emission validates its actual physical MIR through the existing producer,
resource, callable, bounds and ABI owners. It does not rerun the new source
interpreter on erased native events or reconstruct typed DB semantics from raw
pointers. Source acceptance uses the retained initial source program; the closed
compiler-owned finalizer and immutable pairing bind that proof to native output. A body mutation after optimization must invalidate the phase
binding and fail closed or re-enter the complete source derivation; it cannot
reconstruct omitted source uses from optimized instruction positions. Imported
artifacts retain the separately stated compiler-produced trust boundary.

The caller audit includes `emit_object`, test/PGO object emission, whole-unit
prelink bitcode, LLVM-IR and optimization-remark entrypoints, plus
`validate_thin_partition_program` before cache/LLVM side effects. Function and
support partitions consume their exact `PartitionCodegenView`, not the original
Program; the new binding must qualify that selected body/shared-table view without
reintroducing ambient peer discovery. A whole-program wrapper alone does not
qualify an independently assembled public partition view.

Expected access failures must be ordinary source diagnostics. The current
`LoweringRejected`/`vanished_lowering_message` route describes malformed checked
HIR as an internal compiler error; it must not report a valid rejection of a
read-only write as a compiler defect. The admission-error and diagnostic-map
records below specify that distinction. In-process memo size
accounting must include retained access/binding data, not only the current HIR
render-length estimate.

The phase owner must demonstrate unchanged builder fusion and parallel work hints,
source-event/source-line erasure alignment, source-record identity across ordinary
optimization, rejection of a copied stale binding after body/target mutation, and
cache rehydration through the same checks. No arbitrary post-optimization API may
mint a source certificate from a source-event-free body. These are implementation acceptance cells under the exact binding and entrypoint
records below. No permissive legacy lowering route is retained.

### Source admission errors and diagnostic ownership

The fallible source boundary returns either SourceProgram or
`SourceAdmissionError`. Its variants are `MalformedHir(LoweringRejected)`,
`MalformedAccess(AccessRecordError)` and `Violations(Vec<AccessViolation>)`.
The first preserves the existing malformed-checked-input report; the second
identifies invalid compiler-produced or imported access records; the third is a
normal source rejection and must never use the vanished-program/internal-error
message. A nonempty violation list cannot publish a SourceProgram, interface,
unit-frontend cache entry or newly emitted native artifact.

Source-formation refusal uses the same `LoweringRejected` carrier with three
explicit added ValidationPass cases: `SourceFormationMissing`,
`SourceFormationMismatch` and `SourceFormationProjectionUnavailable`. Each uses
the supplied checked function/struct counts and `function: None`, since the seal
binds the complete input. After checked-HIR validation and only when the body
requires a descriptor/bridge seal, check presence first, obtain the complete
production projection next, and compare the bound projection last. Missing seals,
an unavailable projection and unequal projections therefore have distinct
deterministic reports. Projection unavailability is a compiler-side inability to
complete formation, not a ReadOnlyWrite or a user type error; it cannot produce
an empty program, skip source proof or fall back to a descriptor-name heuristic.

| Compiler-owned record | Exact fields and meaning |
| --- | --- |
| AccessSite | `bundle: BundleScope`, `program: u32`, `block: u32`, `position: AccessPosition`. AccessPosition is `Instruction(u32)` or `Terminator`; the terminator sorts after every instruction in its block. Source-event and native-action sites use the same instruction order. BundleScope is an invocation-owned selected bundle identity, not a pointer or caller-supplied source name. |
| AccessViolationKind | Closed alternatives `ReadOnlyWrite`, `UnqualifiedWrite`, `EndedUtf8`, `EndedCodec`, `UnqualifiedCallback`. Formal obligations suspended during declaration audit are not violations. A closed actual value must not remain Formal. |
| AccessViolation | `kind: AccessViolationKind`, `operation: AccessSite`, `calls: Vec<AccessSite>`. Calls run from the outermost concrete call toward the failed operation; an intra-function failure has an empty list. These are compiler-host allocations, with no generated runtime metadata/allocation. |
| AccessDiagnosticMap | An immutable optional source anchor for each retained site plus logical unit/function names. Anchors use this invocation's SourceMap and validated source byte ranges. The map is separate from canonical access bytes and cannot alter semantics or interface identity. |

`AccessRecordError` contains `stage: AccessRecordStage`,
`path: Vec<AccessRecordPath>` and `detail: String`. AccessRecordPath is
`Field(&'static str)` or `Element(u32)`: field names come from the fixed schema,
and element ordinals identify the first failed record in that sequence. A path
can identify an invalid reference without treating its target as a valid
AccessSite. Detail is an English diagnostic explanation, not a persisted tag,
cache key or source-language error value. The stage and path determine precedence;
message wording does not. All three fields are compiler-host data.
AccessRecordStage is the closed ordered set `Wire`, `Consumption`,
`CanonicalOrder`, `References`, `Closure`, `SourceEvents`. These stages describe
record admission; declaration audit and actual source violations retain their
separate SourceAdmissionError variant. Source-produced records use the same
canonical paths and semantic stages even when no wire byte offset exists.

Validation stages run in this order: checked-HIR validation, source-formation and
descriptor binding, complete initial-MIR structural validation, access record
structure/types/source-event liveness, selected import/ABI resolution, declaration
audit, then closed-root replay. A malformed earlier stage prevents later stages;
source violations are considered only after all required structural checks pass.
Within a stage, use the specified record/field and canonical program/block/site
order, never hash-map iteration. The detailed AccessRecordError wire precedence
remains the decoder record's owner rather than an incidental helper call order.

Interpretation retains every reached violation through the fixed point, including
writes before a nonreturning call. Deduplicate by the failing operation and kind,
not by abstract context id. If a destination has both ReadOnly and Unavailable
alternatives, ReadOnlyWrite is the root write diagnostic; otherwise an unproved
actual write uses UnqualifiedWrite. For one use with both ended validation kinds,
report EndedUtf8 before EndedCodec and retain one root error for that operation.
A failed operation has no invented successful continuation. Other independently
reached CFG edges still contribute their own violations.

Select the shortest retained concrete call witness for each root error, breaking
ties by the canonical sequence of call sites. The finite state/call graph owns
predecessors; recursively repeated context edges do not grow an unbounded source
call stack. After root selection, order diagnostics by resolved logical source
unit, byte offset, logical function and retained site order, with kind as the last
tie-breaker. Unknown/missing diagnostic anchors fall after known anchors and use
selected bundle/entry/site identity. Diagnostic rendering cannot read artifact or
source files to guess a missing anchor.

Use the failed operation as the primary span when its source anchor exists. Add
the selected concrete call sites as notes. An imported private site with no local
anchor uses the nearest available concrete caller anchor as primary and names the
selected dependency/function as a note. With no anchor at all, emit a spanless
ordinary source diagnostic. Keep established read-only/validated-byte wording
where applicable; state unavailable authority/target as an unproved write/call,
without suggesting an implicit copy, hidden cast or new language qualifier.

Diagnostic maps are produced during initial lowering before source-event erasure.
Every event-producing/remapping path must preserve the site's anchor, including
cache memo rebinding to the current SourceMap. Imported canonical bundles need
not expose private source paths to preserve the caller-located fallback. Acceptance
checks normal error classification, eager call witnesses, unavailable targets,
mixed invalid inputs, multiple contexts for one operation, recursive witness
bounds, missing imported anchors and deterministic whole/per-unit rendering.

### Source formation and finalization ownership

The typed descriptor relation must originate while sema still has its
`GenericNominalInstance` table. A bounded alternative to persisting a second
generic-type graph is a sealed source-formation record: retain the exact
span-erased production-HIR projection together with the exact ordered descriptor
projection after sema finishes its existing remapping. Its constructor is private
to semantic checking. Lowering compares both supplied inputs against that record
before substituting a complete descriptor constructor with its typed semantic MIR
producer. The descriptor's Params/Row relation is then grounded in the source
checker that actually validated it, not reconstructed from its printed name.

The existing native shape checks do not replace that relation.
`producer::db_resource_matches_row` compares a resource against the complete
direct or reconstructed row-name mangle, its declaring module and arity.
`query_descriptor_row_matches` parses the Params name length and checks that
some matching Params definition and the selected Row definition exist. These
are useful malformed-producer checks; they do not identify the Params used by
a particular bind call or prove that a copied raw plan belongs to that Query.
The reconstructed name maps both `.` and `$` to `_`; it must not become a new
canonical type identity or a source of permission. Preserve the existing native
checks and derive access-role identity from the checked type relation instead.

The required relation spans `query<P,R>`, `command<P>`, `stmt<P,R>`, `rows<R>`
and `batch<R>`. Sema's prepare dispatch already selects the Stmt instance from
the Query's exact argument vector; current-row and batch projections select R
from their resource instance. The source seal authenticates these decisions; DbBridgeWitness below retains the
exact remapped types for interpretation and imported ABI checking. Do not recover
the relation through printed names. No second complete generic-instance graph is
persisted.

The raw transport path is concrete and must appear in that closure, rather than
being hidden behind a successful header check:

| Retained producer carrier | Current construction and role transport |
| --- | --- |
| Query/command descriptor | The driver materializer constructs the 144-byte header with typed relocation targets; a Query retains the 72-byte batch plan at offset 128. Source analysis needs the semantic equivalent without private SQL/artifact bytes. |
| Statement | `new_stmt_state` allocates 112 bytes, version 4, then stores binder at 24, row validator at 56, stream decoder at 64, batch plan at 72, parameter resolver at 80 and parameter-count thunk at 96. Return and resource wrapping retain those raw pointer values. |
| Rows | `new_rows_state` allocates 120 bytes, version 4, then stores row validator at 40, stream decoder at 48 and batch plan at 88. These are ordinary raw stores in an Align body, not new generated callback definitions. |
| Batch | `new_batch_state` allocates 48 bytes, version 1, and stores plan at 8 and independent payload at 16. Row/SoA dispatch loads the retained plan; Drop loads plan/payload before marking the wrapper closed and clearing its payload. |

A source access profile cannot infer a target role from a non-null Raw value.
Use the compiler-owned typed DbBridgeCall operation below to preserve source
intent across the native transport. The source-formation seal binds the exact
operation, callee expression and typed witness. General raw-memory interpretation
would require an additional alias/offset/free/foreign-effect analysis and is not
selected. No package prefix or runtime address authenticates a source role.

Keep this decision within the existing unsafe boundary. Draft §15 assigns raw
range/pointer validity to the unsafe wrapper; the D13 internal thunk contract
requires the plan to be producer-owned, the append context to have passed its
row validator, and projection indices to be in range. Source access analysis
does not prove arbitrary C code or make those unsafe preconditions safe. The
typed bridge retains those preconditions and checks its specified source-visible
read/result/observation effects, like a closed native intrinsic. Its authenticated
operation and witness cannot be inferred from an enclosing module name or an
arbitrary unsafe call with the same physical ABI.

The selected typed operation has ten production formation sites in sema's
descriptor checkers. The other two explicit `ExprKind::RawCall` constructors in
the current file are test fixtures, not source admission routes. All ten already
know the requested operation before erasing it to a raw pointer load/call. The
operation contract can retain that intent through remapping and source lowering,
then lower to the existing RawCall only during trusted native finalization.
This preserves source intent without deriving permission from a raw address;
native representation and explicit unsafe preconditions remain intact.

The missing-type cases are bounded but must be represented honestly. Batch
create/finish/drop take only raw plan/payload operands and do not themselves
return a source view. Prepared parameter count likewise takes a raw statement,
so its source contract cannot claim the exact Params field count without an
additional typed witness. It may conservatively return the admitted scalar
domain; that is different from the descriptor count operation whose concrete P
provides the exact count. Batch append has `RowsRef<R>`; BatchRow/SoA have
`BatchRef<R>` and their exact selected output type. Bind has its exact P; direct
decode and QueryMeta retain the checked Query application. The exact role, scalar/control and finalization records below define their
source-visible effects. A copied type witness alone cannot authorize a changed
source operation beside the seal.

The lifecycle boundary limits what a typed role must observe. A partial batch
payload has no published source Row/SoA view: the package finishes it and creates
the batch resource only on successful publication. Failure releases that partial
payload without returning a source view. Public projections attach their backing
and nested content profiles to the selected batch resource generation. Ordinary
resource Drop ends that generation under the existing lifetime proof before
dispatching its raw hook; the analyzer need not recover an owner by reverse
engineering the raw payload's allocation address. This relies on the declared
no-partial-publication contract and its actual package/native owners. An operation
that could project an unpublished raw payload without a typed owner would require
a different contract and cannot be silently admitted by these roles.

The selected typed-call witnesses and their roles are:

| Role | Typed witness retained by semantic checking | Source effect that requires the witness |
| --- | --- | --- |
| Bind | exact P and its physical shared parameter | Read all relevant Params leaves during binding; no caller-storage retention after the operation. |
| ParameterCount | exact P when supplied by the descriptor operation; absent for the raw prepared-count bridge | Return P's exact field count only in the witnessed case. Without P use the conservative u32 scalar domain; never infer P from the statement pointer or its header count. |
| ParameterOrdinal | exact P from Query/command or Stmt | Check the input name read. Preserve only justified public scalar facts; native protocol order and unavailable per-Query options remain abstract. |
| StaticValidate | exact checked descriptor application | Scalar status and existing raw-context precondition; no source view result. |
| RowValidate | exact R from Query or Rows | Scalar status; validation precedes source-view publication through its separate role. |
| Decode | exact R from Query | Construct independent supported scalar fields; unsupported view-bearing direct decode has no normal return. |
| StreamDecode | exact R and `RowsRef<R>` | Publish owner-rooted text/byte views with the raw-view validation/permission contract, including absent optional fields. |
| QueryMeta | exact Query application and QueryMeta result type | Read-only static text leaves and public inspection structure; private evidence/content remain abstract. |
| BatchCreate | no source Row witness in the current raw-only bridge | Fresh unpublished payload or null; no source view can be returned or guessed from its address. |
| BatchAppend | exact R and `RowsRef<R>` | Copy validated current-row data into an unpublished batch; failure publishes no lane or source view. |
| BatchFinish | no source Row witness in the current raw-only bridge | Finish an unpublished payload; return unit. Fixed-column projections still require a published typed batch owner. |
| BatchRow | exact R and `BatchRef<R>` | Select the batch generation's field/element profiles with qualified byte storage, read-only text and optional presence. |
| BatchSoa | exact SoaPlain R and `BatchRef<R>` | Select finished batch columns and nested text profiles. The owner and complete Row shape must agree. |
| BatchDrop | no source Row witness in the current raw-only bridge | Release a partial payload or the payload of an already-dropping resource; return unit and publish no source view. |

These witnesses must use remapped concrete type ids in the source representation
and canonical TypeRefs in the wire representation. A missing witness is permitted
only in the explicit raw-only cases above, not as a fallback when a concrete
descriptor/resource relation fails validation. Their exact tagged record and
formation/finalization API records below bind those producer facts.

The source operation is a compiler-owned `DbBridgeCall`.
Its HIR node replaces the ten source `RawCall` producers and retains all existing
`guard`, `callee`, ordered `args`, `param_tys`, `param_modes`, `return_borrow`,
`return_region` and `return_cleanup` fields, plus the enclosing expression's
result type. Two added fields are mandatory: `operation: DbBridgeOp` and
`witness: DbBridgeWitness`. There is no optional unqualified source-call form.
The source-formation seal covers these fields and the complete expression;
hand-constructed native inspection remains a separate route. The initial MIR
node carries the same operation/witness plus the existing evaluated callee,
ordered operand list, parameter/result types and complete signature. Guard
lowering remains the preceding branch/abort CFG. Trusted native finalization
erases only operation/witness and emits the existing native `RawCall`; source
admission never derives that role from an arbitrary native callee.

`DbBridgeWitness` has the following exclusive concrete forms. Each `Ty` is
remapped with the owning HIR; the wire counterpart uses a canonical `TypeRef`.
The descriptor and resource types below name the actual selected nominal type,
not a name reconstructed from P/R. Field order in this table is record order.

| Witness tag | Fields | Formation relation |
| --- | --- | --- |
| 0 Query | `descriptor: Ty, params: Ty, row: Ty` | Exact checked `query<P,R>` application. |
| 1 Command | `descriptor: Ty, params: Ty` | Exact checked `command<P>` application. |
| 2 Statement | `resource: Ty, params: Ty, row: Ty` | Exact checked `stmt<P,R>` application selected by a resource reference. |
| 3 Rows | `resource: Ty, row: Ty` | Exact checked `rows<R>` application selected by a resource reference. |
| 4 Batch | `resource: Ty, row: Ty` | Exact checked `batch<R>` application selected by a resource reference. |
| 5 RawPreparedCount | no fields | Existing raw-statement count operation; no P witness or exact count is claimed. |
| 6 RawBatchPayload | no fields | Existing unpublished/already-dropping payload create/finish/drop operations. |

`DbBridgeOp` is one `u8` discriminator with no implicit default. The allowed
witness sets are exhaustive; all other combinations are malformed before
transfer or native finalization. The ordinal operation denotes the generated
resolver's signed result; the existing ordinary ordinal/binary normalization
calls remain subsequent source operations.

| Operation tag | Operation | Permitted witness tags |
| --- | --- | --- |
| 0 | Bind | Query, Command, Statement |
| 1 | ParameterCount | Query, Command, RawPreparedCount |
| 2 | ParameterOrdinal | Query, Command, Statement |
| 3 | StaticValidate | Query, Command |
| 4 | RowValidate | Query, Rows |
| 5 | Decode | Query |
| 6 | StreamDecode | Rows |
| 7 | QueryMeta | Query |
| 8 | BatchCreate | RawBatchPayload |
| 9 | BatchAppend | Rows |
| 10 | BatchFinish | RawBatchPayload |
| 11 | BatchRow | Batch |
| 12 | BatchSoa | Batch |
| 13 | BatchDrop | RawBatchPayload |

These fourteen operations select the correspondingly named generated roles;
BatchSoa remains its own conditional native role and physical ABI. Witness kind
distinguishes descriptor, statement, rows and raw-only invocation forms without
changing role identity. Check the exact operation signature, resource argument
position (or selected resource inside the callee expression), return-region
roots, Row admissibility, guard and raw-pointer load shape against the existing
producer owner. A structurally valid witness is not
proof of its nominal application by itself. Local records use the source seal;
imported records additionally undergo strict canonical decoding and selected
import ABI checks under the existing compiler-produced artifact trust boundary.
Do not claim that the current canonical resource type encoding independently
reconstructs generic arguments it does not store.

The source schema below assigns the two semantic extension tags and reserves
native RawCall. The full guarded-count and Rows stream-decoder vectors below
exercise distinct bridge shapes; the complete producer-validation matrix remains
required for every operation/witness combination. Neither an unresolved type
witness nor a native RawCall fallback is an admitted source record.

The operation/witness subrecord is encoded as `operation:u8`, `witness:u8`,
then the witness's listed `TypeRef:u32le` fields. No optional field tag or padding
is present. Query/Command descriptor and P/R references name structs; resource
witnesses name the underlying resource type, not its ResourceRef wrapper. The
bounded subrecord decoder accepts the 22 allowed operation/witness combinations
and rejects the remaining 76 combinations, all 224 truncated accepted-record
prefixes, trailing bytes, unknown discriminators, invalid TypeRefs and wrong type
kinds. Six independently specified vectors cover Query bind, Statement bind,
raw prepared count, Rows stream decode, BatchSoa and raw batch Drop. This is
subrecord coverage, not a complete guarded-call bundle golden or proof of the
nominal generic relation. Full source-wire and native ABI owners remain required.

The typed-call migration has these concrete consumers. An enum rename or added
field is not complete when only the lowerer compiles:

| Owner | Required preservation |
| --- | --- |
| sema source formation | All ten production descriptor-checker constructors retain their exact operation, argument modes, return roots, cleanup and guard. Raw-only count/create/finish/drop cannot acquire an invented type witness. |
| sema type remapping | The existing RawCall arm remaps only `param_tys`. Every added descriptor, Params, Row, resource and result witness must use the same final remap; no source id may survive into another type-id universe. |
| sema effect and evaluation traversal | Preserve guard/callee/argument order and termination, plus inferred impurity. A typed bridge never becomes a Pure parallel callback. |
| sema lifetime, borrow and storage analysis | Preserve exact return-region roots and parameter modes, mapped return-borrow roots, call completion/snapshot classification and source-use behavior. Reclassifying a view-returning bridge as Static would reopen escape/next-generation defects. |
| `replay_clone.rs` and `hir_depth.rs` | Retain role/type metadata during replay and production projection; clone/drop child expressions through the existing stack-bounded machinery. Projection bytes must include new semantic metadata before sealing. |
| MIR checked-HIR validation | Validate role/physical-signature/type-witness agreement and exact required guards; source admission additionally authenticates the complete seal. Existing malformed record owners must still exercise structural rejection independently of a missing seal. |
| MIR source lowering and finalization | Source lowering preserves the authenticated role and source events. The compiler-owned finalizer alone erases it to the existing actual native RawCall shape, which still undergoes actual-MIR validation. No public hand-built native body can obtain a source-qualified pair through that erasure. |
| interface projection and concrete generics | Serialize only the canonical role/type contract, recheck imported physical ABIs, and re-run source checking when instantiating generic template ASTs. Unreachable abstract HIR table entries cannot become concrete native witnesses. |

The existing sema raw-call storage/evaluation fixtures, checked-HIR raw-call
negative controls, MIR producer/native view tests and driver return-provenance
owners remain relevant. Add parameterized witness substitution, post-remap seal,
bridge-only unit and guarded early-termination cases to their owning targets;
do not replace those checks with a constructor-count assertion alone.

This is a complete input binding, not certification of a copied descriptor row.
Changing a function body, header, type definition, constructor selection or
semantic option must invalidate it. Copying the record beside a different HIR
program must fail. Source-position-only changes may compare equal through the
existing exhaustive production projection; diagnostic locations remain separate.
A missing record cannot authorize a static constructor or an authenticated typed
descriptor bridge. A program containing neither needs no such source seal. In
particular, a package unit may contain concrete bridge calls without defining a
static Query; the typed-call operation requires a seal in that case too.
The exact carrier, remapping and test-overlay records below define this input. Its retained bytes count toward memo
budgets. Do not invoke a source-insensitive re-certification fallback on a cache
miss or malformed record.

The existing `production_codegen_projection` is fallible even for a valid input:
its stack-bounded clone and fixed-stack serialization worker may return None,
including worker creation/serialization failure. A projection used only as a
memo optimization may decline that optimization; a required source seal cannot
decline its authority check. Source formation must report this failure and
publish no seal. Admission uses the distinct projection-unavailable report above.
Retain the existing exhaustive, span-erased projection and bounded teardown;
do not add recursive per-expression serialization on the normal owner stack.
When the same invocation already computed the exact final projection for its
lowering memo, pass those immutable bytes through the internal formation path
instead of independently cloning and serializing the same HIR again. This reuse
is internal and exact-input-bound; a public caller cannot supply arbitrary bytes
to authenticate a changed HIR. Retain the 2 MiB owner-stack projection test and
add refusal controls for absent, mismatched and unavailable required projections.

A suitable finalization owner already exists in the dependency graph:
`align_interface` depends on sema and MIR, owns validated static artifact formats,
and is already a dependency of LLVM codegen and the driver. Move the pure static
MIR and query-metadata generators into that owner. Its finalizer consumes
source-checked MIR, validates supplied descriptor artifacts, materializes native
bodies, erases discharged events, applies MIR execution passes and constructs
an immutable CodegenProgram. Artifact file resolution remains in
the driver after its existing package/test preconditions. The move must include
all fourteen generated roles and their shared typed inputs; no public callback
may supply an arbitrary replacement body and obtain a fresh source binding.

The source descriptor carrier has one semantic owner. The current separate
`Checked.static_descriptors`, `TestOverlay.static_descriptors` and driver copies
must not become independently mutable semantic authorities beside a certificate.
The HIR carrier stores the descriptor rows behind an opaque public type
with private fields and a private source-checking constructor. Serialization of
the production projection includes the actual descriptor rows but omits the
certificate bytes themselves, avoiding recursive self-encoding. Thus the same
exact projection binds body/type facts, inline SQL, implicit-file item identity
and semantic options; the lowering memo cannot return a source program carrying
another request merely because its aborting placeholder MIR was identical.

Each private source descriptor record pairs its StaticDescriptor request with
`constructor_function: String`, the exact final logical HIR function identity
selected by semantic checking. This binding is included in the complete source
projection and updated by the same trusted logical-name remapping as its body.
It is not another public interface field. Initial lowering locates that exact
validated constructor and records the resulting DbDescriptor site/target in
SourceProgram. Finalization uses this bound site, rather than accepting another
entry-unit string and reconstructing a symbol from unit/item names. The current
native installer distinguishes plain entry-unit names from `unit$item` through
an explicit entry-unit argument; the bound source target preserves that existing
selection without introducing a finalization-time naming guess. Missing,
duplicate or differently typed constructor targets fail source formation before
any SQL input resolution.

The projection/seal is finalized after the existing semantic remapping. Production
and combined-test HIR receive their own records after their respective source
checks; a test-only change must not contaminate the production input projection.
Any legitimate later transformation must either consume authenticated inputs in a
named compiler-owned transformation or invalidate the seal. There is no generic
public reseal operation taking arbitrary mutated HIR. Handcrafted HIR without
static constructors or authenticated descriptor bridges keeps its existing
structural/body validation path. An unsealed constructor or typed bridge is
rejected before MIR formation. Raw-native inspection owners may still exercise
their explicitly separate actual-MIR validator; they cannot manufacture a source
seal through that route.

The test-overlay formation site is concrete: sema first finishes production
analysis, rechecks an immutable source twin with synthetic test functions, then
changes the matched synthetic functions to `FnOrigin::Test`. Seal the combined
program after that last trusted origin rewrite, not inside the recursive check
and then carry a stale seal through it. Its descriptor carrier must describe the
combined program's own type-id universe. The existing overlay-only descriptor
list is a delta used to reject test-only database descriptor formation; it is not
a complete source binding for the combined HIR. Preserve that rejection while
keeping the complete combined inventory in its one semantic owner. Production
and overlay projections remain independent; no source-only test edit changes the
production seal or consumer interface behavior.

The production/test selector has a concrete immutable owner. Make sema's
`CheckedProgram` fields private, retaining immutable access to its production
HIR, descriptor owner and optional TestOverlay. The driver Checked carrier owns
that result instead of publishing a separately mutable HIR/test-overlay pair.
The compiler-produced TestOverlay likewise has private body/catalog/delta fields.
Ordinary structural-HIR inspection can still clone a borrowed production body;
doing so does not create another checked test owner or reseal its descriptors.

`CheckedProgram::test_source(&self) -> CheckedTestSource<'_>` returns an opaque
borrowed view with private fields for the exact HIR, complete checked-test slice
and overlay descriptor delta. For Some overlay it selects that combined body
after the trusted Test-origin rewrite. For None it selects the same owner's
production body with empty test/delta slices, explicitly in test mode. This
trusted selection allocates nothing and does not clone/recheck the no-test body.
There is no public constructor from arbitrary `&hir::Program` plus an empty
catalog. The caller cannot mutate the owning CheckedProgram while that view is
borrowed. The full descriptor inventory still comes from the selected body's
one sealed descriptor carrier; the delta is only the existing test-boundary
rejection input.

MIR source admission selects either a production `&hir::Program` or this opaque
CheckedTestSource. It validates a supplied test catalog against its selected HIR
before source formation, even though the semantic owner already correlates them.
That keeps the existing checked-HIR test validator as the structural prerequisite.
The no-overlay per-unit path therefore receives a real empty test-mode catalog
without converting an already admitted production CodegenProgram or inventing
test authority from a reserved name. Existing malformed-HIR/catalog owners may
continue calling the structural validator on deliberately altered copies; they
cannot manufacture the opaque source view. Production-only clients may consume
the production HIR out of their checked result, but consuming that owner also
ends access to its checked test selection.

The MIR-owned admission API has these exact input records:

| Record | Variants/fields |
| --- | --- |
| SourceInput<'a> | `Production(&'a hir::Program)` or `Tests(align_sema::CheckedTestSource<'a>)`. |
| SourceScope<'a> | `Whole` or `Unit { logical_unit: &'a str, is_entry: bool }`. The checked walk supplies scope explicitly, including empty units. |
| SourceLocations<'a> | `Unlocated { source_map: Option<&'a SourceMap> }` or `Located { source_map: &'a SourceMap, plan_catalog: Option<&'a LocatedPlanSourceCatalog> }`. |
| SelectedAccessDependency<'a> | `logical_unit: &'a str`, `interface_identity: [u8; 16]`, `bundle: &'a AccessBundle`. The identity bytes are the existing interface Hash128's little-endian lo followed by little-endian hi, not a new hash algorithm or wire record. |

`admit_source(input: SourceInput<'_>, scope: SourceScope<'_>,
dependencies: &[SelectedAccessDependency<'_>], locations: SourceLocations<'_>)
-> Result<SourceProgram, SourceAdmissionError>` performs the ordered stages above
and returns an owned immutable source program. No input borrow escapes the call.
The resulting owner retains initial MIR, access records, required descriptor
requests and exact constructor bindings, entry/scope records, diagnostic map and
the source projection required by its memo identity. Its body/access tables and
selected dependency data are compiler-host allocations charged to the existing
memo accounting. It exposes immutable getters, not mutable native bodies or a
constructor from a copied admission Boolean.

Unlocated source-map input supplies source diagnostic anchors without populating
native `stmt_lines` or generating located current-plan records. Located input
preserves the existing source-map/optional authenticated plan-catalog behavior.
No map means the specified spanless diagnostic fallback. Locations remain outside
access identity; changing diagnostic availability does not change permission or
closed-call admission. Scope validation checks the existing Source-origin role
and exact logical-unit rules; it does not infer a unit or test role from a
monomorph/lifted symbol prefix.

The interface/driver adapter supplies dependencies from the exact selected
decoded summaries. Admission groups selections by logical-unit byte order;
repeated selections must agree in both interface identity and complete canonical
bundle bytes before they are coalesced. It checks bundle structure and every selected
entry/ABI relation in its specified import-validation stage. It does not
recompute an entire InterfaceSummary hash from an access bundle that omits the
summary's other fields. Matching the selected full interface identity remains
the existing interface/cache adapter's responsibility. This is the previously
stated compiler-produced artifact boundary, not authentication of arbitrary
caller-forged summaries. Whole-program calls may supply an empty dependency
slice when all ordinary access bodies are local; a missing required import
never becomes an empty callee effect.

The interface-owned finalization entry is
`finalize_source(source: SourceProgram, artifacts: Vec<BuiltStaticArtifact>) ->
Result<CodegenProgram, String>`. It consumes both inputs. Validate the exact
required descriptor-id set and each artifact's public contract against the
source's bound requests before changing native bodies. Materialize only the
bound constructor sites and their compiler-generated roles, erase discharged
source events, apply the named execution passes and move the resulting body
and immutable source binding into CodegenProgram. The finalizer clones initial
MIR internally for these transformations and retains the complete immutable
SourceProgram beside the final native body; both count toward memo storage.
Finalization is pure with respect to filesystem, cache and LLVM state.
Static input resolution/artifact-building errors retain their existing driver
reports; generation/binding refusal returns its existing owned reason string.
Actual native validation remains mandatory in module/partition qualification
before LLVM or publication. Finalization alone is not a QualifiedModule.

Even descriptor-free input goes through this finalizer with an empty artifact
vector, so event erasure, execution passes and source/native pairing have one
owner. Extra artifacts are rejected, not ignored. There is no separate finalizer
input for arbitrary replacement MIR, entry-unit naming, mutable descriptor rows,
native callbacks or a source-certificate digest. Required descriptor source data
is already in SourceProgram; generated artifact data is already in the opaque
BuiltStaticArtifact values.

MIR owns the public descriptor role/type plan derived from the admitted Params/Row
and existing runtime schemas. The interface-owned generator consumes that plan
and validates the supplied artifact's contract and ordinal/layout relations before
generating native bodies. Artifact field shapes cannot silently override the source
plan. Pure summary checking needs neither `BuiltStaticArtifact` nor generated
SQL/metadata constants. This preserves the direction of semantic ownership while
allowing the artifact/finalization owner to depend on MIR without a dependency
cycle through LLVM or the driver.

CodegenProgram owns its source access bundle and native `Program`
privately and exposes immutable inspection. Cloning/memoization preserve the
complete pair. The codegen owner must distinguish source-certified input from
explicit raw-MIR inspection input: the latter can validate the supplied native
MIR but cannot mint or claim an Align source certificate. Normal driver build,
cache publication and source-based test execution accept only the source-certified
route. Explicit native mutation owners use the separate inspection route below.
The emission API and negative-owner migration below close that input distinction;
retaining the old permissive source path under a new name is insufficient.

A partition needs its own explicit projection of this binding. The selected body,
supplied peer declarations/physical functions, shared tables and required source
access records must match the sealed producer data in that exact view. An
independently assembled public `PartitionCodegenView` cannot inherit authority
solely because its selected function originally came from a checked program.
This qualification must not rediscover missing LLVM peers from an ambient whole
program. The borrowed-partition record below fixes its exact type, comparison order and
cache identity.

### Entry selection and module qualification

SourceProgram retains one immutable entry catalog produced alongside initial MIR,
before HIR origins are erased. Its compilation scope is the explicit existing
whole-program or per-unit lowering input. A per-unit scope retains the logical
unit and entry-unit Boolean supplied by the checked walk, and validates that role
against every retained Source origin; monomorph/lifted origins are not guessed
from their symbol prefixes. An empty unit retains its explicit scope even though
no function can supply a role witness. Scope is compiler input, never environment
configuration or inferred physical peer linkage.

The catalog has these disjoint uses:

| Catalog record | Derivation and admitted use |
| --- | --- |
| Production main | The existing source entry main, correlated with its HIR Source origin, exact admitted parameter/result ABI and initial program target. Record its closed proof using either no arguments or the existing argv producer contract. The ordinary native main/wrapper must select this exact target and proof. |
| Conditional functions | Every admitted ordinary local access body with its exact physical ABI and completed declaration audit. Explicit exports select existing logical function identities from this catalog; no actual argument profile is invented by exporting them. |
| Implicit per-unit exports | The exact existing per-unit exportable set derived from Source origin and lowering scope. These remain conditional callable entries, with their interface access programs. Whole-program scope does not acquire this linkage merely by containing a public dependency body. |
| Test roots | The combined-test catalog correlated by the existing checked-HIR test validator, retaining logical target and exact zero-argument Result ABI. Closed replay uses those roots; source main does not become the C entry in test mode. |

Native callbacks and task/parallel consumers retain their exact producer transfer
and seed contract at each reached registration/application. A callback target
whose binding is formal suspends declaration audit; its closed actual use must
be instantiated. Merely appearing in the entry catalog never substitutes for
that producer-specific application.

Use an LLVM-owned opaque `QualifiedModule<'a>` containing a borrow of the complete
CodegenProgram and an owned immutable validated emission selection. Its constructor is
`qualify_module(owner: &'a CodegenProgram, request: ModuleRequest<'_>) ->
Result<QualifiedModule<'a>, CodegenError>`. ModuleRequest has exactly
`Ordinary { exports: &[String] }` and
`Tests { roots: &[TestRoot] }`, using its own request lifetime. The qualifier
copies the small selection into its private record; only the program borrow
determines the result lifetime. These are compiler-side selection allocations,
not body copies or generated runtime allocation. The qualified object exposes only immutable
inspection; it cannot be created from native MIR plus a copied catalog.

Ordinary qualification requires a production owner, checks every requested
logical export against its actual native function and source target/ABI mapping,
then binds any emitted native main to the catalog's closed-main proof. Keep the
existing special treatment of explicit main and duplicate export names; no new
Source-only name filter silently removes currently accepted concrete functions.
The CLI still checks entry-unit-only export selection against the checked walk
before constructing the per-unit requests. Export selection happens after source
admission, so changing only --export does not require re-lowering a memoized
source program or treating the export as a new concrete Align caller.

Test qualification requires a combined-test owner and correlates every supplied
root with its exact catalog target and fixed test ABI. Reject production/test
mode substitution, missing/extra roots, duplicate logical or dispatch identities
and disagreeing dispatch symbols before module/cache side effects. The aggregate
harness qualifier separately validates the globally ordered dispatch mapping
across those exact unit owners. Individual units cannot claim an unrelated
unit's root merely by presenting the same reserved native symbol.

A test build also emits dependency units with no local tests. The current walk
clones production MIR for `test_overlay = None`; the new walk must instead form
an explicit empty test view through the same checked test-catalog boundary with
an empty root list. Its initial bodies may be shared immutably with production,
but its test-mode catalog is distinct and suppresses native main ownership.
Only the checked walk's no-overlay branch forms this view; it is not a public
cast from an arbitrary production CodegenProgram to a test certificate. Aggregate
catalog completeness still includes every checked unit, including empty-root
units. Cover this path with a tested caller and a dependency having no tests.

The driver forms one opaque `TestEmissionPlan<'a>` from an immutable successful
`TestPerUnitWalk`, before CLI executable/object staging. The walk's unit inventory
and success/error state, and each artifact's test body/catalog, become private
with immutable getters; the checked walk remains their constructor. The entry is
`prepare_test_emission<'a>(walk: &'a TestPerUnitWalk) ->
Result<TestEmissionPlan<'a>, String>`. Preserve the existing diagnostics
and boundary-error reports, and refuse formation when either prevents a build.
Walk every unit in the existing dependency-first order, including empty-test
units. Derive global `align_test$<8hex>` ordinals and canonical catalog ids from
those exact source catalogs, then qualify each combined-test module with its
assigned roots. Neither a separate mutable `tests` vector nor caller-supplied
all_roots/catalog_ids arrays can override the source catalog.

LLVM aggregate qualification has the exact signature
`qualify_test_harness<'a>(modules: Vec<QualifiedModule<'a>>) ->
Result<QualifiedTestHarness<'a>, CodegenError>`. It consumes the ordered module
proofs, validates Tests mode, distinct unit identities, exact per-unit source
catalog coverage, global canonical-id uniqueness, dispatch order and the existing
1..=65,535 total-root bound. The result privately owns those module proofs and
the derived global dispatch/catalog records; only their underlying programs are
borrowed. This avoids a self-reference between an owned root vector and modules
that borrow that vector. Empty-root units remain present even though they add
no dispatch row. Immutable getters supply module inputs and the existing harness
identity fields; there is no independently replaceable root-symbol slice.

TestEmissionPlan owns that qualified aggregate and borrows the checked walk's
matching dependency-interface identities. Its private driver constructor passes
the complete walk inventory, so omission of an entire unit cannot be hidden by
also deleting its roots from an external catalog. The LLVM constructor validates
the complete supplied selection; it does not discover missing package units from
the filesystem or claim to reproduce the driver's checked walk. The normal
emission API accepts only the driver plan, never an independently assembled
aggregate as a replacement for that plan.

`emit_test_harness_object` takes `&QualifiedTestHarness<'_>`, out, target and
profile, returning `Result<(), CodegenError>`. The driver `emit_test_objects`
replaces its first three inputs with `&TestEmissionPlan<'_>` and
`&[PathBuf]` for the ordered unit output paths; its remaining harness path,
target, profile, rt_lto, jobs and cache inputs retain their current types/order.
Require one output path per admitted unit before lookup/scheduling. Unit and
harness cache keys come from the plan's immutable module/catalog records, with
the existing harness key still independent of unrelated unit bodies. Every
supplied module has already passed qualification before any unit or harness
cache hit is accepted. The CLI runner's reporting catalog is an immutable
projection of the same ordered canonical ids; no second ordinal assignment is
performed after emission.

A standalone Rust metadata-only check compiles the proposed lifetimes: temporary
per-unit root vectors may be dropped after qualification, the aggregate owns its
module proofs while borrowing programs from the walk, a partition reborrows its
module for a shorter invocation, and an owned worker task creates its module
borrow locally. This checks ownership feasibility only, not source/catalog
validation or permission to use Tests mode for an Ordinary-only operation.

Object, PGO, whole-unit prelink, LLVM-IR and remark entrypoints accept the qualified
module instead of a raw Program plus independently supplied exports/roots. Target,
profile, instrumentation, debug and runtime-bitcode inputs remain explicit as in
the existing APIs. Remarks use Ordinary with an empty export list and must obtain
qualification before the process-global flags. Test harness emission takes only
its separately qualified aggregate catalog. No normal driver emitter keeps a
parallel permissive raw-Program overload.

The emission signature migration is exact. In the following table `module` is
`&QualifiedModule<'_>`, `partition` is `&QualifiedPartition<'_>`, paths are
`&Path`, target is `&BuildTarget`, profile is `Profile`, runtime bitcode is
`Option<&[u8]>` and stable identity is `&str`. Keep the existing public function
names; replace the raw input and remove independently supplied exports/roots.

| Function | Ordered arguments after migration | Result and mode |
| --- | --- | --- |
| emit_object | module, out, target, profile, rt_lto | `Result<(), CodegenError>`; Ordinary only. |
| emit_object_pgo | module, out, target, profile, rt_lto, action: `pgo::PgoAction<'_>` | `Result<pgo::PgoRunReport, CodegenError>`; Ordinary only. |
| emit_prelink_bc | module, out_bc, target, profile, rt_lto, stable_id | `Result<(), CodegenError>`; Ordinary only. |
| emit_test_object | module, out, target, profile, rt_lto | `Result<(), CodegenError>`; Tests only. |
| emit_llvm_ir | module, target, optimized: `bool`, rt_lto | `Result<String, CodegenError>`; Ordinary only, preserving the existing diagnostic lens. |
| collect_opt_remarks | module, target, debug: `&DebugInfo` | `Result<Vec<String>, CodegenError>`; Ordinary with an empty explicit-export list only. |
| validate_thin_partition_program | module | `Result<(), CodegenError>`; Ordinary only. |
| emit_function_prelink_bc | partition, out_bc, target, profile, rt_lto, stable_id | `Result<(), CodegenError>`; Function view under Ordinary. |
| emit_support_prelink_bc | partition, out_bc, target, profile, stable_id | `Result<(), CodegenError>`; Support view under Ordinary. |

Check mode/view selection before native target/module creation, output staging or
remark-global initialization. A remarks request cannot silently ignore exports
already bound into its module. Function/support emitters preserve their current
wrong-view rejection instead of interpreting one view as the other. These are
borrowed invocation inputs; no emitter retains the module, path, bitcode, debug
or PGO borrow beyond its existing synchronous call. LLVM text and remark results
own their allocations; file-producing operations retain their existing output
and error ownership. Qualification introduces no extra implicit target, profile,
runtime-bitcode selection or ambient configuration. The independently qualified
aggregate harness remains a separate input because it generates dispatch code
without an ordinary MIR module.

The partition qualifier borrows this QualifiedModule, not just CodegenProgram:
`qualify_partition(module: &'a QualifiedModule<'a>,
view: PartitionCodegenView<'a>) -> Result<QualifiedPartition<'a>, CodegenError>`.
Its private record retains the module borrow and exact supplied view. The
selected source target, explicit/implicit root linkage and physical symbol/ABI
must agree with that module's bound selection and immutable compilation scope.
Missing peers are rejected on the supplied view, never filled from the owner.
Ordinary module qualification may be shared among its function/support
partitions; it does not force their cache keys to include unrelated bodies.

Explicit raw-MIR inspection remains a distinct validator/test route. It may
report structural/native defects in a supplied Program, but cannot return any
of SourceProgram, CodegenProgram, QualifiedModule, QualifiedPartition or cached
unit admission. Native emission owners built from source use the qualified API;
malformed native tests can mutate an inspected copy and exercise the raw
validator without authorizing it for normal driver publication.

Existing native shape owners also inspect successful LLVM from handcrafted MIR.
Keep one explicit LLVM-owned `native_inspection` module for these native-only
owners, including cross-crate integration owners. Its API returns bytes, not a
source-qualified carrier or a file publication operation. Its exact input and
output are:

```rust
pub enum Request<'a> {
    LlvmIr { optimized: bool, rt_lto: Option<&'a [u8]> },
    Object { profile: Profile, rt_lto: Option<&'a [u8]> },
}
pub enum Output {
    LlvmIr(String),
    Object(Vec<u8>),
}
pub fn inspect(
    program: &Program,
    exports: &[String],
    target: &BuildTarget,
    request: Request<'_>,
) -> Result<Output, CodegenError>;
```

The request borrows its complete input only for the call; output owns its text
or bytes. Run the same target-independent native validators and exact
export/callable validation before target/module construction, then the existing
selected pipeline and native verification. Object inspection writes to an LLVM
memory buffer and copies its bytes into the returned Vec; no temporary output
path, cache operation, source certificate or remark-global initialization is
part of this API. Both output variants are native inspection artifacts, not a
SourceProgram, CodegenProgram or qualified emission input. This introduces only
compiler-side allocation, with no compiled Align allocation change. The driver
production path and its cache writers never call this module. A native owner
may explicitly write the returned object bytes into its disposable test
directory and link/run its bounded probe. This does not certify the deliberately
mutated MIR as safe Align source. There is no normal-driver raw Program overload.

The caller audit includes these distinct cross-crate native controls:
`align_driver::tests::current_plan_side_table_is_absent_from_llvm_and_object_identity`
currently clears `Program::plan_records` on a cloned native body and invokes the
public raw emitters. Keep its MIR-text and implementation-hash assertions;
compare both LLVM stages and in-memory object bytes through native inspection.
`large_drop_codegen` constructs a 4,096-level native Drop graph directly and
executes its one-helper-frame owner. The cleanup owners in `m11_crypto_stream`,
`move_record_slices`, `m11_process_live`, `m11_process_verified`,
`fs_retained_tree` and `m11_os_host` remove actual Drop instructions to prove
their native allocation/resource detectors discriminate missing cleanup. Their
mutated twins must continue through native inspection and their disposable
bounded execution harness; source qualification rejection cannot replace those
negative controls. Their unmodified source twins use qualified emission, and
both modes retain their whole/per-unit assertions. These owners rule out making
the inspection API available only inside the LLVM crate's `cfg(test)` module.
A source-built `rt_lto` integration owner that supplies malformed runtime bitcode
instead uses a qualified source owner and preserves its current bitcode fallback
assertion. It does not need a raw inspection exception. Existing forged function
partition tests are LLVM unit tests: exercise the extracted exact-view native
validator directly, with separate qualifier negatives for owner/peer substitution.
They must still check the original malformed-peer diagnostic and pre-LLVM
failure, rather than becoming vacuous failures for lack of a source certificate.
Handcrafted native success/negative fixtures use native inspection;
source-based emission owners use the qualified route. The final caller sweep
must also follow driver wrapper calls, not only fully qualified LLVM calls;
otherwise these integration mutation owners are missed.

The source-admission/finalizer inputs above bind descriptor and selected
dependency carriers. This entry catalog and module/partition API fix
the entry-selection axis independently: no caller-supplied export list, test
symbol or native body can change the proof class of a formed owner. Acceptance
crosses production/test substitution, main/argv seeds, no-main exports,
entry/non-entry scopes, explicit main/duplicate exports, selected root/ABI
mutation, cloned owners and partition source/physical-peer disagreement.

### Native entrypoint and partition binding inventory

The current partition input is a public `PartitionCodegenView` enum. Its Function
arm contains selected Function, definition, canonical peers, physical
peer_functions and a sealed shared-table view. The shared table set is exactly
structs, enums, resources, tagged_types, fn_types, tuples, externs, imported_fns
and sqlite_callback_effects. Its Support arm contains the explicit support thunk
records. A new source binding must cover those actual fields rather than only
the existing Debug fingerprint: that fingerprint intentionally omits physical
peer functions. The source proof cannot rely on that omission as authentication
of a substituted physical ABI source.

| Existing entrypoint | Required input and pre-side-effect owner |
| --- | --- |
| emit_object / emit_object_pgo / emit_prelink_bc | The immutable final CodegenProgram plus the explicit existing target, profile, export and instrumentation inputs. Validate the selected source entry class and actual native body before creating LLVM modules or output files. |
| emit_test_object | Each unit's combined-test CodegenProgram and exact selected test-root dispatch mapping from trusted test formation. Validate every selected root, symbol and ABI before test module/object construction; an unrelated production binding cannot qualify a substituted test body. |
| emit_test_harness_object | The exact validated aggregate test catalog/root-symbol mapping. This emitter generates native harness code directly and has no MIR Program input. Qualify the aggregate against all selected unit objects before any unit or harness cache lookup; preserve its current ordered reserved symbols and root-count checks. |
| emit_llvm_ir | The same final source/native pair and explicit export selection as object emission. Diagnostic output cannot bypass source admission because it produces text instead of an executable. |
| collect_opt_remarks | Validate the final pair before ensure_remark_cl_opts, target/module creation or handler registration. The current first call changes process-global LLVM flags through Once, so rejecting after build_module would be too late for the new pre-side-effect source requirement. |
| validate_thin_partition_program | Consume the final pair before any partition cache lookup/staging. Retain all existing target-independent type/resource/callable checks. This whole-program preflight does not replace validation of each exact borrowed partition. |
| emit_function_prelink_bc | Consume a qualified Function partition containing the exact selected body, definition, canonical peer records, matching physical peer ABI sources and all nine shared tables. Check it before partition cache publication and LLVM construction. |
| emit_support_prelink_bc | Consume a qualified Support partition bound to the exact thunk records and their resource/hook source access entries. A changed hook, ABI, ownership or thunk set invalidates it even though this arm contains no selected ordinary Function. |

The driver owns the aggregate test preflight before `emit_test_objects` computes
keys or accepts cached objects. Its input-unit/root sets must cover the aggregate
catalog exactly, with no duplicate or missing dispatch symbol and matching fixed
test-result ABI. Each selected unit has its own combined source formation;
requiring one synthetic whole-program seal would break the existing per-unit
boundary. The harness key remains based on its catalog and dispatch behavior,
not every unit body. Changing a unit's access behavior still revalidates that
unit before a harness cache hit is accepted; it need not rebuild an unchanged
harness object. No new runtime dispatch protocol is introduced.

The source-admission/finalizer inputs, emission signatures and borrowed partition
binding above keep explicit peer discovery in the driver and exact
partition emission in LLVM; neither an ambient-program lookup nor a public
constructor accepting a copied certificate and arbitrary view closes this table.
Owners must alter each named field independently, including physical peer ABI
sources omitted from Debug, and reject before the relevant cache/LLVM side
effect. The support and test paths need their own negative controls; they cannot
be inferred from ordinary function partition coverage.

### Artifact and final native pair ownership

The current public BuiltStaticArtifact has five independently mutable fields:
descriptor_id, artifact, bytes, digest and runtime. That shape cannot become the
new finalizer's authority without correlating all five. A copied digest beside a
rewritten generated runtime plan is not an authenticated descriptor producer.
The generation owner already accepts only StaticArtifact and its digest in
`generate_static_runtime`; it needs no filesystem input to rebuild the typed
runtime plan.

Move the built-artifact carrier and pure runtime generation into align_interface.
Keep the validated artifact, canonical bytes, recomputed digest and generated
runtime behind one opaque carrier with immutable getters. Its constructor accepts
canonical artifact bytes, decodes them through the existing strict artifact
codec and derives the other fields from that decoded value. The descriptor id is
read from the validated artifact rather than independently trusted. Driver-owned
SQL/input resolution and artifact construction still happen after their existing
preconditions; handing off the encoded result does not move filesystem access
into source admission. Existing source requests and canonical public contracts
must match this carrier before native materialization.

The finalizer accepts an immutable SourceProgram and these qualified artifacts,
not a separately supplied native Program or arbitrary body-replacement callback.
It clones initial MIR internally, materializes the qualified roles, erases events
and applies the named execution passes. CodegenProgram privately owns the source
and final native programs together. Its public getters borrow immutable data;
copying native MIR out for an explicit raw inspection test cannot replace the
body inside that pair or create another source-qualified object.

Binding source to compiler-owned finalization and validating native emission are
different obligations. align_interface cannot invoke the LLVM validator through
a dependency cycle. MIR owns source/access derivation and backend-independent
native access checks; LLVM retains its actual target/type/ABI validation before
emission. For a materialized CodegenProgram, driver cache preflight must invoke that
actual-native validation before accepting an object hit, not merely rely on the
existence of CodegenProgram. The separately specified persistent frontend Reused
path accepts a previously admitted keyed artifact without materializing MIR;
new emission or rehydration always enters the full actual-native validation.
The source/event-free native body cannot re-create a missing source proof.

The existing backend-independent owners are concrete MIR functions:
`producer::validate_tagged_program`, `validate_resource_program`,
`validate_resource_rvalues`, `validate_slice_index_rvalues` and
`validate_fixed_element_nulling`, together with the existing producer readiness
and provenance checks. Preserve their exact physical-type, resource, view-retype,
cleanup and callable prerequisites on the final native body. Do not replace them
with access-program validation or a scan of its exported summaries. Native phase
validation additionally rejects residual source events and semantic DbDescriptor
or DbBridgeCall operations: the finalizer must have consumed those operations
before a CodegenProgram is published. Native RawCall and generated column-batch
operations retain their existing actual-body validation.

LLVM qualification runs its existing runtime-ABI registry, callable declaration
and selected-scope preflight checks before cache acceptance or LLVM creation.
Function partitions retain `validate_partition_tagged_program` on their exact
view; ordinary and test modules use the complete tagged-program validator.
Target layout, generated instruction verification and runtime-bitcode validation
still run at emission with the actual explicit target and payload. These are
separate owners: a backend-independent native check does not claim to validate
an LLVM target or runtime bitcode that was not supplied to it. The finalizer's
closed transformations and immutable source/native pairing retain the source
access proof; no second source-event reconstruction or new qualifier bypass is
introduced in these validators.

Negative owners independently alter each old carrier component and every final
body/shared-table input. API privacy prevents safe callers from mutating a formed
carrier/pair, while malformed canonical bytes and disagreeing source requests
produce ordinary errors before materialization. The entry-selection and
admission-error records specify the separate qualification obligation; an opaque
pair alone does not distinguish conditional exports from closed roots.

### Borrowed partition qualification record

Use an opaque LLVM-owned `QualifiedPartition<'a>` with private fields
`module: &'a QualifiedModule<'a>` and `view: PartitionCodegenView<'a>`;
the module owns the immutable borrow of CodegenProgram and bound entry selection. A fallible
qualifier consumes the explicit supplied view and borrows its immutable final
owner. Its constructor is the only way source emission obtains this type;
immutable view inspection is permitted, mutable inspection is not. The driver
continues to enumerate targets and assemble the raw view. Qualification checks
those supplied records; it does not add peers that the driver omitted.

For a Function view, check in this order before creating the qualified record:

1. The owner's source/native phase pair and selected entry class are valid.
2. The selected Function is an actual borrow of a function in that owner's native
   function storage, not a clone paired with the owner's certificate. Every
   physical peer is likewise an actual borrow from that same immutable storage.
3. Each shared slice borrows the corresponding complete owner table (same start
   and length), and callback_effects borrows its exact map. Empty tables compare
   by their empty content; distinct empty slice addresses confer no different
   authority. A table fingerprint cannot replace this origin check.
4. The definition and ordered peer declarations pass the existing symbol,
   duplicate, callable-target and exact canonical ABI checks, including the
   correspondence with their supplied physical functions. Required target
   completeness is validated on the supplied selected-body view. Source records
   are selected through the owner's logical-to-access mapping, never from an
   independently supplied bundle.
5. Run the existing target-independent validation on the exact selected module
   view before any partition cache lookup or artifact staging. Native emission
   repeats validation on that immutable exact view before LLVM creation.

This address identity is an in-process borrowing check only. No address enters a
cache key or persisted record, and no unbounded global certificate registry is
introduced. Cloning CodegenProgram creates another complete owned pair; callers
must construct a fresh borrowed view from that clone. A partition cannot mix a
selected function from one clone with shared tables from another merely because
their hashes are equal. Reborrowing a qualified view does not allocate or copy
program bodies; the qualifier may allocate its existing temporary validation
module, with no new runtime allocation in compiled Align programs.

For a Support view, retain the same owner borrow and validate the explicit thunk
set against that owner's resource inventory, declaration-unit ownership and
local/imported hook ABI records. Compare thunk symbol, representation version,
Drop ABI fingerprint and the complete owner alternative, including an Owned
hook's logical name, symbol, linkage and ABI. Preserve canonical ordering,
deduplication and the requirement for at least one Owned thunk. Reject an
omitted/extra/disagreeing record; do not silently reconstruct the emitter input.
Associate each thunk with its source Drop entry or selected dependency Drop entry.
The emitter continues to use only the supplied support records.

Qualification data is not a new full-unit dependency in a function partition's
implementation hash. It must be checked before accepting that partition's cache
hit on every invocation. Keep the existing body/ABI/shared-table structural key and selected dependency
interface identities. No source certificate, source-event or native-proof-id
field is added to that key by this design. A source-rejected unit cannot reuse
an older object because its erased native body happens to have the same hash.
Conversely, changing an unrelated local function must not rekey every partition
through an opaque owner pointer or a whole-unit source certificate digest.

The Entry selection and module qualification record binds the existing explicit
unit/export selection before partition qualification. The source-admission API
must populate that catalog from the same initial program and checked scope;
physical peer symbol spelling cannot populate or replace it.

### Location creation and recency

Canonical graph ordinals are context-local analysis identities. They are not
runtime addresses, allocation sites or an unbounded generation counter. Exact
local slots have fixed roots; canonical inputs have fixed protected anchors.
Descriptors and typed edges identify all other reachable storage, capture and
observation nodes.

A construction introduces a temporarily distinct node, assigns the completed
result its fixed instruction/value root, and then normalizes the graph. The
maximum temporary excess is the finite node count introduced by one instruction's
typed result/native schema. New objects do not reuse a reached node merely because
execution revisits the same instruction. Return translation likewise introduces
fresh result objects distinctly from input nodes before normalizing the caller.

The result root distinguishes the newest validation observation from retained
older observations. If an older value still has a local/input/retained-content
root, its ended status survives on that graph node or a conservative merged node.
If normalization merges observations, it unions status alternatives and sets
`Many`; it never resets an old observation to live. This is the MIR analogue of
plan 57's Current/Prior requirement, with rooted graph identity rather than a
second runtime generation scheme. No explicit numeric Current/Prior field is
serialized or added to source.

The quotient bounds normalized nodes from the fixed root and edge alphabets.
Temporary fresh identities disappear from canonical state/context keys. Repeated
allocation or validation therefore cannot create an unbounded identity chain.
Merging two represented objects retains `Many`, and an already-many object cannot
regain singleton authority after later aliases disappear. Independent new storage
can be singleton when it is represented distinctly from all retained older nodes.

Exact addressed local/fixed-array places remain point-specific. Dynamic contents
and summary locations use weak updates. A returned input-place change replaces
its selected profile only when entry/evaluation/return translation all preserve
singleton identity and an exact path; otherwise it joins the old alternatives.
The alias, recursion, revalidation and control owners must close that condition.

### Tabulation and finite control

A call context contains the selected concrete target and canonical incoming
graph, parameter profiles and availability mode. It excludes the caller's
location names and continuation. Calls suspend their continuation until a normal
return is published; a nonreturning call does not erase preceding write
observations. Every new return reactivates the saved callers. Recursive calls
use the same context/result worklist rather than recursively evaluating bodies.

Control facts include active sum tags, Boolean alternatives and the finite
integer constants required by the access program, with an additional unknown
alternative. Integer operations with unknown operands conservatively include
all possible represented outcomes; `Other + 1` must not remain `Other` when it
can equal a tracked constant. Native success/presence alternatives retain their
relationship to output initialization. No runtime integer value, trace or growing
symbolic expression becomes part of a context key.

For each concrete integer type, fix the tracked set before interpretation: zero,
one, constants in the retained access closure and exact public schema constants
required by semantic producers (including Params field counts). Exclude native
artifact-only constants and unreachable private bodies from this source-analysis
set. The partition consists of each tracked constant and `Other`, the admitted
type values outside that set. A state carries a subset of these alternatives;
`Other` is not a symbolic identity shared by all untracked runtime integers.
Use the same fixed partition throughout that analysis's calls and continuations.
Do not grow it when arithmetic produces another runtime value. Bundle bytes
carry the original typed constants and public schema, not the derived partition
or process-local control ordinals.

For singleton concrete operands, compute the existing typed operation, including
integer wrapping, then map the result back into that fixed partition. A guarded
invalid integer division has no normal arithmetic result. For any other operand
combination, the result must contain the abstraction of every concrete admitted
outcome; returning every partition alternative is a sound fallback. Comparisons
likewise retain both Boolean outcomes unless the represented operand sets prove
one impossible. In particular two `Other` operands cannot prove equality merely
because their abstract tokens match. Booleans and active sum tags retain their
own finite alternatives, and floating comparisons preserve IEEE NaN behavior
rather than inventing a trap or treating unknown operands as equal.

Lengths and loop counters are point-state facts. A materializer's zero-iteration
state retains its zero output count and empty contents together; a state after
publishing an element retains that produced profile before incrementing the
output count. Do not independently join the count from one state with the empty
heap from another. A final source descriptor takes the actual completed counter
operand. Filters, fallible callbacks, early exits and integer wrap alternatives
must preserve this order even when scalar precision is lost. These requirements
explain the existing collect trace; the owner still has to cover every lowering
that publishes a dynamically initialized collection.

### Complete finite-domain bound

Fix the finite concrete access closure before analysis: functions, CFG sites,
layouts, field paths, target/capture layouts and scalar partitions cannot grow
with runtime execution. Inline layout is finite; pointed-to storage and callable
capture objects are graph edges, never recursively inlined value definitions.
The following are semantic counts, not tables that must be eagerly allocated.

Let L be the number of content layouts, E the finite typed edge alphabet, and
d the maximum number of inline scalar/view/callable leaf positions in one
layout (at least one). For a function f with p physical parameters, its entry
root alphabet has at most Re = p*d + 1 elements, including UnknownObservers.
Normalize that input before adding anchors. The quotient gives

```text
He = L * (2^(Re * (1 + |E|)) - 1)
Rp = (p + slots + values + source_snapshots + He) * d + 1
Hp = L * (2^(Rp * (1 + |E|)) - 1)
```

At most He protected input anchors are then added. Caller-local roots, caller
anchors and call-stack depth never become callee entry roots. The finite slot,
value and snapshot counts come from f's access record. Hp therefore bounds
body/return heap nodes without a circular dependence on the caller heap size.
One instruction or call image may temporarily introduce only its finite typed
result layout and a fixed number of these bounded frames; normalize afterward.

At each point, the retained position inventory is: ordinary root inline leaves;
each of the at most Hp nodes' content leaves; collection presence, uniform/value
leaves and one bound row schema per applicable content role; protected input
leaves; and source snapshot leaves. Use three copies of the maximum profile
extent for collection row/uniform/value storage. This gives the conservative
position bound P = 3*d*(Rp + Hp + 1). A completed row occupies those existing
value/slot/snapshot positions; escaping it stores its profile through a bounded
content edge. Sampling never adds a retained row or observation id per execution.

Every position has a finite label alphabet: its layout; selected target and
capture edges; Writable/ReadOnly/Formal/Unavailable; active scalar partition;
and observation status/dependencies. Heap presence, layout, multiplicity,
coexistence and typed edges also range over finite alphabets of at most Hp nodes.
A binding partition has at most Bell(P) shapes. Let M be the product of these
finite structural-label choices, and K the number of Boolean coordinates in
their declared fact inventory. A one-hot encoding suffices: position labels
use at most P times the sum of their alphabet sizes; graph facts use at most
Hp*L + Hp*4 + Hp*Hp*(|E|+2) + Rp*Hp coordinates; uniform/presence flags add
at most 3*P. These bounds include observer edges and callback capture edges.

For each shape, the number of possible current relations is at most
2^(2^K). Applying the same construction with entry roots gives Me and Ke.
With A availability/audit modes and T concrete targets, context count is bounded
by the finite sum over targets of A*Me*2^(2^Ke). Point states and return relations
use M and K. Saved continuations are keyed by the finite caller context, call
site and canonical caller state, not by a runtime stack or invocation number.

The worklist monotonically unions canonical concretizations. Exact replacement
is monotone as a transfer on input sets; heap merging may lose precision, but
cannot lose ReadOnly, Ended or Many possibilities. Equal alternative objects
are distinguished from coexisting objects by the coexistence rule above.
Canonicalize before comparing states, publish only strict growth and reschedule
only dependents of that growth. These finite domains and monotone joins yield
termination even when recursive calls retain selected rows and observations.
A recursive cycle without a founded base return stays bottom.

This argument fixes the domain and its release rules; the owner
view_access_finite_domain must test their implementation together, including
recursive collection insertion, callback returns, saved observations and frame
renaming. Existing bounded models test the component operations, not that
unwritten owner. No time, depth or state-count cutoff may return a clean verdict,
invent a normal return, or drop an obligation.

## Canonical heap quotient

For one concrete function, conceptually flatten only inline value layout, using
the compact profile/path representation above. A pointer leaf
names a content layout; a callable leaf names its signature and a target-relative
capture-content layout. Do not recursively inline pointed-to objects or function
signatures. Every ordinary root is a finite parameter/local/value/anchor ordinal plus its
inline leaf path. `UnknownObservers` adds one fixed distinguished root. Every heap edge is a pair `(content_layout, inline_leaf_path)`;
dynamic element selection uses the layout's element-summary edge.

For roots `R` and typed edges `E`, the reachability alphabet is
`R × ({entry} ∪ E)`. A root's immediate target receives `(root, entry)`.
Traversing an edge `e` reachable from that root adds `(root, e)` to its target,
regardless of the number of preceding pointer traversals. The role of a location
is the set of labels that reach it. Remove unreachable locations and quotient
locations with equal content layout and role. Union their properties, profiles
and outgoing edges; retain `Many` if any input was `Many` or two locations merged.
Recompute roles and repeat a merge when quotienting introduced new reachability.
Every non-final round removes a location; a renaming-only round stops.

The resulting number of locations is at most
`|Layouts| × (2^(|R| × (1 + |E|)) - 1)`. This is a finiteness bound, not an
implementation allocation size or a performance claim. The worklist constructs
only reached nodes. A recursive tail can merge without merging a distinct direct
root or a different inline sibling seed. Every original rooted edge and backing
property is represented by the quotient; merging may lose precision but cannot
remove a read-only alternative or create singleton authority.

An author-only graph experiment enumerated 4,096 three-node graphs with two
roots, all subsets of recursive edges and both writable/read-only labels. It
checked root/edge/property preservation and merged-node multiplicity. Chains of
2, 3, 10, 100 and 1,000 nodes plus an independent read-only sibling normalized to
three locations. This checks the isolated quotient, not the interpreter, call
translation, wire record, practical context count or a full soundness theorem.

### Call translation obligations

At entry, quotient the argument/dependency closure using the callee's fixed
parameter leaf roots plus UnknownObservers. Save a caller-side map from each canonical input
location to the actual caller locations it represents. This map is continuation
state; it is not part of the callee context or serialized interface.

Inside the callee, retain one protected anchor for each canonical input location.
These anchors keep a detached object available for reporting mutations to an
older caller alias. Their number is bounded by the entry quotient bound, not by
call depth. Local slots and value definitions add finite function-local roots.
A call context contains the normalized incoming graph, parameter values and
selected target, not the continuation's caller-location map.

A returning state contains the result, updated anchored input objects and fresh
objects reachable from those roots. Input anchors translate through the saved
map. Fresh output objects are temporarily distinct from every caller input
object, then normalized under the caller's fixed point/result roots. Merged
locations become summaries. Never reuse a callee allocation-site id as a caller
singleton merely because the same site returned before. A caller object represented by a merged input remains `Many`;
applying its changed contents must preserve alternatives by weak update.

A definite update to one initialized descriptor place may replace that place's
profile. Heap replacement requires a singleton location, an exact selected
inline path and no multiplicity loss at entry, evaluation or return translation.
Dynamic elements and summary locations retain weak updates. Input/output maps
must be checked against alias, recursive escape and write-before-replacement
owners before the capability is published.

### Parameter binding and call-action order

Parameter mode is part of the existing canonical ABI, not a replacement for a
view's backing permission. `ByValue` and `Out` pass a value physically;
`Borrow` and `BorrowMut` pass an address, with the existing cleanup companion for
a Move `BorrowMut` pointee. Preserve those distinctions in the access program.

| Mode | Access binding and existing prerequisites |
| --- | --- |
| ByValue | Copy the completed inline profile into the callee value/slot; contained views retain their addressed storage. A later slice of a copied fixed-array slot addresses that callee slot, not the caller's inline storage. Move ownership/lifetime remains the existing producer's responsibility. |
| Out | Pass the completed descriptor/value as the existing ABI does, not a pointer that replaces the caller's descriptor. Enforce its explicit writable-buffer effect and existing no-read/no-alias rules. It does not make an unavailable or read-only view writable. |
| Borrow | Bind the callee parameter place to the exact caller place. Do not load a new owned aggregate or create independent backing. Existing read-only-place/ownership rules remain prerequisites; native operations such as `buffer.bytes()` still supply their own exact publication permission. |
| BorrowMut | Bind the exact caller place and reflect permitted descriptor/field replacement through the anchored return map. Existing exclusive access, old-generation ending, cleanup companion, Drop-before-replacement and peer-argument exclusion remain unchanged. |

MIR's explicit parameter initialization records remain prerequisites, including
the missing/self-founded Out prologue negative owners. LLVM's borrowed-parameter
slots alias incoming addresses and its `Store(slot, Arg)` marker performs no
runtime copy for Borrow/BorrowMut. The access binding must follow that actual
semantics without letting an implicit slot seed bypass producer validation.
`BorrowedPlace` retypes still use the stored and requested types as specified above.

At each call action, actual target/signature availability is qualified first.
An unavailable actual target rejects; a declaration-only formal target remains a
conditional application. Next consume completed argument observations and enforce
the existing explicit Out/BorrowMut write requirements before entering a callee.
Their existing byte-validation ending effect remains visible even when the called
body happens not to store a byte: this capability does not weaken the local mode
contract from plans 57 and 60. A known read-only BorrowMut slice argument is therefore rejected
even when the declaration audit's callback target is formal. Direct Out calls
retain their mode effect, but source formation still excludes first-class
functions with Out parameters; this plan does not widen that domain. If a required mode
transition needs formal permission or alias data, suspend that audit continuation.
After concrete mode requirements/effects, a formal callback suspends; a concrete
callee receives the resulting graph. Normal returns alone reactivate callers.

Plain by-value view calls add no guessed write effect. Their ordered body program
supplies hidden writes and validations. Argument evaluation order is already
represented by preceding instructions; later argument writes can end an earlier
completed text/codec operand before the consuming call. Returned value profiles,
updated borrowed places and byte-validation observations are distinct outputs.
No caller descriptor is replaced merely because its addressed bytes were written.

### Author call-model evidence

A separate compile-time model exercises canonical input contexts, suspended call
continuations, protected input anchors, return translation and ordered writes.
It accepts readers of writable/read-only/unavailable bytes; rejects plain and
callback writes to read-only/unavailable bytes; preserves literal versus owned
return origins; and distinguishes a replaced descriptor from an older loaded
snapshot. A write before replacement still rejects. An unused read-only capture
does not taint the callback's independent writable return.

The same model reaches fixed points for recursively returned fresh object graphs
and recursively growing known-reader capture environments. The capture case
uses ten canonical contexts and 44 reached states, without substituting an
unavailable callback or using a recursion cutoff. The fresh-result case uses two
contexts and 17 states. These are bounded author experiments, not a benchmark or
proof that the full compiler implementation has these costs.

The model omits dynamic-array initialization, point-specific weak contents and validation observations,
formal declaration audit, native transfers, source type formation and the wire
codec. It therefore provides evidence for the call/alias quotient only. The remaining compiler composition is assigned to the implementation owners;
these bounded results are not a completed compiler test.

### Conditional bodies and closed calls

`Formal` and `Unavailable` are different profile alternatives. A formal input
or callback belongs to a declaration audit; it represents an obligation to be
instantiated by a future caller. It never means writable. An unavailable actual
producer is a failed closed-call proof, not a formal placeholder. A concrete
read-only alternative remains read-only when joined with either alternative.

The same access program is used in both modes, but declaration audit state is
never a closed-call certificate. The declaration audit supplies formal input
locations and formal callback targets. It checks concrete prefixes, rejecting a
known read-only write or ended observation reached without an unresolved formal
obligation. At a write whose permission/alias transition needs an actual formal
input, at a formal callback application, or at a control decision depending on a
formal scalar/tag/length, suspend that audit continuation.
The complete ordered program already contains the continuation; export it without
inventing a callback return, mutable-place update, live observation or empty effect.
Do not confuse a formal-dependent branch with concrete runtime uncertainty:
the latter explores every feasible branch and must reject any unsafe alternative.
Formal dependence propagates through scalar and tag calculations in audit mode;
it is not the concrete `Other` value of the finite scalar domain.
Other concrete audit paths may still produce their own errors or returns. An
error-free suspended audit means conditional behavior remains to instantiate.

This suspension is specific to declaration audit. A closed instantiation cannot
suspend an unresolved actual callback as if it were a formal obligation; it fails.
It replays the actual input graph and callback body, including changed places and
observation invalidation, from the program's entry. It never imports audit states,
returns, memo entries or a Boolean claiming that formal storage is writable.
Declaration and closed contexts use separate mode identities even if a future
implementation shares an interpreter cache. Formal roots cannot enter closed
executable/generated entry seeds.

Per-unit compilation may emit a reusable conditional function together with its
complete access program. That does not make it a native closed root: every safe
language caller must instantiate the program. Executable main and generated
native callback roots require the concrete seed rules below. Existing explicit
unsafe foreign entry contracts are not silently converted into safe source calls.
No returned descriptor or old completed alias is modified speculatively by a
formal callback; its actual update is replayed only during concrete instantiation.

An author model adds twelve audit/actual controls for both validation kinds:
suspension at a formal callback/write, rejection of a formal actual root,
unchanged local invalidation rejection and a concrete read-only write before a
conditional call. Five control cases distinguish a suspended formal branch from
a known safe branch, a known unsafe branch and concrete unknown control. Eight
anchored-output cases cross old/new read-only and writable descriptors and
formal/actual callbacks; a definite replacement can install a writable view
without preserving the old descriptor's read-only permission. The model does not
replace source BorrowMut lifetime/cleanup admission.
Together with the twelve concrete callback cases above, this
checks that suspension invents no normal return or validity certificate. The
source-level owner matrix must additionally cross typed mutable outputs, formal
alias alternatives and lowered materializers. These are implementation acceptance
products; the bounded audit model does not claim to have executed compiler owners.

A genuinely unavailable actual callback fails at application when its access
behavior cannot be qualified. A view from an unavailable native/raw producer may
be read or explicitly copied; a safe write through that view fails. An explicit
unsafe raw store/call keeps its existing unsafe contract, but its result does
not thereby become a proved writable safe view.

Standalone `emit-obj`/`emit-llvm --export` is another explicit boundary. The
existing flag preserves an entry-unit function's externally callable native ABI,
including no-main library/benchmark objects; it does not supply actual Align
arguments. Keep those requested exports as conditional entries with declaration
audit and actual native-body validation, rather than seeding every slice as
writable or treating the export flag as a concrete caller. Calls from other Align
bodies still instantiate their access programs. An external native caller remains
responsible for the existing unsafe ABI/argument preconditions; this capability
does not add runtime permission checks or claim to validate calls made outside
the compiler. Ordinary per-unit public symbols use the same conditional-body
distinction, while an executable main or compiler-generated callback must satisfy
its closed seed contract. Native publication records which entry class was
validated; a conditional export cannot be reused as a closed-main certificate.

Native-generated roots are part of the inventory, even when LLVM constructs the
wrapper: argv entry, SQLite callbacks and synthesized parallel/task consumers.
For argv, `align_rt_args_build` allocates writable outer `AlignStr` storage,
but its text elements remain zero-copy process-lifetime views. Their byte
publication is read-only. The existing entry contract supplies text; this access
analysis adds no runtime UTF-8 validation or allocation.

The SQLite trampoline builds an invocation-local `pkg.db.value` scratch array in
LLVM `alloca` storage. Its ordinary outer slice is writable under the existing
source admission rules. Null/integer/float alternatives carry no byte view.
Text and blob leaves borrow the native provider's read-only storage, including
the static empty sentinel. The producer performs the existing Text validation;
no safe byte write is admitted through either leaf. Repeated native argument
identities reuse descriptors, so distinct argument ordinals cannot prove
non-aliasing. Result Text/Bytes are consumed and transient-copied before invocation
exit, preserving the existing lifetime and non-Send rules. These permissions
follow the exact wrapper and [SQLite provider](https://www.sqlite.org/c3ref/value_blob.html),
not the outer scratch owner.

The closed-entry seed records use these exact existing source signatures and
physical layout owners. Validate the signature before constructing its seed:

| Entry class | Signature/layout and seed |
| --- | --- |
| Main without argv | `fn() -> ()`, `fn() -> i32` or `fn() -> Result<(), Error>` under the existing canonical Error validator. No parameter/capture storage is introduced. |
| Main with argv | One ByValue `array<str>` parameter and `Result<(), Error>` result. The array has writable outer storage; each contained Str has process-lifetime read-only bytes. Zero length is permitted. No other main mode or result is inferred from the C wrapper. |
| SQLite scalar callback | One ByValue `pkg.db.sqlite.function_args { values: slice<pkg.db.value> }` parameter and `Result<pkg.db.value, str>` result. `sqlite_callback_contract_types` owns the exact nominal names, field order, enum payload layouts and non-C-repr/no-align checks. The outer values slice is writable invocation-local scratch; Text and `Bytes(byte_view { bytes: slice<u8> })` leaves are read-only native views. |
| Parallel or task wrapper | The selected operation's validated Fn ABI and explicit trailing capture ABI, with actual source element, accumulator, range and capture profiles supplied by that operation's transfer. These are synthesized applications of the retained source operation, not an unrelated root that seeds every parameter as fresh writable storage. Existing Pure/task lifetime/output admission remains required. |

The SQLite input trampoline publishes exactly Null, I64, F64, Text and Bytes
variants (ordinals 0, 4, 6, 7 and 8). Bool, I16, I32 and F32 are valid members of
the shared value schema and may be returned by Align, but are not input tags
produced by this wrapper. Its F64 path rejects NaN before invoking Align; text
and blob size/pointer/UTF-8 failures likewise have no callback continuation.
Scalar abstraction may conservatively forget a numeric distinction, but cannot
use the schema's extra output variants as evidence of a writable input view.
Repeated native-value pointers reuse the already materialized descriptor,
including its alias relationship; invocation ordinal alone never proves distinct
text/blob backing. The callback result is consumed under the exact validated
Result/value/Str ABI before scratch teardown.

No entry wrapper may silently supply a formal-origin placeholder as a closed-call
proof. The seed inventory above correlates signatures, layouts and producers;
the implementation owner must reject independently forged modes, layouts and
entry classes. Source permission is not inferred from an LLVM pointer type.

## Interface integration decisions

The current interface format is 12. This capability changes it once to 13 and
adds one length-prefixed canonical access bundle after the constant records in
`write_surface`. The complete bundle participates in `interface_hash`; existing
capabilities and hash trailers retain their ordering and exclusion rules. No
separate permissive old-format path or compatibility marker is added.

Use the existing backend-independent `CanonicalTy` and `CanonicalFnAbi` codecs
for complete reachable type/signature identity. Their byte records already
validate nested definitions, nominal names, fields, modes and callable facts.
Do not replace those records with a shallow type-name hash or process-local Ty
id. Access layouts are derived from the validated type graph and the compiler's
closed native-carrier schema. Ordinary inline recursion remains rejected by the
existing type-formation rules; pointer/callable recursion is represented by graph
references.

The existing canonical decoder already reconstructs and validates definition
tables internally in `canonical_type_record_len`; its public `decode` returns
only validated opaque bytes. Factor that existing parser so the MIR access-layout
owner can retain the same validated decoded graph. Do not write another partial
canonical-type decoder or reconstruct layouts from source-visible interface names.
Private helper-local types remain analysis metadata and are not inserted into
Sema's source-visible import namespace. The parsed graph and compact derived
layout may be shared by canonical-byte identity inside the compiler; their local
ordinals never become type identity across records. The borrowed graph view is
valid only while its owning decoded record is retained. No decoded record exposes
runtime reflection or performs artifact/source I/O during generated execution.

#### Resource identity in the access projection

The existing canonical resource node includes `drop_hook` as well as the public
thunk metadata (`canonical_graph.rs`'s resource encoder and decoder). A producer
stores its real private hook there, while an interface-only import reconstructed
by `render_resource` stores an inert surrogate. Comparing those unmodified
canonical bytes would reject equivalent producer/consumer resource types. This
is an access-bundle integration constraint, not evidence of an existing native
ABI failure.

For this access projection only, replace the resource node's `drop_hook` text
with its validated `drop_thunk` text before using the existing canonical codecs.
Preserve every other resource field and every other reachable definition field.
Apply that same projection to standalone types, nested types and function ABIs
on both sides of an access boundary, including imported entry ABI comparison.
The access codec must reject resource nodes whose projected hook differs from
their thunk. The parsed graph is analysis data; it cannot be passed to native
lowering as an actual resource program. The source formation binding and final
native ABI validation still use the original, complete MIR resource metadata.

Before this projection, the local derivation resolves each actual resource Drop
hook through its declaration-owned support entry. The separate
`AccessDropBinding` preserves that behavior after hook-name normalization.
Normalization cannot supply a hook body, change a Drop target, or certify a
surrogate as a body. The ordinary resource metadata validator, exact support ABI
and owning interface resource record remain prerequisites. Tests must compare a
real producer hook with its reconstructed import surrogate, reject a mismatched
thunk/ABI/owner, and distinguish two different hook behaviors behind the same
public thunk. Generic declaration-owned thunks require the same checks without
inventing a concrete resource argument type.

One bundle contains its concrete exported entry mapping, declaration-owned
resource Drop entries, and the transitive local access programs reached by direct
calls, constructed callable targets and implicit cleanup. Helpers in that closure
are metadata, not new source-callable exports. A generic exported declaration
carries its existing generic body; its access program is derived only after
concrete monomorphization. Concrete helper instances reached from an exported
ordinary body remain part of that body's bundle. A generic resource's existing
type-erased producer record roots its declaration-owned Drop entry even when no
concrete resource instance exists in that producer.

Private functions have bundle-scoped ordinal identity. Their source/lifted names
are retained only in a separate diagnostic map, not the canonical access bytes.
An imported call names an entry in the selected dependency bundle; it does not
copy foreign private bodies into a global function-name table. Existing logical
`ProgramCall` names identify ordinary exported entries. A Drop entry uses the
already-public `IResourceDef.drop_thunk` identity and ABI; this is an explicitly
validated support identity, not an arbitrary native symbol or a source-callable
export. The exact scoped closure and traversal are specified below.

The Drop entry is the compiler-owned support thunk, not necessarily a hook body
in the resource's declaring unit. Existing resource formation requires a
non-generic public hook in that unit's internal subtree; the declaring unit can
import that hook from its internal dependency. Derive the entry's local access
function from the same exact support contract used by native emission: one
ByValue raw parameter, one ordered Align call to the actual hook with that
parameter, then a unit return. Resolve that call locally or through an ordinary
imported hook entry as appropriate. If the hook does not return, the wrapper has
no normal continuation. This synthetic access body adds no runtime wrapper or
allocation: it represents the support thunk the compiler already emits. Bind it
to the original resource metadata and actual hook ABI in SourceProgram formation
and final support-partition validation. Never mint it from an import surrogate.

Keep the structured access program and interpreter in MIR. Sema's imported
program record carries shared opaque canonical bundle bytes so it does not
depend on MIR or invent a second access evaluator. The interface layer validates
the bundle and its entry mapping; MIR decodes and validates again at its actual
boundary. Source checking must run the necessary backend-independent lowering
and MIR proof rather than stopping at sema. Per-unit publication uses that initial source proof. Native emission retains
its bound source proof and validates actual physical MIR through the existing
native owners; cache rehydration follows the same phase order.

Local access programs are always recomputed from actual MIR; a copied local
summary or producer-certification Boolean cannot substitute for that derivation.
Imported bundles use the existing compiler-produced artifact trust boundary and
cache identity. Structural validation and a recomputed hash detect malformed or
stale records; they are not cryptographic proof that arbitrary externally forged
metadata describes an independently forged object file. This capability must
not claim that stronger property.

The exact bundle/instruction tags, sequence order, validation precedence and
full-record golden vectors below define this integration. Format 13 is the
single implementation transition; the current compiler still writes format 12.

### Cache contract and source-of-truth closure

The exported access program is a public soundness summary even when it includes
a private helper's behavior. A body edit that changes this program changes
`interface_hash` and invalidates reverse-dependent frontend/codegen results.
A private function outside every exported program's dependency closure remains
an implementation-only edit. An edit whose derived canonical access program is
unchanged also preserves the interface hash. The producer must not claim that
every private-body edit preserves consumer cache hits once consumers depend on
its ordered writes, observations, callbacks or scalar control results.

Keep dependency keys based on `interface_hash`, not the entire serialized summary
or `impl_hash`. The new access field belongs in the existing surface hash; it is
not an additional ambient cache input. The driver already includes the complete
transitive dependency interface-hash set in the frontend/codegen key. Resolve an
import only against that invocation's selected, validated dependency bundle.
An import records its unit, entry and ABI; it does not recursively embed a
dependency's hash or body. A dependency access change therefore invalidates
consumer cache keys through that existing transitive set. Recursive local helpers
use scoped references, without recursively expanding or hashing their call paths.
Two dependency records claiming the same selected unit must have the same
validated interface identity before any entry is usable; never merge their
private function tables by name or accept the first conflicting record.

| Normative owner | Required integration in the capability implementation |
| --- | --- |
| This plan | Exact access records, source-event semantics, unknown/formal rules, dependency closure and acceptance owners remain authoritative here. |
| `draft.md`, `docs/language-spec.md` | Preserve literal/constant write rejection and explicit owned-copy behavior; describe function-boundary validation and qualify the private-dependency-body cache promise by unchanged public soundness summaries. |
| `docs/design-notes.md`, `docs/open-questions.md` | Record closure of ordinary argument/result laundering and the reason ordered access behavior is part of the public summary, without reopening the locked ownership or mutability model. |
| Plans 10 and 07 | Update the cache key/acceptance rows that currently promise all private-body edits preserve consumers. Keep the interface/implementation hash split and distinguish unreachable-private changes from changed exported access behavior. |
| Plans 52, 55, 57 and 60 | Mark the interprocedural boundary only at capability completion; keep local origin, producer, UTF-8 and codec contracts aligned with the new caller instantiation owner. |
| Plan 19 and compiler interfaces | Add the source-use event/replay and canonical access-bundle records to the checked-body inventory; update `align_interface`'s public-surface documentation and format version once. |

The cache owner must change a hidden writer behind an unchanged exported
signature from reader to writer and back, observing changed interface identity
and changed dependent verdicts in separate processes. It must also keep an
unreachable private-body edit as a consumer hit, and keep a comment-only edit
from changing the canonical access bundle. Scalar-only public results that select
a caller's access branch are part of the input behavior and cannot be discarded
solely because their own return type carries no view. These are correctness/cache
identity promises, not elapsed-time benchmark requirements.

### Persistent frontend reuse without MIR

The existing `UnitBody::Reused` path carries no MIR: it obtains summary bytes,
replayable diagnostics and link libraries from UnitEntry, then can address an
already compiled object by the summary's implementation hash. Requiring an
in-memory CodegenProgram before every cache lookup would silently remove this
shipped frontend-cache capability. Distinguish reuse of a previously admitted
compiler-produced artifact from new native emission.

Keep the existing unit manifest shape and key comparison/digest/trailing-byte
validation order. The interface-format version is already a unit-key component. Changing it from
12 to 13 invalidates older frontend entries for this access contract. A miss may
publish a clean descriptor-free UnitEntry only after complete source admission
and canonical interface-bundle construction. The separate object cache publishes
only after actual native validation/emission. A hit therefore requires both
existing keyed, validated records; a frontend manifest alone cannot authorize a
new object or create a CodegenProgram.

After a matching unit-manifest key and value digest, strictly decode and validate
the interface access bundle and selected dependency entry/ABI identities before
replaying the cached unit as an admitted input. The fresh unit key already binds
exact source bytes, entry-unit role, compiler/interface versions, explicit keyed
toggles and complete transitive dependency interface identities. Any change in
hidden access behavior changes either the unit's own source key or a selected
dependency interface hash. A private main need not become a public interface
entry merely to repeat a proof of byte-identical source under identical inputs.
No new proof Boolean, whole-MIR persisted codec or full-unit public hash is added.

Use an opaque driver-owned cached-unit admission record, constructed only by that
successful lookup path and borrowing/owning its exact validated UnitKey and
UnitEntry. The Reused body carries this record with link libraries instead of
only a permissive absence of MIR. The cache-hit codegen route consumes this
record and checks its summary implementation hash against the requested object
key. It cannot accept an arbitrary public summary accompanied by `is_reused`.
On an object miss, located request or any operation needing native MIR, rehydrate
the unit through the complete source/finalization/native-validation route and
compare the existing summary, implementation and diagnostic identities before
publication. Never recreate source events from an optimized cached body.

The current package object path fills absent hit-unit MIR entries with one
`Program::default()` solely to keep a vector indexable, relying on the `misses`
list never to read them. Remove that placeholder when changing the carrier; an
empty raw program cannot stand in for either cached admission or a qualified
empty source unit. Lookup inputs must distinguish a borrowed qualified
materialized unit from an exact CachedUnitAdmission. After lookup, materialize
every reused miss, then assemble emission jobs only for those actual misses,
each carrying its original unit index and complete CodegenProgram. Missing or
duplicate job indices, a job for a hit, or an unmaterialized miss is an error
before worker scheduling. No constructor silently fills absent jobs with empty
owners.

Borrowed module qualifications must not become self-references inside an owned
worker task. PipelineTask owns the complete immutable CodegenProgram and its
explicit selection inputs; the worker obtains a fresh borrowed qualification
from that owner before emission. The calling phase still qualifies materialized
units before the existing cache/artifact boundary. A pure frontend/object hit
uses the cached admission path and starts no worker. Preserve the existing
`all_hit_starts_no_worker` and `panicking_worker_notifies_joins_then_resumes`
owners: the latter currently injects a default raw MIR before a forced worker
panic, and must use a small genuinely admitted fixture instead of introducing a
public empty-certificate constructor just for its test. Queue cancellation,
in-flight accounting and joining before panic resumption remain unchanged.

This preserves the existing compiler-produced artifact trust boundary: manifest
value digests and object identity detect corruption/staleness, not coordinated
forgery of all cache records. In-process body mutation has no corresponding cache
keyed lookup and cannot use this reuse route. The source/native wrapper checks
remain mandatory for every newly emitted or rehydrated body. A supplied raw
partition cannot obtain qualification from a cached-unit admission record.

Acceptance adds all-hit fresh-process reuse, hidden writer changes in own and
dependency source, unchanged unrelated private helpers, malformed access bundles,
changed entry role, object miss rehydration, and rejection of a mismatched cached
summary/object request. Preserve descriptor exclusion and warning replay. The
owner measures cache hit/miss outcomes, not a new timing promise. The driver
carrier's exact fields/signature belong with the entry-selection API closure;
this record closes the mandatory distinction between cached artifacts and new
native emission.

### Descriptor implementation data must remain private

Plan 17 §6.2 explicitly excludes source/wire SQL, rewrite maps, occurrence tables,
checked metadata and generated bind/decode thunks from `interface_hash`. Only the
exported Params/Row/driver/static semantic-option contract participates. Plan 10
repeats this rule. The existing `pkg_db_q1` identity owner changes `'name' AS name`
to `'renamed' AS name` and asserts unchanged producer interface/dependent hashes
while implementation/artifact identities change. This requirement is preserved;
it is not weakened by calling every serialized private body a public summary.

The first full-MIR field-copy proposal violates that rule: StaticData contains SQL
and metadata bytes, and generated descriptor helpers contain query-dependent
scalar constants and tables. Omitting only SQL text bytes would still export
private occurrence/ordinal/metadata behavior through those helpers. Generated
helper logical names are stable (`$static_bind_v2`, `$parameter_ordinal_v1` and
related roles), so symbol renaming is not the defect. Exported source access must
represent the descriptor's public typed behavior without these implementation
inputs. Private scalar/length values at that descriptor inspection boundary must
remain abstract whenever publishing them would expose forbidden implementation
identity; downstream checks explore their possible outcomes conservatively.

The abstraction boundary is semantic, not a rule that discards every generated
scalar. Plan 17 requires one binding per Params field and rejects unused fields;
therefore the generated parameter-count thunk is exactly the public Params field
count. Its access result may retain that constant. Protocol ordinals, however,
follow first SQL occurrence; their exact positive/negative ordinal values cannot
enter the public bundle. An eligible parameter's sign follows its public native
option, while its magnitude remains abstract over the permitted nonzero ordinal
range. Query metadata similarly separates public driver/detail/field-order facts
from private verification evidence, SQL hashes and origin/alias contents.

The current direct materialized decode thunk supports scalar-only Row fields; it
does not return fresh owned text or byte buffers for a view-bearing Row. The stream
decoder forms resource-tied views via `ResourceViewFromRaw`; the batch decoder
borrows independently materialized batch storage. Public `one`/`maybe_one`/`all`
clone view fields into the explicit caller region in their ordinary package bodies.
The access record must preserve those three different producers and the direct
scalar decoder's unsupported-row termination, not invent a successful fresh-owned
return for every generated decode role.

Use the dedicated initial-MIR `DbDescriptor` producer,
paired with the typed `DbBridgeCall` defined above. Its public payload is the
Query or Command witness, driver restriction and effective static semantic
options. The witness contains the descriptor result type and complete P/R
types. Descriptor id, source-unit/item lookup and constructor spans remain
private source formation/finalization data. Initial MIR retains its sealed
constructor binding; access projection never serializes that private id. SQL source kind/path/text, artifact bytes/hashes,
occurrence tables, verification evidence, generated native helper identities and
materialized metadata are absent. This producer publishes the descriptor's
compiler-owned static backing and the exact permitted role inventory. It does
not expose fourteen arbitrary function addresses as qualified source callbacks.

Initial MIR uses concrete remapped types and the existing semantic option enums.
Source derivation converts them through the existing pure canonical contract and
option validators; it performs no SQL or artifact I/O. The effective option list
contains exactly one Common/Check entry. An omitted check normalizes to
DeclaredOnly, matching its explicit spelling under plan 17's existing artifact
contract. Driver-incompatible options, duplicates, unknown Params fields and
unsupported canonical PostgreSQL type names remain errors in their existing
owner. The finalizer consumes the sealed descriptor request and qualified
artifact, substitutes the actual constructor/generated roles, then validates
the native body independently. Neither a package-name prefix nor copied public
metadata beside a changed native body grants that authority.

The qualifying producer covers constructor return, bind/decode callbacks,
copied/borrowed outputs, scalar metadata and all generated roles before source
access interpretation. The exact record shapes below close field presence;
the guarded-call/constructor vectors below cover representative complete records.
The producer-to-native owners must qualify the implementation before publication.

Implement the pure-check/materialization relation with the codec; The 335-variant baseline inventory does not authorize
publishing native artifact data. Omission is a closed rule: source locations and
diagnostic anchors remain outside canonical access bytes; compiler-generated
ParMap work hints are omitted as already specified; native StaticData and column
batch materialization records are reserved and replaced by the two semantic DB
records. Ordinary private helper names become rooted target ordinals; descriptor
ids stay in private constructor bindings. No-alias scope metadata and native
borrowed-element reservation identities follow the explicit projection table
below. Every other retained source instruction field follows the exact canonical
table below.
There is no general filter for strings or scalars that look like implementation
or formatting data. A returned message, length or scalar can select a caller's
access branch, so omitting it without a producer-owned abstraction could preserve
a consumer hit after a soundness-relevant behavior change. Any additional
omission requires its exact semantic producer, identity rule and owner in this
ledger before changing the codec; it cannot be a post-hoc hash exclusion.

The cache owner closes both directions: SQL/metadata/occurrence-only edits with an
unchanged public descriptor contract retain consumer hits; a changed public
Params/Row/driver/options or actual exported safe-access behavior invalidates
consumers. Renaming a reachable private descriptor changes its private source
binding and implementation identity but leaves projected access bytes unchanged.
The exported entry key, when present, remains public and is not renamed by this
invariant. Keep this negative control in the existing descriptor identity owner. Existing malformed descriptor/ABI owners must reject independently of
whether these consumer hashes are unchanged. No performance benchmark is needed
to test this identity invariant.

### Generated descriptor role inventory

These are the fourteen actual producer roles, not fourteen arbitrary private
functions selected by spelling. `P` and `R` are the complete descriptor contracts;
`RowsRef<R>` and `BatchRef<R>` mean the existing one-pointer `ResourceRef` ABI,
not a new source type. Parameters are ByValue except the stated shared `P`.
Every result has the current `ReturnCleanupAbi::None`. Stream and batch Row
returns retain borrow/region root 1 exactly when `R` contains views; SoA retains
root 1. This table fixes the source of the remaining transfer/qualification work.

| Role | Actual internal signature | Access behavior to preserve |
| --- | --- | --- |
| Bind | `(raw context, borrow P, u8 mode) -> i32` | Read the checked Params fields; preserve the context action and mode/status control. No result view and no transfer of Params ownership. |
| ParameterCount | `() -> u32` | Exact public Params field count. |
| ParameterOrdinal | `(str name) -> i32` | Read the name; miss is zero. A matching public field yields its private protocol-order magnitude, with binary-eligibility sign determined by its public native option. |
| StaticValidate | `(raw context) -> i32` | Driver/context validation and status; no returned view. Raw context effects need their qualified owner, not an empty-call default. |
| RowValidate | `(raw context, u8 mode) -> i32` | Metadata/value validation against the public Row shape and native status; no Row publication on this role. |
| Decode | `(raw context) -> R` | Scalar-only supported Row production. View-bearing/unsupported Row has no normal continuation in this direct thunk. |
| StreamDecode | `(raw context, RowsRef<R>) -> R` | Scalar/optional fields and validated resource-tied text/byte views; no owned copy. The raw-view producer's owner, flags and presence relations are required. |
| QueryMeta | `(u8 driver, u8 detail, i64 index) -> Option<QueryMeta>` | Static read-only text leaves; public row order/detail structure and driver restriction; private evidence/content remain abstract. None publishes no Row profile. |
| BatchCreate | `(i64 max_rows) -> raw` | A fresh bounded batch payload or null for invalid size. Its logical row state starts empty; no caller view is retained merely by creating it. |
| BatchAppend | `(raw batch, raw context, RowsRef<R>) -> i32` | Copy a current row into batch columns/child storage after validation; retain the independent batch storage and exact failure publication rule. This is not a shallow retention of stream byte pointers. |
| BatchFinish | `(raw batch, i64 rows) -> ()` | Finalize/compact the batch backing before projection; do not keep descriptors to relocated fixed columns. |
| BatchRow | `(raw batch, BatchRef<R>, i64 index) -> R` | Borrow selected column/child storage from the batch generation; returned text and byte permissions come from those producers. |
| BatchSoa | `(raw batch, BatchRef<R>) -> soa<R>` | Present only for the existing SoaPlain Row domain; borrow actual batch columns. |
| BatchDrop | `(raw batch) -> ()` | End/free fixed columns, every child chain and payload under the existing release owner. No normal returned storage. |

Role presence is derived, not supplied as independent flags. Commands have
exactly Bind, ParameterCount, ParameterOrdinal and StaticValidate. Queries add
RowValidate, Decode, StreamDecode, QueryMeta, BatchCreate, BatchAppend,
BatchFinish, BatchRow and BatchDrop. BatchSoa is added exactly when the Row has
at least one field and every field is Bool, Char, Int, Float or Str under the
existing SoaPlain formation rule. Optional fields, bytes and an empty Row do not
qualify BatchSoa. Role presence is separate from normal-return support: the
scalar Decode role exists even for a view-bearing Row, where its body terminates
without a result. A malformed record cannot suppress an existing role or add a
SoA callback merely by claiming an optional flag. The native generator must
match this set and its descriptor relocation presence before finalization.

The physical shape inventory alone does not decide native context aliases,
failure/presence alternatives, fixed-column relocation or byte-child permission.
The transfers below retain the generated implementation and D13 ledger contracts.
Neither handle ownership nor a shared receiver alone decides byte permission.

`column_batch_create` allocates and clears a fixed column block and its own
payload. Each view-bearing logical field has its own child-chain entry in that
payload. `column_batch_append` copies nonempty bytes into the selected field's
allocator-owned chain and stores a descriptor of the destination. It neither
retains the source pointer nor shares a child chain between different logical
fields. Multiple rows in one field may share a chunk, so the abstraction may
not claim independently allocated rows merely because their indices differ.
The existing weak element and multiplicity rules apply when the row index is
unknown. Known disjoint fields remain distinct even when a broader row role
contains both text and bytes.

An empty present view stores the all-zero slice header without dereferencing its
source; an absent optional view publishes no view payload. A None field is not
a view whose null pointer gains permission. A present empty bytes field has the
same qualified writable producer as its nonempty case, with bounds excluding
element access. Batch text bytes are read-only; batch byte slices have writable
storage because the actual producer allocates and initializes their destination.
Fresh copied text does not retain the stream/input's validation observation.
The successful copy must first satisfy the input read/validation obligations.
The qualified role still ties every projected descriptor to the batch lifetime;
no copied child acquires an independent source owner.

`column_batch_row` loads scalar values and view headers from fixed columns. A
nullable field tests its bitmap and loads the field only on Some; its None path
is zero-initialized. `column_batch_finish` may relocate fixed columns while
keeping child chunks stable. Consequently BatchSoa may publish fixed-column
views only after finish, while BatchRow's copied headers still point into the
same child storage. Drop ends both storage classes. The semantic producer must
preserve initialization, finish-before-projection and no-publication-on-failure;
these facts do not follow from the returned Row signature alone.

BatchRow and BatchSoa must share the same generation's mutable fixed-column
content cells. SoA contains the fixed-block pointer and length; it is not an
independent snapshot of the original decoded headers. Any source-admitted column
header replacement must therefore be visible to a later BatchRow projection,
including the replacement text's backing and validation observations. Do not
reseed every BatchRow/BatchSoa invocation with fresh original-decoder profiles.
The initial copy rule above applies when the producer appends the row; subsequent
projections read current column state. An already completed Row keeps its copied
header snapshot when a column header changes, while aliases of the column share
that change. Existing source retention/region rules decide which replacements
are admitted; this plan adds no array-field assignment surface. The owner matrix
must include repeated projections, shared column updates, unaffected columns
and a completed pre-update Row, with a validated-text replacement where existing
retention rules admit it.

The current compile-only probe bounds that header case. A repeated BatchRow/SoA
read and an `id` scalar-column replacement both pass whole-program and per-unit
checking. Direct replacement of a batch `label: str` column with a literal fails
both modes because the destination's backing storage is not known to the existing
retention checker. Whole-program borrowed-text replacement and a shared helper
using `borrow mut slice<str>` also fail that backing/retention boundary. Keep these
as rejection controls; do not widen native-view retention to create an additional
accepted header-mutation owner. Current accepted scalar-column updates still
require shared column state and completed-row snapshots in the access model.

The binding side has a separate copy boundary already fixed by the DB design
§5.6.1. Bind reads Params during the operation and cannot leave a source Params
view in returned statement/rows state. SQLite uses TRANSIENT copies; PostgreSQL
finishes synchronous transmission or retains execution-owned bytes until its
asynchronous protocol is synchronized. A copied parameter does not carry the
source byte-validation observation into that execution-owned storage. Partial
bind failure retains the existing cleanup obligations and cannot publish a
successful resource profile. The exact driver/context state transitions remain
part of the role proof; the source-level no-Params-retention rule is not an empty
native effect.

### Bundle envelope and body identity

The outer record, Rvalue payloads and complete body canonicalization form one
codec contract. The envelope alone cannot validate an access program.

All counts, lengths and ordinals are unsigned `u32`, little-endian. A byte string
is its byte count followed by exactly that many bytes. A sequence is its element
count followed by records with no padding. Names are byte strings containing
valid UTF-8 without embedded NUL, validated by `ProgramCall`; ordering is by
unsigned UTF-8 bytes, independent of locale. Producers reject an unrepresentable
count or length before serialization. Decoders validate against remaining input
before allocation and use iterative worklists for nested type/projection records.
No allocation is sized directly from an unvalidated claimed count.

| Record | Fields in exact wire order | Canonical constraints |
| --- | --- | --- |
| AccessBundle | `types: sequence<byte_string>`, `abis: sequence<byte_string>`, `imports: sequence<AccessImport>`, `programs: sequence<AccessFunction>`, `entries: sequence<AccessEntry>`, `drops: sequence<AccessDropBinding>` | Types and ABIs are strictly byte-sorted, duplicate-free records from the access projection through the existing canonical codecs. Imports and entries follow the key order below; programs follow rooted discovery order; drops are strictly increasing by resource TypeRef. No separate bundle version: interface format 13 owns this schema. |
| AccessFunction | `abi: u32`, `params: sequence<u32>`, `slots: sequence<u32>`, `values: sequence<u32>`, `snapshot_count: u32`, `entry: u32`, `blocks: sequence<AccessBlock>` | ABI references the bundle ABI table. Param ordinals select distinct function slots in physical parameter order; slot/value entries reference type records. Slot types/modes/return agree with the ABI. Block ordinals are sequence positions; entry is zero. No private function name is serialized. |
| AccessBlock | `instructions: sequence<AccessInstruction>`, `terminator: AccessTerminator` | Instruction order is evaluation order. Rvalue payloads use the closed schemas below; no arbitrary executable bytecode or native symbol is accepted. |
| AccessEntryKey | `tag:u8`, `name:byte_string` | Tag 0 is an ordinary concrete exported logical function name; tag 1 is a declaration-owned resource Drop thunk. Other tags reject. Order by tag, then unsigned name bytes (not the encoded length prefix). |
| AccessEntry | `key:AccessEntryKey`, `program:u32` | Program references a local definition with the exact entry ABI. A function entry matches its concrete interface function. A Drop entry matches an exported resource's validated thunk and the fixed `(raw) -> ()` ByValue ABI, with no return roots or cleanup. Generic function declarations have no concrete entry until instantiated. |
| AccessImport | `unit:byte_string`, `key:AccessEntryKey`, `abi:u32` | Unit selects an existing dependency interface, key selects one of its validated entries, and ABI must agree after access resource projection. Order by unsigned unit bytes, then entry-key order. Duplicate keys reject even when ABIs agree. |
| AccessAlignTarget | `tag:u8`, `index:u32` | Tag 0 selects a local program; tag 1 selects an import. Other tags reject. A target's complete ABI comes from its selected record. |
| AccessDropBinding | `resource:u32`, `target:AccessAlignTarget` | Resource is a standalone owned-resource TypeRef required by a retained cleanup plan. Target selects its local declaration-owned support access function or the imported declaration-owned Drop entry; a Drop binding cannot select an ordinary imported function entry. |

Every local Align program in a published bundle is reachable from an entry through
Align direct calls, constructed callable targets or implicit cleanup. Foreign
calls use the exact target record below rather than a fabricated Align body;
unreachable private bodies are excluded. Every import and Drop binding must be
used by this closure. Every type/ABI table record is referenced by a retained
program/instruction, import, entry or cleanup requirement. A unit with neither
concrete exported functions nor exported resources has the empty bundle: six
zero `u32` counts, or 24 zero bytes. The outer interface field prefixes those bytes
with length 24. The vectors below include projection, call, Drop, import and nonempty source
subjects; the implementation codec must consume those independent golden bytes.

Functions lifted from closures use their complete physical MIR parameter list,
including trailing captures, and the already-remapped return summaries. Callable
construction additionally validates its explicit source signature and the exact
trailing capture suffix against that target; an exported entry cannot claim a
lifted capture ABI as an ordinary source signature. The source signature and the
physical target signature are different validated records, not a guessed capture
count inferred from whichever call happens to appear first.

Imports retain their selected bundle scope during interpretation. Local program
0 in two different bundles does not identify the same function. Resolve each
import once against the compiler-selected dependency set, reject an absent unit,
entry or disagreeing projected ABI before declaration audit or closed replay,
and retain the resolved bundle with the decoded program. A body instantiated from
an imported generic template is a local definition in its consumer's access
program; it is not an invented concrete entry in the template producer.

The source program includes source-use events and finite scalar/control operands
needed by its transfers. Debug spans, source lines, allocation addresses, absolute
paths, hash-map iteration order and codegen linkage hints are excluded. Private
names and process-local ordinals are excluded by the following traversal.

Validate the complete actual input first, including structurally malformed
unreachable blocks. For the published projection, walk each function's CFG in
breadth-first order starting at its entry: Goto adds its target, Branch adds then
followed by else, and terminal returns/Unreachable add nothing. Add a block only
at its first discovery; discard unreachable blocks from the projection. Do not
fold a constant condition or drop a called nonreturning block's syntactic tail
as part of identity normalization. Those remain the interpreter's semantics.

Assign physical parameter slots first in ABI order. Then scan canonical blocks,
their statements, and their terminator, visiting typed record fields in wire
order. Assign each remaining slot, value and source snapshot its next ordinal on first
typed reference or definition after native-only projection. Rewrite references
through the resulting maps; discard unreferenced non-parameter table entries.
Keep ordinary integer constants unchanged. An inline field/index ordinal is
not a slot/value identity. All retained definition, dominance and source
snapshot-CFG checks precede interpretation.

Initial source formation and MIR producer checks validate admitted operation
shapes before projection. Native proof metadata stays in the immutable
initial/final MIR; it is not an access fact. LLVM's existing borrowed-element
owner reconstructs its bounds proof at pointer formation. Closed lowering and
finalization preserve source no-alias obligations and map_into scope assignment;
this plan does not invent a generic scope-pair validator or move target-specific
validation into source admission:

| Actual MIR input | Canonical access representation | Proof owner retained |
| --- | --- | --- |
| PtrStoreNoalias(pointer, index, value, scope) | PtrStore, statement tag 5, with the same three operands. Statement tag 6 is reserved and rejected. | Actual native no-alias metadata and producer validation. |
| SliceIndexNoalias(slice, index, scope) | SliceIndex, Rvalue tag 272, with slice then index. Rvalue tag 273 is reserved and rejected. | Actual native no-alias metadata and producer validation. |
| BorrowedElementReservation(token, root) | Omitted inert statement; statement tag 25 is reserved and rejected. | Actual reservation marker/root and successful bounds-edge dominance. |
| BorrowedElementPlace.guard | Retain its length operand and all typed place/index fields; omit only the reservation id. | Actual marker/use/root/length relation and post-rewrite bounds proof. |

SourceEvent remains the owner for evaluation-time observation reservations and
completion. Those source events and snapshot ids are retained; they are not the
native borrowed-element marker. Private source-to-access location maps account
for omitted statements and identify the original action for diagnostics.
No scopes or native reservation counts/tables exist in AccessFunction.
An imported access operand validates retained types, lengths, indices and modes;
it does not manufacture a native bounds/no-alias certificate. A malformed actual
borrowed-element marker must still fail its existing native owner even when
its access projection equals a valid program. Scope ids come from the trusted
map_into lowerer and remain inside the immutable phase pair; access decoding
supplies no replacement LLVM metadata. Alpha-renaming valid native identities
must preserve canonical access bytes.

Sort entry keys first and seed a FIFO function queue with each entry's selected
local function, assigning a program ordinal on first discovery. Walk each queued
function in the canonical block/field order just defined. A referenced local
Align target is assigned once and appended to that queue; an imported target is
recorded without traversing its foreign private body. At each cleanup statement,
also discover the required resource hooks, in ascending canonical owned-resource
type-byte order. This discovery order determines identity only; execution still
follows the existing type-directed Drop order and active variant/null state.
Ordinary calls and resource cleanup can discover the same local hook only once.

Derive this closure afresh from the entries of the interface being written. Do
not copy ordinals from a whole-program/source-program table that also contains
unrelated private functions or test roots. Build and sort the referenced type,
ABI, import and Drop tables, then remap every typed reference. Recursion uses
already-discovered ordinals and requires no recursive body hashing. The decoder
repeats the same rooted traversal to reject noncanonical numbering and redundant
records. Structural graph normalization does not promise that semantically
equivalent but differently written computations have identical access bytes.

Drop, DropValue, DropElem and DropElemField use the selected type's existing
cleanup plan, including nested aggregates, active enum/Option/Result payloads,
owned collection elements and resource leaves. Retain a binding for every
resource hook that such a retained cleanup can select; do not approximate this
by scanning explicit Call records or by including every resource in the unit.
The interpreter invokes the selected hook with the consumed resource's raw
identity and caller observation state before completing that leaf's cleanup.
Null/moved-out leaves do not call the hook. Unknown alternatives explore the
existing feasible cleanup paths; a nonreturning hook has no continuation.
Detailed native/raw hook effects still belong to the transfer inventory below.

The saved author traversal model permutes private names, function/block table
order and typed local identities in 1,000 independently seeded cases. It predates
the native-only projection correction; scope/reservation omission and source
snapshot renaming require the implementation controls specified above.
Its recursive helper graph includes an imported entry, a dormant generic Drop
root, a Drop-only hook dependency and an unreachable private helper. Every
permutation preserves the normalized record. Changes to reachable helper/Drop
behavior, integer constants or ordered branch successors change it; an
unreachable private-body edit does not. Malformed unreachable CFGs still reject
before projection. This closes the proposed traversal on that finite graph; it
does not validate the full instruction codec, resource type projection, imported
ABI resolution or the access interpreter.

Validation precedence for the envelope is structural scan in wire order (length,
UTF-8/NUL, nested canonical type/ABI error, unknown instruction tag), complete
record consumption, canonical table/name order and duplicate checks, reference
and type/signature validation, entry/dependency closure, then source-event CFG
liveness and access interpretation. At the interface boundary these new semantic
checks follow the existing parallel-root and owned-JSON semantic checks and
precede the existing interface-hash mismatch check. A malformed access record
never becomes usable merely because its supplied hash matches. All
instruction owners must use this same staged order. In particular, decoding
an out-of-range TypeRef or AbiRef records its scalar value during Wire; it does
not resolve the reference until References. A later malformed tag, scalar,
UTF-8 name or nested canonical type/ABI therefore wins over that earlier bad
reference. A complete structurally valid record with trailing input fails
Consumption before duplicate or reference checks. Reserved source tags fail
Wire as soon as they are encountered; no native payload is guessed or skipped.

Within each semantic stage, traverse the listed top-level tables, their records,
fields, functions, blocks and instructions in wire order. References validates
bounds, required type kind, derived operand/result types and signature agreement
at each field before advancing to the next field; Closure subsequently checks
entry reachability, local/private coverage and selected dependency closure.
SourceEvents then checks typed snapshot definitions and CFG liveness in the
specified program/block/site order. Never choose a failure from hash-map order
or from whichever independent validation worker finishes first.

Six full-record controls reuse the complete 93-byte unit bundle below. Mutations
preserve all unrelated fields, so the paired defect is reached only if its
preceding defect is repaired:

| Mutations | Required first error |
| --- | --- |
| Function ABI 99, entry program 99 and terminator tag 255 | Wire at program 0/block 0/terminator. |
| Function ABI 99, entry program 99 and embedded NUL in the entry name | Wire at entry 0/name. |
| Duplicate ABI, function ABI 99, entry program 99 and trailing byte | Consumption at the bundle. |
| Duplicate ABI, function ABI 99 and entry program 99 | CanonicalOrder at ABI 1. |
| Function ABI 99 and entry program 99 | References at program 0/abi. |
| Entry program 99 only | References at entry 0/program. |

The independently staged restricted reader passes all six controls and rejects
all 93 proper prefixes at Wire. This closes structural/consumption/order/reference
precedence for a complete body; operation-specific type/signature conflicts and
Closure/SourceEvents combinations remain acceptance cells of their corresponding
owners. The earlier restricted golden readers validate their individual record
shapes; they are not implementations of this complete staged error policy.

### Operand and terminator wire records

Every tagged record below starts with its listed `u8` tag; fields follow in the
listed order. `u32`, sequences and byte strings follow the envelope rules.
`optional<T>` is tag `0` without payload or tag `1` followed by `T`; every other
tag rejects. A Boolean is exactly byte `0` or `1`. Table ids refer to the current
bundle; slot/value/argument/block/source-snapshot ids refer to the current function.
Nested operands are decoded with a worklist, not recursive descent on attacker
controlled depth.

| AccessOperand tag | Payload |
| --- | --- |
| 0 Constant | `AccessConstant` |
| 1 Value | `value: u32` |
| 2 Argument | `parameter: u32` |
| 3 BorrowedPlace | `AccessBorrowedPlace` |
| 4 BorrowedElementPlace | `base: AccessBorrowedPlace`, `index: AccessOperand`, `element_type: u32`, `field_path: sequence<u32>`, `length: AccessOperand` |
| 5 BorrowedFixedElementPlace | `base: u32`, `index: u32`, `field_path: sequence<u32>`, `requested_type: u32`, `cleanup: optional<u32>` |
| 6 BorrowedCleanupArgument | `parameter: u32` |

`AccessBorrowedPlace` is `slot: u32`, `path: sequence<AccessBorrowedSegment>`,
`requested_type: u32`, `cleanup: optional<u32>`, in that order. Segment tags are
`0 StructField(field:u32)`, `1 EnumPayload(variant:u32,payload:u32)`,
`2 OptionSome`, `3 ResultOk`, `4 ResultErr`. RootSlot is not a wire segment: the
slot field already names the root. Derivation first validates the MIR operand;
it then removes the one optional leading RootSlot admitted for a dynamic-element
base. An interior RootSlot or a RootSlot in an otherwise forbidden MIR path
remains malformed and cannot be normalized into an accepted operand.

The stored leaf and requested type are both checked using the canonical graph.
Preserve the admitted view-retype relation, exact field path, physical parameter
modes and length/index types. A cleanup id must select the required Boolean
companion and cannot grant ownership or writability. An imported call cannot
qualify a mismatched place merely by naming a desired view type. Reservation
identity and successful bounds-edge proof remain actual-MIR validation
prerequisites; they are projected out as specified above.

| AccessConstant tag | Payload and validation |
| --- | --- |
| 0 Integer | `type: u32`, followed by 16 little-endian two's-complement bytes for the MIR `i128` value. Referenced type and admitted value range follow the existing MIR constant owner. |
| 1 Float | `type: u32`, followed by the eight little-endian bytes of the MIR `f64` bit pattern. Preserve signed zero and NaN bits; the declared floating type determines the existing scalar conversion semantics. |
| 2 Character | `value: u32`, validated as the existing MIR character scalar. |
| 3 Boolean | One Boolean byte. |
| 4 Unit | No payload. |

Constants contain scalar values only. Text/static aggregate bytes belong to their
producer instruction and retain the exact byte/element order required by that
instruction; they cannot be smuggled into a scalar `Constant` record. Access
interpretation maps concrete scalar constants into its finite scalar domain only
after decoding and structural validation, without changing the canonical bytes.

| AccessTerminator tag | Payload |
| --- | --- |
| 0 Goto | `target: u32` |
| 1 Branch | `condition: AccessOperand`, `then: u32`, `else: u32` |
| 2 Return | `value: optional<AccessOperand>` |
| 3 ReturnWithCleanup | `value: AccessOperand`, `cleanup: AccessOperand` |
| 4 Unreachable | No payload. |

There is exactly one terminator per block. Branch condition type, target bounds,
return type, return-cleanup shape, value availability and producer foundations
retain their existing validated-MIR requirements. Then and else remain ordered;
equal targets are permitted and do not invent two different side effects.
Unreachable has no normal return. A nonreturning called program likewise produces
no continuation even when an enclosing block has a syntactic terminator.

### Instruction statement wire records

The access body retains the ordered typed MIR operations needed by abstract
interpretation, including scalar/control producers and lifecycle prerequisites.
It contains no executable machine code. The outer statement tags are fixed below;
`AccessRvalue` uses the baseline variant inventory below, with native StaticData/ColumnBatch operations excluded
from the source phase and the semantic descriptor/bridge records specified below.
Their compound payload owners remain required. Type fields always
reference a `CanonicalTy` table entry. In particular a `record_type` replaces a
process-local MIR `struct_id` and must decode to the required nominal struct.

| AccessInstruction tag | Payload in wire order |
| --- | --- |
| 0 Let | `value:u32`, `rvalue:AccessRvalue` |
| 1 Store | `slot:u32`, `value:AccessOperand` |
| 2 StoreField | `slot:u32`, `path:sequence<u32>`, `value:AccessOperand` |
| 3 StoreIndex | `slot:u32`, `index:AccessOperand`, `value:AccessOperand` |
| 4 StoreConstArray | `slot:u32`, `element_type:u32`, `elements:sequence<AccessConstElement>` |
| 5 PtrStore | `pointer:AccessOperand`, `index:AccessOperand`, `value:AccessOperand` |
| 6 (reserved; reject) | PtrStoreNoalias projects to tag 5; no native scope id is serialized. |
| 7 VecStore | `slice:AccessOperand`, `index:AccessOperand`, `value:AccessOperand`, `element_type:u32`, `lanes:u32` |
| 8 StoreElemField | `slot:u32`, `index:AccessOperand`, `path:sequence<u32>`, `value:AccessOperand` |
| 9 StoreElemFieldPtr | `base:AccessOperand`, `index:AccessOperand`, `path:sequence<u32>`, `record_type:u32`, `value:AccessOperand` |
| 10 StoreColumn | `base:AccessOperand`, `length:AccessOperand`, `index:AccessOperand`, `field:u32`, `record_type:u32`, `value:AccessOperand` |
| 11 ArenaEnd | `arena:AccessOperand` |
| 12 RawFree | `pointer:AccessOperand` |
| 13 ColumnBatchFinish (reserved; reject) | Native materialization only; no public source-access payload. |
| 14 ColumnBatchDrop (reserved; reject) | Native materialization only; no public source-access payload. |
| 15 RawStore | `pointer:AccessOperand`, `offset:AccessOperand`, `value:AccessOperand` |
| 16 TgWait | `group:AccessOperand` |
| 17 TgEnd | `group:AccessOperand` |
| 18 DropFlagInit | `slot:u32` |
| 19 NullTupleField | `slot:u32`, `field:u32` |
| 20 NullStructField | `slot:u32`, `field:u32` |
| 21 NullElemField | `slot:u32`, `index:AccessOperand`, `path:sequence<u32>` |
| 22 Drop | `slot:u32` |
| 23 DropElem | `slot:u32`, `index:AccessOperand`, `record_type:u32` |
| 24 DropElemField | `slot:u32`, `index:AccessOperand`, `path:sequence<u32>` |
| 25 (reserved; reject) | Native BorrowedElementReservation is validated and omitted. |
| 26 DropValue | `value:AccessOperand` |
| 27 SourceEvent | `event:AccessSourceEvent` |

`AccessConstElement` uses tags `0 Integer` with 16 little-endian two's-complement
bytes, `1 Float` with eight little-endian MIR `f64` bits, `2 Character` with a
`u32`, `3 Boolean` with one Boolean byte, or `4 Text` with a byte string. Unlike
logical names, text data may contain embedded NUL and must preserve it. Text is
validated UTF-8; numeric/Boolean/character element kind and range must agree with
the declared element type. The sequence count must equal the fixed destination
extent. Initializer text leaves retain read-only byte publication; the copied
fixed-array slot itself is writable. The corresponding `ConstArray` Rvalue has
read-only backing and cannot reuse the StoreConstArray transfer.

SourceEvent's tag is 27 regardless of the Rust enum's declaration position.
New Rust variants or secondary fields must fail the closed inventory owner until
the unified schema, transfer and interface version are deliberately updated.
No compiler `Debug` representation, Rust discriminant or Rust struct field layout
is a canonical serialization contract. The statement tags above are not inferred
at runtime from enum order.

### Direct targets and explicit foreign calls

MIR's `DirectCall` has Program and Runtime alternatives, but a Program logical
name can select either an Align body/import or a declared `extern "C"` function.
Resolve that distinction against the validated declaration tables before deriving
an access record. Do not confuse an explicit foreign declaration with an
unavailable safe callback, or require a nonexistent Align body for it.

`AccessDirectTarget` has `u8` tags `0 Program(program:u32)`,
`1 Runtime(key:u8)`, `2 Foreign(name:byte_string,abi:u32)`, or
`3 Imported(import:u32)`. Program references a local concrete Align access
function; Imported selects an ordinary function entry through the import table.
A source call cannot select a resource Drop entry. Foreign names are nonempty UTF-8
without NUL, and the ABI references a complete validated `CanonicalFnAbi` record
matching the actual foreign declaration. It remains a C call under the existing
extern ABI/unsafe source contract, not an Align closure with an environment.
This record does not create a source-callable export, mark an opaque result
writable, or trust a local summary in place of a body for a safe Align function.

`FnAddr`, direct calls with a cleanup result, SQLite callback descriptors and
parallel terminal/stage targets also use `AccessAlignTarget`. Each imported
alternative must select an ordinary entry and satisfy that operation's existing
physical/source ABI, capture, effect and cleanup checks. The current parallel
callable validator accepts stored and imported Align declarations and rejects
extern declarations; retaining only a local ProgramRef would lose this admitted
case. `Closure.lifted` remains a local ProgramRef under its existing stored-body
and explicit-capture contract. These target encodings do not widen any callable
or parameter-mode admission rule.

Runtime keys are `0 Print`, `1 PrintStr`, `2 PrintBool`, `3 PrintChar`,
`4 PrintF32`, `5 PrintF64`, `6 Hash64`, `7 Hash128`, `8 ProcessExit`,
`9 ProcessAbort`, `10 DivFail`, `11 BoundsFail`, `12 Utf8BoundaryFail`,
`13 LenMismatchFail`, `14 RangeFail`. This is the direct-call whitelist already
specified below, not an ordinal from the much larger Rust `RuntimeKey` enum.
Keys 0–7 consume inputs and return no view-bearing value; keys 8–14 have no normal
return. Signature and source-use requirements are still validated for each key.

### Unsafe proof boundary

Source qualification proves the specified safe access operations conditional on
the program satisfying draft §15's existing unsafe obligations. It does not
prove arbitrary C implementations or raw pointer operations. The same boundary
is explicit in plan 52's write contract and plan 57's unsafe-write row.

For Foreign, RawStore and RawFree, evaluate arguments and retain their source
Use/completion checks in order. The access transfer then leaves tracked safe
descriptor/observation facts unchanged *under that precondition*. This is not
a claim that native memory is unchanged or that the operation is read-only.
The unsafe author must preserve every safe value used afterward: backing
lifetime, no-alias/ownership invariants, descriptor validity and validated text/
codec invariants. A raw or hidden foreign write that invalidates a retained
validated view violates that obligation; accepting its syntax is not a clean
proof of the unsafe code. A foreign operation may validly mutate unrelated
storage or preserve the view's invariants without ending that view.

A compiler certificate, interface body or closed entry always has this same
conditional meaning. No view result is admitted by the existing extern result
grammar. An opaque raw result grants no new safe writability; ResourceViewFromRaw
publishes Unavailable slice authority or validated text under its existing
unsafe construction preconditions. No module-name whitelist supplies authority.

Ordinary indexed stores, explicit mutable arguments and typed native operations
remain their specified checked transfers, even inside an unsafe block. Their
known writes end overlapping observations. Compiler-owned DbBridgeCall is such
a typed source operation; its retained witness gives its source effect, while
the package wrapper retains the existing pointer/plan validity obligation.

Ending every observation at an arbitrary foreign call would reject valid
read-only foreign use and still would not prove arbitrary native memory
corruption safe. A new authenticated foreign-effect annotation system is outside
this repair. The implementation owner pairs a valid foreign reader with ordinary
source-safe overlapping writes inside/outside unsafe, and preserves the existing
raw-construction admission controls; it must not execute invalid raw witnesses.

`RawCall` is a separate closed compiler producer in current checked HIR. There
is no general source raw-call operation: sema synthesizes the descriptor,
prepared-statement, current-row and batch bridges, and checked-HIR validation
checks their exact signature, resource relation and required guard. Preserve
`lower_raw_call` evaluation order: evaluate the guard first; its false branch
aborts before evaluating the callee or any argument; on success evaluate the
callee, then each argument in order, then perform the call. Source completion
events must follow this order. A role cannot be modeled as unconditional merely
because its generated thunk has a complete signature.

The batch-plan guard is structural, not pointer provenance. Its existing body
checks version, flags, reserved fields, required non-null callback slots and
the optional SoA slot's flag/count relation. It does not compare callback
addresses with the producer's generated bodies or identify P/R. Thus a true
`batch_plan_valid` result alone cannot turn an arbitrary raw pointer into a
qualified safe callback or a particular semantic descriptor role. Close the
producer-to-operation relation described above before deriving its transfer;
do not treat the guard as that missing relation. Ordinary explicit foreign
calls and these synthesized typed bridges keep distinct dispatch records.

### Source-subject and source-event wire records

`AccessSubject` has tag `0 Value(value:u32)` or tag
`1 Place(slot:u32,path:sequence<AccessProjection>)`. A Value selects the profile
already completed at its definition; a Place selects current slot contents.
The type of every step is derived from the slot/value type record. Subjects do
not carry a requested type or retype bit: a source view conversion must produce
its ordinary typed value, and its input source-use event checks the physical
selected input before that conversion. This prevents a requested byte type from
hiding the source text's immediate validation obligation.

| AccessProjection tag | Payload | Selection |
| --- | --- | --- |
| 0 StructField | `field:u32` | Exact nominal record field. |
| 1 TupleElement | `element:u32` | Exact tuple position. |
| 2 EnumPayload | `variant:u32`, `payload:u32` | Payload of the named active variant. |
| 3 OptionSome | None | Active Some payload. |
| 4 ResultOk | None | Active Ok payload. |
| 5 ResultErr | None | Active Err payload. |
| 6 FixedElement | `index:u32` | One in-range element of a fixed inline array. |
| 7 DynamicElement | `index:AccessOperand` | Addressed element selected by an integer operand; point-state alias/index facts determine its conservative content alternatives. |

Every step must be admitted by its derived predecessor type; inactive tagged
payloads or an unproved array range cannot be named as an available subject.
A subject used to check the whole receiver before index evaluation has an empty
or enclosing-record path, not a DynamicElement with an index that has not yet
completed. DynamicElement is an analysis selection only: it does not create a
source-addressable Move element, native borrowed pointer, extra load or new write
surface. Existing source/MIR admission remains the prerequisite. Profiles and
role languages represent selections compactly; encoding a path writes the one
explicit step sequence rather than enumerating all sibling leaf paths.

A source-event instruction carries one `AccessSourceEvent`. Event tags and fields
are `0 Use(subject)`, `1 CaptureLive(snapshot:u32,subjects:sequence<AccessSubject>)`,
`2 Rebind(snapshot:u32,inputs:sequence<u32>)`, `3 Check(snapshots:sequence<u32>)`,
and `4 Release(snapshots:sequence<u32>)`. The enclosing SourceEvent instruction has tag 27 as fixed above; nested
event tags do not share its tag space. Snapshot ordering, liveness, branch consolidation and release
follow the logical record above. Subject-list duplicate rejection compares fully
encoded canonical subjects, without sorting away their written evaluation order.

For structural validation, first decode the event and every operand/path tag and
scalar field in wire order; then check ordinal/table bounds, duplicate/order
rules, predecessor/result types and value availability, followed by CFG snapshot
liveness. Access validity is interpreted only after those checks. The identity
of an observation is never serialized in an event: the interpreter derives it
from the instantiated subject profile at the event's program point.

The independent author record checks below have this cumulative coverage. They
are restricted semantic/byte models, not the implemented general decoder.

| Complete record | Bytes | Closed record property |
| --- | ---: | --- |
| Empty bundle | 24 | Six empty sequences. |
| Unit-return entry | 93 | Entry/body/ABI envelope. |
| Empty live snapshot | 123 | Capture, Check and Release with an active empty observation set. |
| Imported direct call | 147 | Selected ordinary import identity and ABI. |
| Fixed-array source subject | 188 | Typed element projection and nonempty source-subject records. |
| Local mutual recursion | 177 | Rooted private program numbering and recursive target references. |
| Capture-free function value | 212 | Canonical Fn type, FnAddr and indirect invocation ABI. |
| Captured Bool closure | 279 | Local lifted target, captured argument and distinct closure/lifted signatures. |
| Captured text return | 399 | Capture-relative borrow/region roots translated to the lifted physical parameter, with source Uses. |
| Recursive callable type | 328 | Finite Node/Fn backreferences, self function address and exact initialized field. |
| Type-erased resource Drop entry | 208 | Generic declaration-owned support entry and imported hook forwarding. |
| Concrete resource Drop binding | 529 | Projected resource type/ABI, actual Drop action and used support binding. |
| Direct owned-string call | 233 | Dynamic cleanup ABI and its atomic Boolean result companion. |
| Semantic Command constructor | 251 | Public descriptor witness, effective options and constructor result; no native artifact payload. |
| Guarded prepared-count bridge | 458 | Guard import, success-only callee load, typed role/signature and abort path. |
| Rows-tied stream decode | 864 | Concrete Row/resource/ref types, typed Rows witness and return roots selecting the second parameter. |
| Indirect owned return | 288 | FnAddr and Fn ABI DynamicBit agree with the indirect result and atomic Boolean cleanup companion. |
| Nested active-arm Drop | 426 | Holder field and Option payload select owned String cleanup behind the returned DynamicBit. |
| Query with nondefault options | 313 | Query P/R witness, SQLite restriction and exact effective check/version option bytes. |

The codec owner extends these records with parameterized captured-aggregate,
recursive-callable, descriptor-operation and operation-specific multi-invalid
controls. Reuse these independently checked full records to anchor that coverage;
each Cartesian cell does not require a separate hand-written full hex vector. Source-observation and
interpreter-state tests additionally cover behavior that is not serialized as
an observation identifier.

The first nonempty full-record golden is an ordinary exported entry named
`sample.f` selecting local program 0, with no parameters, slots, values or
snapshots, returning unit from block 0. Its standalone type, import and Drop
binding tables are empty. The ABI table contains the existing 14-byte canonical
zero-argument unit ABI. One block contains zero instructions and `Return(None)`.
Its bundle is exactly 93 bytes, grouped by record fields below (whitespace is
not encoded):

```text
00000000
01000000 0e000000 01 00000000 03 00000000 38 00 00 00
00000000
01000000
00000000 00000000 00000000 00000000 00000000 00000000
01000000 00000000 02 00
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

The outer interface field prefixes those bytes with `5d000000`. An independent
author encoder and parser agree with this fixed vector in both directions; every
one of its 93 proper prefixes rejects as truncated and an added trailing byte
rejects. This fixture closes the empty/unit-return envelope only; the subsequent
fixtures supply their separate projection/call/Drop coverage, and the cumulative
table records the distinct covered wire axes.

A source-event full-record golden extends that same function with one declared
snapshot and three instructions: `CaptureLive(0, [])`, `Check([0])`, then
`Release([0])`. The capture has an active empty obligation set, exercising the
distinction between an empty completion and an absent snapshot. Its bundle is
123 bytes; the outer field length is `7b000000`:

```text
00000000
01000000 0e000000 01 00000000 03 00000000 38 00 00 00
00000000
01000000
00000000 00000000 00000000 00000000 01000000 00000000
01000000 03000000
1b 01 00000000 00000000
1b 03 01000000 00000000
1b 04 01000000 00000000
02 00
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

The independent author encoder/parser checks this exact semantic/byte pair and
all 123 proper truncated prefixes. The snapshot-CFG model separately checks empty
completion liveness and invalid missing/repeated ordinals. These models cover
distinct invariants; the final codec owner must combine byte parsing, typed
subjects and liveness, including multi-invalid records and nonempty captures.

The imported-call golden is `sample.f() -> ()` with one instruction defining
unit value 0 by calling imported ordinary entry `dep.tick() -> ()`, then returning
unit. Its one standalone type is the existing six-byte canonical unit record;
the function and import share ABI 0. It has no slots, parameters, snapshots or
Drop bindings. The bundle is 147 bytes, prefixed by `93000000` in the outer field:

```text
01000000 06000000 03 00000000 38
01000000 0e000000 01 00000000 03 00000000 38 00 00 00
01000000 03000000 646570 00 08000000 6465702e7469636b 00000000
01000000
00000000 00000000 00000000 01000000 00000000 00000000 00000000
01000000 01000000
00 00000000 1a00 03 00000000 00000000
02 00
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

An independently written restricted encoder/parser checks that semantic/byte
pair in both directions and rejects all 147 proper prefixes and a trailing byte.
Separate resolution cases admit the selected dependency entry and reject a
missing unit, missing entry, Drop-entry substitution, mismatched ABI and two
conflicting selected records for one unit. This is one direct imported call,
not full import-graph validation. The following vectors close their own typed
subject, local recursion, callable and resource Drop records. They do not replace
the remaining captured/cleanup-call and malformed multi-invalid record owners.

A projected-subject full-record golden uses one ByValue `array<bool, 2>`
parameter in slot 0 and a unit result. It executes Use of fixed element 1,
CaptureLive of that same subject into snapshot 0, Check and Release, then returns
unit. The active snapshot is valid even though the Bool subject contributes no
byte observation. The exact bundle is 188 bytes (outer field length `bc000000`):

```text
01000000 0b000000 03 00000000 08 02 02000000
01000000 1a000000 01 01000000 00 03 00000000 08 02 02000000 03 00000000 38 00 00 00
00000000
01000000
00000000 01000000 00000000 01000000 00000000 00000000 01000000 00000000
01000000 04000000
1b 00 01 00000000 01000000 06 01000000
1b 01 00000000 01000000 01 00000000 01000000 06 01000000
1b 03 01000000 00000000
1b 04 01000000 00000000
02 00
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

The fixed-array type bytes reuse the canonical type codec's Bool-array vector;
the ABI uses its existing ByValue tag and nested type grammar. A restricted
independent encoder/parser agrees with the stated semantic/byte pair, rejects
all 188 proper prefixes and trailing input, and rejects fixed indices 2 and
u32::MAX against the two-element type. This closes one nonempty typed subject,
not the general projected-observer/type/path Cartesian product. The later
callable and resource records have separate coverage; captured recursive
callbacks, nested cleanup and multi-invalid precedence remain required.

A local recursive closure golden has two zero-argument unit functions: exported
`sample.f` calls a private function, which calls the entry function. Both initial
blocks contain a syntactic unit return after their call. Rooted discovery assigns
the entry ordinal 0 and the private target ordinal 1; the back edge references
0 rather than recursively embedding a body. The bundle is 177 bytes (outer field
length `b1000000`):

```text
01000000 06000000 03 00000000 38
01000000 0e000000 01 00000000 03 00000000 38 00 00 00
00000000
02000000
00000000 00000000 00000000 01000000 00000000 00000000 00000000
01000000 01000000
00 00000000 1a00 00 01000000 00000000
02 00
00000000 00000000 00000000 01000000 00000000 00000000 00000000
01000000 01000000
00 00000000 1a00 00 00000000 00000000
02 00
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

An independent restricted encoder and reader agree with this semantic/byte pair.
Reversing the producer's function storage order, renaming the private logical
target and adding an unreachable helper preserve these bytes. All 177 proper
prefixes and trailing input reject. Changing the first target to 2 rejects its
out-of-range reference; changing it to 0 leaves program 1 unreachable and rejects
the noncanonical bundle. This closes local recursive target encoding and rooted
discovery, not recursive callback/capture ABI or interpreter fixed-point behavior.
In particular, the syntactic return after an unconditional recursive call cannot
seed an interpreter return before that call actually returns.

A separate four-case interpreter control closes that last property for the
existing author call model. Direct and first-class-function versions of the
mutual cycle both reach a fixed point with no normal return and do not inspect
the caller's later read-only write. Adding one concrete nondeterministic base
return to either version makes that continuation reachable and rejects the
write. Each run exhausts its worklist; no provisional return or early clean
verdict is used. These are bounded controls for founded recursion returns, not
the remaining deep-profile/callback-capture tabulation proof.

A capture-free callable golden constructs a function value targeting private
program 1, invokes it with no arguments, and returns unit. Its canonical function
type is the 19-byte graph-v3 record
`03 01000000 04 00000000 38 00 00 00 34 00000000`: one Fn node with no
parameters, unit return, no return roots/cleanup, followed by the root Fn reference.
The unit type sorts before it. Both construction and invocation select the same
zero-argument unit ABI. The full bundle is 212 bytes (outer length `d4000000`):

```text
02000000
06000000 03 00000000 38
13000000 03 01000000 04 00000000 38 00 00 00 34 00000000
01000000 0e000000 01 00000000 03 00000000 38 00 00 00
00000000
02000000
00000000 00000000 00000000 02000000 01000000 00000000 00000000 00000000
01000000 02000000
00 00000000 6900 00 01000000 00000000
00 01000000 1b00 01 00000000 00000000 00000000 00000000 00000000
02 00
00000000 00000000 00000000 00000000 00000000 00000000
01000000 00000000 02 00
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

An independent restricted encoder assembles the function graph, FnAddr and
CallIndirect from their semantic fields; a separate reader agrees with the stated
bytes and selected target/signature. All 212 proper prefixes, trailing input and
a substitution of the callable value's type with Unit reject. This is a complete
no-capture function-value record. Captured environments, recursive function types,
cleanup-return calls and cross-bundle callback construction still need their own
full-record vectors and semantic checks.

A type-erased resource Drop-entry golden exports
`__align_resource_drop$u$R` and forwards its one ByValue raw argument to the
ordinary imported hook `u.internal$drop` in selected unit `u.internal`. The
declaring interface describes generic resource `R<T>`; no concrete resource
instance or cleanup-plan binding is needed to publish its declaration-owned
Drop entry. Raw and Unit are the only standalone types. The raw-to-unit ABI has
no return roots or cleanup. The bundle is 208 bytes (outer length `d0000000`):

```text
02000000 06000000 03 00000000 15 06000000 03 00000000 38
01000000 15000000 01 01000000 00 03 00000000 15 03 00000000 38 00 00 00
01000000 0a000000 752e696e7465726e616c 00 0f000000 752e696e7465726e616c2464726f70 00000000
01000000
00000000 01000000 00000000 01000000 00000000 01000000 01000000 00000000 00000000
01000000 01000000
00 00000000 1a00 03 00000000 01000000 02 00000000
02 00
01000000 01 19000000 5f5f616c69676e5f7265736f757263655f64726f7024752452 00000000
00000000
```

The independent restricted encoder/reader agree on the selected hook, exported
Drop key and argument forwarding. All 208 proper prefixes and trailing input
reject. Entry qualification rejects a different declared resource thunk, absent
hook, wrong imported entry kind or ABI, and an `internal_evil` prefix lookalike
outside the declaring unit's `internal` subtree. This closes type-erased Drop
entry publication through a selected dependency. The following concrete
counterpart closes a used leaf-resource Drop binding and actual source action.

The concrete counterpart exports `u.consume(value: u.R) -> ()`, whose first
program drops its owned parameter slot. Entry 1 is the same type-erased support
thunk forwarding to the imported hook. Resource `u.R` is non-generic here; its
112-byte canonical resource record has source/name `u$R`, declaring unit `u`,
representation 1, the existing 16-byte Drop fingerprint and zero generic arity.
Both hook and thunk fields contain the public thunk under the access-only
projection. Raw, Unit and Resource are standalone TypeRefs 0, 1 and 2. The
raw-to-unit ABI sorts before the 127-byte resource-to-unit ABI. The used
AccessDropBinding maps resource 2 to local support program 1. This full bundle is
529 bytes (outer length `11020000`):

```text
03000000
06000000 03 00000000 15
06000000 03 00000000 38
70000000
03 01000000 05
03000000 752452
03000000 752452
01000000 75
19000000 5f5f616c69676e5f7265736f757263655f64726f7024752452
19000000 5f5f616c69676e5f7265736f757263655f64726f7024752452
01000000 616c69676e2d7265732d64726f702d31 00000000
39 00000000
02000000
15000000 01 01000000 00 03 00000000 15 03 00000000 38 00 00 00
7f000000 01 01000000 00
03 01000000 05
03000000 752452
03000000 752452
01000000 75
19000000 5f5f616c69676e5f7265736f757263655f64726f7024752452
19000000 5f5f616c69676e5f7265736f757263655f64726f7024752452
01000000 616c69676e2d7265732d64726f702d31 00000000
39 00000000
03 00000000 38 00 00 00
01000000 0a000000 752e696e7465726e616c 00 0f000000 752e696e7465726e616c2464726f70 00000000
02000000
01000000 01000000 00000000 01000000 02000000 00000000 00000000 00000000
01000000 01000000 16 00000000 02 00
00000000 01000000 00000000 01000000 00000000 01000000 01000000 00000000 00000000
01000000 01000000
00 00000000 1a00 03 00000000 01000000 02 00000000
02 00
02000000
00 09000000 752e636f6e73756d65 00000000
01 19000000 5f5f616c69676e5f7265736f757263655f64726f7024752452 01000000
01000000 02000000 00 01000000
```

The restricted independent encoder/reader agree on resource metadata, both ABI
records, the source Drop, support forwarding and the used binding. All 529 proper
prefixes and trailing input reject. Substituting Raw for the binding's resource
TypeRef or selecting ordinary program 0 instead of the support target rejects.
This closes a concrete leaf-resource Drop record. The later nested Drop and
indirect cleanup-return records cover those distinct full-record shapes; staged
malformed-record precedence has its own controls. The projected resource graph is access metadata and must
never be handed to native resource lowering as an actual hook definition.

An owned-string call golden gives both functions the zero-argument string-return
ABI with `DynamicBit` cleanup. Program 1 constructs literal `"x"`, clones it to
an owned string and returns the string with Boolean true. Program 0 receives the
value and its atomic cleanup companion through CallWithCleanup and forwards
that exact pair through ReturnWithCleanup. Bool, Str and String are standalone
TypeRefs 0, 1 and 2. The bundle is 233 bytes (outer length `e9000000`):

```text
03000000
06000000 03 00000000 02
06000000 03 00000000 12
06000000 03 00000000 13
01000000 0e000000 01 00000000 03 00000000 13 00 00 01
00000000
02000000
00000000 00000000 00000000 02000000 02000000 00000000 00000000 00000000
01000000 01000000
00 00000000 1d00 00 01000000 00000000 01000000
03 01 00000000 01 01000000
00000000 00000000 00000000 02000000 01000000 02000000 00000000 00000000
01000000 02000000
00 00000000 1e01 01000000 78
00 01000000 1b01 01 00000000
03 01 01000000 00 03 01
01000000 00 08000000 73616d706c652e66 00000000
00000000
```

The independent restricted encoder/reader agree on the owned result, Boolean
companion, literal bytes and forwarded return pair. All 233 proper prefixes and
trailing input reject. Naming the returned String value as its cleanup companion
rejects the value/type relation; a cleanup constant byte of 2 rejects Boolean
encoding. This is record and companion-identity coverage. It does not prove
arbitrary native allocation/cleanup parity merely because a supplied cleanup bit
has Boolean type; original ownership validation and trusted finalization remain
separate requirements. Indirect cleanup calls and captured environments remain
separate full-record obligations.

The first complete semantic descriptor record returns `command<u.P>` from
`u.cmd`. Its Params struct is empty and its descriptor has the existing private
Raw field. P and the descriptor are TypeRefs 0 and 1; the zero-argument ABI
returns that descriptor with no cleanup. The constructor uses Command witness,
AnySupportedDriver and the effective DeclaredOnly option; its private descriptor
id is absent from that payload. The existing public entry key remains u.cmd.
The bundle is 251 bytes (outer length `fb000000`):

```text
02 00 00 00 18 00 00 00 03 01 00 00 00 00 03 00 00 00 75 24 50 00 00 00
00 00 00 32 00 00 00 00 33 00 00 00 03 01 00 00 00 00 12 00 00 00 70 6b
67 2e 64 62 24 63 6f 6d 6d 61 6e 64 24 75 24 50 00 00 01 00 00 00 07 00
00 00 24 73 74 61 74 69 63 15 32 00 00 00 00 01 00 00 00 3b 00 00 00 01
00 00 00 00 03 01 00 00 00 00 12 00 00 00 70 6b 67 2e 64 62 24 63 6f 6d
6d 61 6e 64 24 75 24 50 00 00 01 00 00 00 07 00 00 00 24 73 74 61 74 69
63 15 32 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 01 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 01 00
00 00 01 00 00 00 00 00 00 00 00 4f 01 01 01 00 00 00 00 00 00 00 00 01
00 00 00 00 00 00 02 01 01 00 00 00 00 01 00 00 00 00 05 00 00 00 75 2e
63 6d 64 00 00 00 00 00 00 00 00
```

The independent restricted encoder and reader agree on the two canonical struct
records, complete ABI, semantic constructor and entry. All nine driver/check
policy combinations pass. All 251 proper prefixes, trailing input, unknown
driver/policy bytes and a raw-payload constructor witness reject. This record
contains no SQL, source path, native pointer or artifact payload. It does not
substitute for source sealing, nominal generic-application formation, general
option validation or native artifact qualification. The guarded bridge and Query
constructor with nondefault options have their own complete records below.

The first complete bridge record is the raw prepared-parameter count. TypeRefs
0–4 are u32, i64, Bool, Raw and Unit. Its ABI table contains `() -> u32`,
`(raw) -> u32` and `(raw) -> bool`, in canonical order. Block 0 calls the selected
statement-header guard and branches. Block 1 loads offset 96 only on success,
invokes DbBridgeCall/ParameterCount/RawPreparedCount with the zero-argument
count ABI, and returns u32. Block 2 calls ProcessAbort and is unreachable.
The bundle is 458 bytes (outer length `ca010000`):

```text
05 00 00 00 08 00 00 00 03 00 00 00 00 00 00 20 08 00 00 00 03 00 00 00
00 00 01 40 06 00 00 00 03 00 00 00 00 02 06 00 00 00 03 00 00 00 00 15
06 00 00 00 03 00 00 00 00 38 03 00 00 00 10 00 00 00 01 00 00 00 00 03
00 00 00 00 00 00 20 00 00 00 17 00 00 00 01 01 00 00 00 00 03 00 00 00
00 15 03 00 00 00 00 00 00 20 00 00 00 15 00 00 00 01 01 00 00 00 00 03
00 00 00 00 15 03 00 00 00 00 02 00 00 00 01 00 00 00 18 00 00 00 70 6b
67 2e 64 62 2e 69 6e 74 65 72 6e 61 6c 2e 72 65 73 6f 75 72 63 65 00 30
00 00 00 70 6b 67 2e 64 62 2e 69 6e 74 65 72 6e 61 6c 2e 72 65 73 6f 75
72 63 65 24 73 74 6d 74 5f 68 65 61 64 65 72 5f 73 68 61 70 65 5f 76 61
6c 69 64 02 00 00 00 01 00 00 00 01 00 00 00 01 00 00 00 00 00 00 00 01
00 00 00 03 00 00 00 04 00 00 00 02 00 00 00 03 00 00 00 00 00 00 00 04
00 00 00 00 00 00 00 00 00 00 00 03 00 00 00 01 00 00 00 00 00 00 00 00
1a 00 03 00 00 00 00 01 00 00 00 02 00 00 00 00 01 01 00 00 00 00 01 00
00 00 02 00 00 00 02 00 00 00 00 01 00 00 00 f1 00 02 00 00 00 00 00 00
01 00 00 00 60 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 02 00 00
00 50 01 01 05 01 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 02 01 01 02 00 00 00 01 00 00 00 00 03 00 00 00 1a 00 01 09 00 00
00 00 04 01 00 00 00 00 1e 00 00 00 70 6b 67 2e 64 62 2e 69 6e 74 65 72
6e 61 6c 24 70 72 65 70 61 72 65 64 5f 63 6f 75 6e 74 00 00 00 00 00 00
00 00
```

The independent restricted encoder/reader agree on guard import/ABI, block/value
numbering, typed integer offset, bridge role/witness and failure termination.
All 458 proper prefixes, trailing input, a resolver-pointer offset substituted
for the count pointer, wrong role/witness and selected guard ABI substituted for
the count ABI reject. This verifies the recorded structure; source sealing and
producer-owned native statement validity remain separate requirements. The
raw count has no P witness and therefore publishes the conservative u32 domain,
not an invented exact Params count.

The first captured-environment record takes a Bool, constructs a zero-argument
closure capturing it, invokes the closure and returns its Bool result. The lifted
local body takes the capture as its trailing parameter and returns that argument.
TypeRefs 0 and 1 are Bool and Fn; ABI 0 is the closure's `() -> bool`, while ABI 1
is the caller/lifted `(bool) -> bool`. The Closure target is a local ProgramRef,
as required by the existing actual-MIR callable owner. The bundle is 279 bytes
(outer length `17010000`):

```text
02 00 00 00 06 00 00 00 03 00 00 00 00 02 13 00 00 00 03 01 00 00 00 04
00 00 00 00 02 00 00 00 34 00 00 00 00 02 00 00 00 0e 00 00 00 01 00 00
00 00 03 00 00 00 00 02 00 00 00 15 00 00 00 01 01 00 00 00 00 03 00 00
00 00 02 03 00 00 00 00 02 00 00 00 00 00 00 00 02 00 00 00 01 00 00 00
01 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 02 00 00 00 01 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 02 00 00 00 00 00 00 00
00 2b 00 01 00 00 00 01 00 00 00 02 00 00 00 00 01 00 00 00 00 00 00 00
00 00 00 00 00 01 00 00 00 1b 00 01 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 02 01 01 01 00 00 00 01 00 00 00 01 00 00 00 00
00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 01
00 00 00 00 00 00 00 02 01 02 00 00 00 00 01 00 00 00 00 08 00 00 00 73
61 6d 70 6c 65 2e 66 00 00 00 00 00 00 00 00
```

The independent restricted encoder/reader agree on capture value/type, local
lifted target and both complete signatures. All 279 proper prefixes, trailing
input, substituting Fn for the Bool capture, selecting the caller as the lifted
target, and substituting the lifted ABI for the zero-argument closure ABI reject.
This closes one actual captured environment, not captured-view return roots,
recursive function-type graphs or dynamic cleanup companions.

A captured-text record additionally returns a borrowed view. A source control
`get := fn { value }` returning a captured `str` passes whole-program and per-unit
checking; its emitted MIR gives the visible zero-argument closure borrow/region
roots `captures: [0]` and the lifted physical `(str) -> str` roots `params: [0]`.
A lambda returning `slice<u8>` is rejected by the existing scalar/Result lambda
return restriction in both modes; this plan does not widen that domain.

In this record TypeRef 0 is str, TypeRef 1 is its zero-argument Fn type. ABI 0 is
the visible capture-rooted closure ABI; ABI 1 is the caller/lifted parameter-rooted
ABI. Four Use events select the captured argument, completed closure, returned
text and lifted argument. There are no active snapshots in this bounded record.
The complete bundle is 399 bytes (outer length `8f010000`):

```text
02 00 00 00 06 00 00 00 03 00 00 00 00 12 2b 00 00 00 03 01 00 00 00 04
00 00 00 00 12 01 00 00 00 00 01 00 00 00 00 00 00 00 01 00 00 00 00 01
00 00 00 00 00 00 00 00 34 00 00 00 00 02 00 00 00 26 00 00 00 01 00 00
00 00 03 00 00 00 00 12 01 00 00 00 00 01 00 00 00 00 00 00 00 01 00 00
00 00 01 00 00 00 00 00 00 00 00 2d 00 00 00 01 01 00 00 00 00 03 00 00
00 00 12 03 00 00 00 00 12 01 01 00 00 00 00 00 00 00 00 00 00 00 01 01
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 02 00 00 00 01 00 00 00
01 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 02 00 00 00 01 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 05 00 00 00 1b 00 00 02
00 00 00 00 00 00 00 00 00 00 00 00 00 2b 00 01 00 00 00 01 00 00 00 02
00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 1b 00 00 01 00 00 00 00
00 00 00 00 00 01 00 00 00 1b 00 01 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 1b 00 00 01 01 00 00 00 00 00 00 00 02 01 01 01
00 00 00 01 00 00 00 01 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 01 00 00 00 1b 00 00 02 00
00 00 00 00 00 00 00 02 01 02 00 00 00 00 01 00 00 00 00 08 00 00 00 73
61 6d 70 6c 65 2e 66 00 00 00 00 00 00 00 00
```

The independently stated golden, encoder and restricted reader agree. All 399
proper prefixes, trailing input, capture type/target/signature substitutions,
capture index 1 for a single-capture environment and a capture-root substitution
in the physical parameter ABI reject. This checks complete record transport and
the capture-to-physical-parameter distinction, not the full source evaluation
recipe or recursive callable graphs. The compile-only source control was not
executed; its actual MIR remains the producer-shape reference for this record.

A recursive callable-type control also passes whole-program and per-unit checks:
`Node { make_node: fn() -> Node }` with a constructor returning
`Node { make_node: node }`. Its emitted MIR stores a self function address in the
record field, loads the completed record and returns it with no cleanup companion.
The type graph is finite even though its function return refers back to Node;
this is not recursive execution of the constructor during value formation.

TypeRef 0 is the Node-rooted canonical graph and TypeRef 1 the Fn-rooted graph.
Each is 56 bytes with two nodes and explicit backreferences; the single
`() -> Node` ABI is 64 bytes. The body initializes field 0 with FnAddr(local 0),
then loads its completed slot and returns that record. The complete bundle is
328 bytes (outer length `48010000`):

```text
02 00 00 00 38 00 00 00 03 02 00 00 00 00 04 00 00 00 4e 6f 64 65 00 00
01 00 00 00 09 00 00 00 6d 61 6b 65 5f 6e 6f 64 65 34 01 00 00 00 04 00
00 00 00 32 00 00 00 00 00 00 00 32 00 00 00 00 38 00 00 00 03 02 00 00
00 04 00 00 00 00 32 01 00 00 00 00 00 00 00 04 00 00 00 4e 6f 64 65 00
00 01 00 00 00 09 00 00 00 6d 61 6b 65 5f 6e 6f 64 65 34 00 00 00 00 34
00 00 00 00 01 00 00 00 40 00 00 00 01 00 00 00 00 03 02 00 00 00 00 04
00 00 00 4e 6f 64 65 00 00 01 00 00 00 09 00 00 00 6d 61 6b 65 5f 6e 6f
64 65 34 01 00 00 00 04 00 00 00 00 32 00 00 00 00 00 00 00 32 00 00 00
00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00
00 00 00 00 02 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
01 00 00 00 03 00 00 00 00 00 00 00 00 69 00 00 00 00 00 00 00 00 00 00
02 00 00 00 00 01 00 00 00 00 00 00 00 01 00 00 00 00 00 01 00 00 00 c8
00 00 00 00 00 02 01 01 01 00 00 00 01 00 00 00 00 0b 00 00 00 73 61 6d
70 6c 65 2e 6e 6f 64 65 00 00 00 00 00 00 00 00
```

The independent encoder and restricted reader agree with the stated golden.
The reader resolves node kinds and references against the complete finite table;
it neither recursively unfolds Fn returns nor requires a referenced node to
precede its use. All 328 proper prefixes, trailing input, out-of-range and
wrong-kind function backreferences, a substituted field index and an absent
function target reject. This closes the Node/Fn recursive record and its actual
producer control; additional nested/parameterized callable combinations remain
in the general canonical-layout owner. The source control was not executed.

The Rows stream-decoder record returns `Row { label: str }` from the
physical `(raw, resource_ref<pkg.db.rows<Row>>) -> Row` ABI. Both parameters
are ByValue; borrow and region roots are parameter 1, with no captures or
cleanup result. Types are i64, raw, Row, the concrete rows resource and its
reference, in canonical byte order. Resource and reference records are each
163 bytes; the Row graph is 34 bytes and the ABI is 237 bytes. The resource
projection retains declaration-owned Drop identity and generic arity one.
Naming the resource as a witness does not require a Drop binding: this body
retains no cleanup plan that uses one.

The body extracts the raw wrapper from argument 1, loads its decoder at byte
offset 48 and invokes StreamDecode (operation 6, Rows witness 3) with arguments
0 and 1. It Uses and returns the completed Row. There is no invented guard:
the source producer supplies `guard: None` for this operation; the surrounding
package control flow and existing unsafe validity rules remain prerequisites.
The complete bundle is 864 bytes (outer length `60030000`):

```text
05 00 00 00 08 00 00 00 03 00 00 00 00 00 01 40 06 00 00 00 03 00 00 00
00 15 22 00 00 00 03 01 00 00 00 00 03 00 00 00 52 6f 77 00 00 01 00 00
00 05 00 00 00 6c 61 62 65 6c 12 32 00 00 00 00 a3 00 00 00 03 01 00 00
00 05 12 00 00 00 70 6b 67 2e 64 62 24 72 6f 77 73 24 53 33 5f 52 6f 77
12 00 00 00 70 6b 67 2e 64 62 24 72 6f 77 73 24 53 33 5f 52 6f 77 06 00
00 00 70 6b 67 2e 64 62 21 00 00 00 5f 5f 61 6c 69 67 6e 5f 72 65 73 6f
75 72 63 65 5f 64 72 6f 70 24 70 6b 67 2e 64 62 24 72 6f 77 73 21 00 00
00 5f 5f 61 6c 69 67 6e 5f 72 65 73 6f 75 72 63 65 5f 64 72 6f 70 24 70
6b 67 2e 64 62 24 72 6f 77 73 01 00 00 00 61 6c 69 67 6e 2d 72 65 73 2d
64 72 6f 70 2d 31 01 00 00 00 39 00 00 00 00 a3 00 00 00 03 01 00 00 00
05 12 00 00 00 70 6b 67 2e 64 62 24 72 6f 77 73 24 53 33 5f 52 6f 77 12
00 00 00 70 6b 67 2e 64 62 24 72 6f 77 73 24 53 33 5f 52 6f 77 06 00 00
00 70 6b 67 2e 64 62 21 00 00 00 5f 5f 61 6c 69 67 6e 5f 72 65 73 6f 75
72 63 65 5f 64 72 6f 70 24 70 6b 67 2e 64 62 24 72 6f 77 73 21 00 00 00
5f 5f 61 6c 69 67 6e 5f 72 65 73 6f 75 72 63 65 5f 64 72 6f 70 24 70 6b
67 2e 64 62 24 72 6f 77 73 01 00 00 00 61 6c 69 67 6e 2d 72 65 73 2d 64
72 6f 70 2d 31 01 00 00 00 3a 00 00 00 00 01 00 00 00 ed 00 00 00 01 02
00 00 00 00 03 00 00 00 00 15 00 03 01 00 00 00 05 12 00 00 00 70 6b 67
2e 64 62 24 72 6f 77 73 24 53 33 5f 52 6f 77 12 00 00 00 70 6b 67 2e 64
62 24 72 6f 77 73 24 53 33 5f 52 6f 77 06 00 00 00 70 6b 67 2e 64 62 21
00 00 00 5f 5f 61 6c 69 67 6e 5f 72 65 73 6f 75 72 63 65 5f 64 72 6f 70
24 70 6b 67 2e 64 62 24 72 6f 77 73 21 00 00 00 5f 5f 61 6c 69 67 6e 5f
72 65 73 6f 75 72 63 65 5f 64 72 6f 70 24 70 6b 67 2e 64 62 24 72 6f 77
73 01 00 00 00 61 6c 69 67 6e 2d 72 65 73 2d 64 72 6f 70 2d 31 01 00 00
00 3a 00 00 00 00 03 01 00 00 00 00 03 00 00 00 52 6f 77 00 00 01 00 00
00 05 00 00 00 6c 61 62 65 6c 12 32 00 00 00 00 01 01 00 00 00 01 00 00
00 00 00 00 00 01 01 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 01
00 00 00 00 00 00 00 02 00 00 00 00 00 00 00 01 00 00 00 02 00 00 00 01
00 00 00 04 00 00 00 03 00 00 00 01 00 00 00 01 00 00 00 02 00 00 00 00
00 00 00 00 00 00 00 01 00 00 00 04 00 00 00 00 00 00 00 00 06 01 02 01
00 00 00 03 00 00 00 00 01 00 00 00 f1 00 01 00 00 00 00 00 00 00 00 00
00 30 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 02 00 00 00 50 01
06 03 03 00 00 00 02 00 00 00 01 01 00 00 00 02 00 00 00 02 00 00 00 00
02 01 00 00 00 02 00 00 00 01 00 00 00 04 00 00 00 02 00 00 00 00 00 00
00 1b 00 00 01 02 00 00 00 00 00 00 00 02 01 01 02 00 00 00 01 00 00 00
00 0b 00 00 00 73 61 6d 70 6c 65 2e 72 65 61 64 00 00 00 00 00 00 00 00
```

The independently stated record, encoder and restricted reader agree. All 864
proper prefixes, trailing input, validator-offset substitution, wrong operation
or witness, wrong Row type, absent signature and substituted return roots reject.
This record checks typed transport. It does not authenticate a raw address or
recover the resource's concrete generic arguments from its canonical layout;
that relationship belongs to source formation and the selected interface owner.

The existing `streaming_decoder_bridge_rejects_malformed_rows_generation_provenance`
MIR owner checks this producer and rejects a substituted empty return-region root.
The public `pkg.db.next` compile-only probe must import `pkg.db.sqlite` to retain
the SQLite next implementation: importing only `pkg.db` lowers the unsupported
branch and therefore does not witness StreamDecode. The admitted SQLite MIR loads
the decoder at offset 48 and calls `(context, rows_reference)` with both roots
selecting argument 1. Calling the compiler-private descriptor operation directly
from application source remains rejected; this plan introduces no such surface.

The indirect owned-return record combines FnAddr, the finite `fn() -> string`
type with DynamicBit cleanup, CallIndirectWithCleanup and ReturnWithCleanup.
Program 0 forms program 1's function address, receives owned result 1 and its
atomic Boolean cleanup companion 2, and forwards both. Program 1 returns a fresh
clone of `"x"` with true. The Fn type is 19 bytes; the selected ABI is the same
14-byte owned-string ABI as the direct-call record. The complete bundle is
288 bytes (outer length `20010000`):

```text
04 00 00 00 06 00 00 00 03 00 00 00 00 02 06 00 00 00 03 00 00 00 00 12
06 00 00 00 03 00 00 00 00 13 13 00 00 00 03 01 00 00 00 04 00 00 00 00
13 00 00 01 34 00 00 00 00 01 00 00 00 0e 00 00 00 01 00 00 00 00 03 00
00 00 00 13 00 00 01 00 00 00 00 02 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 03 00 00 00 03 00 00 00 02 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 01 00 00 00 02 00 00 00 00 00 00 00 00 69 00 00 01 00 00 00 00
00 00 00 00 01 00 00 00 1c 00 01 00 00 00 00 00 00 00 00 00 00 00 00 02
00 00 00 00 00 00 00 02 00 00 00 03 01 01 00 00 00 01 02 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 02 00 00 00 01 00 00 00 02 00 00 00 00 00
00 00 00 00 00 00 01 00 00 00 02 00 00 00 00 00 00 00 00 1e 01 01 00 00
00 78 00 01 00 00 00 1b 01 01 00 00 00 00 03 01 01 00 00 00 00 03 01 01
00 00 00 00 08 00 00 00 73 61 6d 70 6c 65 2e 66 00 00 00 00 00 00 00 00
```

The independent encoder and restricted reader match the golden. All 288 proper
prefixes, trailing input, missing target/signature, wrong result type, a non-Bool
cleanup value and aliasing/out-of-range cleanup companions reject. This closes
indirect owned-return transport; it does not replace active-arm Drop or runtime
allocation-parity owners. The compile-only source `call := owned; value := call()`
with `owned() -> string = "x".clone()` passes MIR and per-unit checks. Its actual
MIR uses FnAddr with DynamicBit, CallIndirectWithCleanup and a branch on the
returned cleanup bit before Drop. No fixture execution was needed for the wire
or producer-shape check.

A nested cleanup record uses `Holder { value: Option<string> }`. Program 1
constructs Some of a freshly cloned string, stores that initialized field and
returns the loaded Holder with DynamicBit true. Program 0 receives the Holder
and its atomic cleanup companion, stores the Holder, and branches on the
companion before Drop. The cleanup layout derives the field/Some/String path
from the 38-byte canonical Holder graph; no independent opaque cleanup-plan
blob or resource Drop binding is present. The owned ABI is 46 bytes. This
complete record is 426 bytes (outer length `aa010000`):

```text
05 00 00 00 06 00 00 00 03 00 00 00 00 02 07 00 00 00 03 00 00 00 00 04
05 06 00 00 00 03 00 00 00 00 12 06 00 00 00 03 00 00 00 00 13 26 00 00
00 03 01 00 00 00 00 06 00 00 00 48 6f 6c 64 65 72 00 00 01 00 00 00 05
00 00 00 76 61 6c 75 65 04 05 32 00 00 00 00 02 00 00 00 0e 00 00 00 01
00 00 00 00 03 00 00 00 00 38 00 00 00 2e 00 00 00 01 00 00 00 00 03 01
00 00 00 00 06 00 00 00 48 6f 6c 64 65 72 00 00 01 00 00 00 05 00 00 00
76 61 6c 75 65 04 05 32 00 00 00 00 00 00 01 00 00 00 00 02 00 00 00 00
00 00 00 00 00 00 00 01 00 00 00 04 00 00 00 02 00 00 00 04 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 03 00 00 00 02 00 00 00 00 00 00 00 00
1d 00 00 01 00 00 00 00 00 00 00 01 00 00 00 01 00 00 00 00 01 00 00 00
00 01 01 01 00 00 00 01 00 00 00 02 00 00 00 01 00 00 00 16 00 00 00 00
00 02 00 00 00 00 00 00 00 02 00 01 00 00 00 00 00 00 00 01 00 00 00 04
00 00 00 04 00 00 00 02 00 00 00 03 00 00 00 01 00 00 00 04 00 00 00 00
00 00 00 00 00 00 00 01 00 00 00 05 00 00 00 00 00 00 00 00 1e 01 01 00
00 00 78 00 01 00 00 00 1b 01 01 00 00 00 00 00 02 00 00 00 d9 00 01 01
00 00 00 02 00 00 00 00 01 00 00 00 00 00 00 00 01 02 00 00 00 00 03 00
00 00 c8 00 00 00 00 00 03 01 03 00 00 00 00 03 01 01 00 00 00 00 08 00
00 00 73 61 6d 70 6c 65 2e 66 00 00 00 00 00 00 00 00
```

The independent encoder and restricted reader match the stated record and reject
all 426 proper prefixes, trailing bytes, a missing Drop slot, substituted field,
Str in place of Some's String payload and an overlapping cleanup companion.
The Holder source constructor and caller pass MIR and per-unit checks; their
actual MIR includes the existing temporary-owner staging and guarded Drop.
This full record closes nested active-arm cleanup transport, not execution or
allocation parity. Existing cleanup owners still exercise None, Some, partial
construction, early exit and replacement; the wire fixture does not claim those
unexecuted branches passed a runtime test.

The Query constructor record uses empty Params `u$P`, Row `u$R { ready: bool }`
and its opaque `$static: raw` descriptor. It records SQLiteOnly with effective
CheckedRequired and SQLiteRequireVersionAtLeast 3.45.1. The ordered option bytes
are Common/0/2 followed by SQLite/0 and three u32le components. This is a source
request, so CheckedRequired does not claim that a database artifact has already
been checked or that SQL was read during access admission. The complete record
is 313 bytes (outer length `39010000`):

```text
03 00 00 00 18 00 00 00 03 01 00 00 00 00 03 00 00 00 75 24 50 00 00 00
00 00 00 32 00 00 00 00 22 00 00 00 03 01 00 00 00 00 03 00 00 00 75 24
52 00 00 01 00 00 00 05 00 00 00 72 65 61 64 79 02 32 00 00 00 00 35 00
00 00 03 01 00 00 00 00 14 00 00 00 70 6b 67 2e 64 62 24 71 75 65 72 79
24 75 24 50 24 75 24 52 00 00 01 00 00 00 07 00 00 00 24 73 74 61 74 69
63 15 32 00 00 00 00 01 00 00 00 3d 00 00 00 01 00 00 00 00 03 01 00 00
00 00 14 00 00 00 70 6b 67 2e 64 62 24 71 75 65 72 79 24 75 24 50 24 75
24 52 00 00 01 00 00 00 07 00 00 00 24 73 74 61 74 69 63 15 32 00 00 00
00 00 00 00 00 00 00 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
01 00 00 00 02 00 00 00 00 00 00 00 00 00 00 00 01 00 00 00 01 00 00 00
00 00 00 00 00 4f 01 00 02 00 00 00 00 00 00 00 01 00 00 00 01 02 00 00
00 00 00 02 01 00 03 00 00 00 2d 00 00 00 01 00 00 00 02 01 01 00 00 00
00 01 00 00 00 00 07 00 00 00 75 2e 71 75 65 72 79 00 00 00 00 00 00 00
00
```

The independently stated record, encoder and restricted reader agree. All 313
proper prefixes, trailing bytes, PostgreSQL driver substitution beside the
SQLite option, invalid policy, reversed or duplicate options, wrong Row and
Command-witness substitution reject. Params, Row and descriptor TypeRefs remain
distinct; the public record includes no SQL, native pointer, checked metadata or
artifact digest. The existing pure option/Params validator remains the owner for
PostgreSQL parameter naming and canonical type rules. These request bytes do not
replace source formation, artifact construction or database service tests.

The schemas specify seven operand alternatives, five scalar constant
alternatives and five terminators. The full vectors are representative examples;
they do not claim every operation combination was executed in the compiler. Compound Rvalue records and exact transfer
recipes follow below; neither a catch-all operand nor an opaque unchecked
instruction byte string is permitted.

## Closure matrix

Owner names below designate the tests to implement, not completed evidence.

| Axis | Required implementation and owner |
| --- | --- |
| Formation and layouts | `view_access_layout_matrix`: every admitted view-bearing type, separate backing/content paths, recursive type graphs, bad ids and incompatible projections. Existing formation rejects unresolved or illegal concrete types. |
| Construction and copying | `view_access_transfer_matrix`: literals/constants/mapped/static descriptor origins; fixed value copies versus slot slices; shallow versus deep owned copies; disjoint siblings in both field orders. |
| Replacement and joins | `view_access_control_matrix`: direct and selected-place replacement; both branches, match, else, Try/map_err, loop backedges, value-carrying breaks and early returns. Writes before replacement remain observable. |
| Aliased heap contents | `view_access_alias_matrix`: aliases before and after calls, returned aliases, mutable descriptor outputs, collection elements, builders, recursive allocation and summary-location weak updates. |
| Source timing and completion | `view_access_source_event_matrix`: the identical-MIR receiver/index pair, live-only capture, ended-child rebind, scalar/owned/byte cutoff, exact-destination effects, branch consolidation and loop-local release. `view_access_event_validation_matrix`: typed subjects, malformed ordinals, undefined/duplicate snapshots, merge agreement, terminating paths and remapping. Full HIR placement inventory remains required. |
| Byte validation across calls | `view_access_byte_observation_matrix`: UTF-8 and codec kinds, hidden by-value writes, callee-created validation, observation dependency closure, disjoint and raw-byte controls, eager operand order, text derivatives, owned copies, repeat-site revalidation and recursive observation recency and unknown-backing transport. |
| Calls and captures | `view_access_call_matrix`: direct/imported/generic and indirect calls, target-relative captures, reader/writer twins, unavailable alternatives, recursive readers, nonreturning callees and completed operands. |
| Synthesized applications | `view_access_pipeline_matrix`: map/reduce/scan/filter/partition/sort/group/materializers; empty reduce returns its initial accumulator, scan emits only callback results; task spawn, wait and result reachability retain their existing control contract. |
| Native origins | `view_access_native_matrix`: exact operation inventory, retained-input versus owned-copy twins, borrowed JSON/codec carriers, regex offset-only controls, XML and HTTP projections, raw/resource boundaries. No owner-shell shortcut. |
| Every destination | `view_access_sink_matrix`: scalar and field stores, pointer-derived safe stores, SIMD store, map_into, shuffle, native OutBytes and explicit Out/BorrowMut views. Keep the original place/type/alias/range guards. |
| Ownership lifecycle | Existing borrowed-parameter, return-provenance and mutable-retention owners remain authoritative for move-in/out, nulling, Drop, replacement, early exit and return. Access facts never mint an owner or shorten a lifetime. |
| Whole/per-unit/cache | `view_access_whole_unit_parity`: named/private/imported/generic/callback peers; changing only a helper's access behavior invalidates the importer; restoring it cannot reuse a stale accepting artifact. |
| Scoped identity and implicit edges | `view_access_bundle_identity`: private/lifted-name and typed-ordinal permutations, unreachable private edits, recursive local closure, selected dependency imports and conflicting-unit rejection. Include declaration-owned Drop wrappers, internal imported hooks, dormant generic resources and nested cleanup paths. Resource type/ABI projection must match producer and surrogate imports while retaining actual hook behavior and rejecting mismatched support metadata. |
| Malformed boundaries | `view_access_mir_replay`: mutate actual accepted MIR producers, calls and paths without changing copied summaries; reject before emission. Interface owners reject malformed records and compare independent semantic/byte goldens in both directions. |
| Termination | `view_access_finite_domain`: recursive calls, cyclic heap/capture graphs and fresh escaping allocations reach a finite fixed point without a clean-on-timeout path. Retain existing bounded/deep owner budgets. |

## Native permission and byte-validation contract

The allocation-only interpretation of writability is insufficient. A descriptor
can point into owned writable memory while carrying a pointer obtained through a
read-only Rust borrow. The exact native tables distinguish storage ownership, pointer origin and
copy behavior; only their explicit Writable producers grant write authority.

Rust's [`Vec::as_ptr`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.as_ptr)
contract prohibits writes through that pointer or its derivatives. The
[`str::as_ptr`](https://doc.rust-lang.org/std/primitive.str.html#method.as_ptr)
contract has the same restriction, and obtaining a mutable string pointer also
requires preserving UTF-8. `CString::as_ptr` is similarly read-only. These are
provider contracts, independent of whether the memory page is physically writable.

The native pointer prerequisite is now implemented. Plan 37 publishes
`buffer.bytes()` through a cached mutable pointer while preserving concurrent
shared getters. HTTP/SSE output helpers use `MaybeUninit<u8>` and expose only
initialized prefixes. Plan 52 records read-only origins for shared HTTP body and
process byte-capture views. Text projections publish read-only bytes regardless
of who owns their backing. Interprocedural transport must preserve each exact
producer's permission; allocation ownership is still not authority to write.

Plans 57 and 60 implement local UTF-8 and codec observations. Remaining compile-only
negative witnesses validate a caller's bytes inside a callee, then mutate an
alias and use the returned text; or validate locally, call a hidden slice writer,
and use the old text. Both checkers still accept these witnesses at the baseline
above. They have not been executed.

The access program must preserve descriptor permission, allocation identity and
live byte-validation observations independently. Reuse plans 57 and 60's completed backing
snapshots, validity-only invalidation, observation recency and derivative rules
at the interprocedural boundary. Its public lifetime summary deliberately expands
validation observations to underlying lifetime roots; that summary is not a
validity effect. A byte write through an alias must invalidate reaching text
and codec observations even when no owner moves, no descriptor changes and the callee's
return contains no view. An explicit owned text copy detaches that dependency.

The exact observation-producing transfer records and ordered instruction grammar
below determine these effects. Observation identities are derived during
interpretation, not serialized as independent certificates. The
implemented local passes remain prerequisites and regression owners; this plan
does not reopen their source contract or replace them with an allocation-only
permission rule.

## Native evidence and implementation owners

| Producer family | Evidence and transfer requirement |
| --- | --- |
| StaticDescriptorView | Descriptor ID and both SQL strings point at compiler static storage. Plan 52's local repair seeds read-only origins; call transport must preserve them. |
| FsReadFileView / FsReadBytesView | Preserve the existing read-only classification including the allocated fallback. Arena lifetime does not establish write authority. |
| CodecOpen and column projections | The codec retains its input descriptor. Batch names and borrowed column views select input-derived storage and preserve the codec validation observation from plan 60. Scalar results do not retain it. Never infer fresh bytes from the codec owner. |
| Borrowed JSON and JsonDoc | Unescaped strings can be input subslices; decoded escapes can use fresh arena bytes. Fresh container backing is separate from both alternatives. Owned decoding has fresh owned string leaves. |
| XML | Names and attribute names borrow the consumed owned input; attribute values and decoded text use `publish_owned` and fresh storage (`xml.rs` native producers). These are distinct transfers, not one blanket input union. |
| HTTP | Response and request-context views select receive/copied buffers. SSE last-event-id selects the stream-owned committed-id vector; SSE next-event text/data/id select the explicit caller buffer. Request input literals do not taint an independent copied response. Each header/body/stream projection uses its exact producer mapping below. |
| CLI / regex / column batch | CLI string values own copied text and defaults. Regex matches/capture groups expose offsets only; replacement produces fresh text. Column-batch append copies variable-width inputs into owned child buffers (`column_batch_append`); it does not retain native input pointers as row text. |
| Qualified DB RawCall views | `producer.rs::native_view_call_matches` already checks all-ByValue physical arguments, no cleanup, protected Copy result leaves, raw-pointer load shape/offset, row/resource identity and return roots. Stream decode additionally proves the same owner observation; batch projections require their plan guard. This qualifies the existing shared-view producer boundary, not writable bytes or an arbitrary raw-call effect. The descriptor access producer must bind semantic roles without treating the existing package-name shape checks alone as authority. |
| ResourceViewFromRaw / RawCall | Pointer shape, length, alignment, UTF-8 and owner validation do not establish backing writability. The existing unsafe construction contract supplies no general writable guarantee. Raw slice publication is `Unavailable` for safe writes; reads and explicit copies remain admitted. Requested text still publishes read-only bytes. Existing app adapters read scalar bytes or explicitly clone native blob/bytea data before retention; no trusted-module write exemption is needed. |

`NativeOwnerMirContract`, `xml_written_slots` and the existing exhaustive MIR
variant list provide the shape and output inventory. They do not supply the
missing view-origin semantics. The current inventory contains 335 Rvalue variants;
every variant and every relevant operation-kind discriminator needs an explicit
transfer classification before the author pass can claim closure.

## Implementation acceptance

The design decisions are fixed above. Remaining work is compiler implementation
and verification of the exact records, not another preimplementation research
cycle. Run one author matrix-to-diff pass on that implementation.

| Acceptance boundary | Required evidence |
| --- | --- |
| Actual user defect | Separate plain-parameter writes and returned-slice writes; reject literal/native read-only inputs and preserve readers/explicit owned copies in both frontends. |
| Relational core | Port saved bounded controls to the real typed domain; combine recursive collection/callback/observation cases in view_access_finite_domain. No permissive budget fallback. |
| Source timing and shape | Preserve the identical-runtime-MIR/different-source-use pair, stack-bounded traversal, all source event routes and negative ordinal/CFG controls. |
| Canonical transport | Run complete independent vectors through the production codec; check reserved native tags, private descriptor rename, native-identity projection and deterministic multi-invalid errors. |
| Native/DB integration | Use the enumerated existing producer, resource, ABI, cleanup, SQL-identity and DB owner suites for changed boundaries. Unsafe code retains its conditional contract. |
| Publication and cache | Close whole/per-unit, cold/reused/rehydrated, test/no-test, module/partition/harness routes and stale-body rejection before normal output publication. |

A model pass does not satisfy a compiler owner. Documentation status must not
claim the original defect is repaired until those implementation owners pass.
Update the listed source-of-truth integration rows as part of that capability,
before its final-SHA preflight. No new benchmark is required without an explicit
performance/resource promise.

## AccessRvalue field schema

Each Rvalue starts with the explicit `u16` little-endian tag below. Payload fields
follow in the listed order, without field-name bytes or padding. These frozen tags
were assigned in UTF-8 name order at the investigation baseline; adding a future Rust
variant does not automatically renumber them. A schema change requires a deliberate
interface-version change and updated independent goldens/inventory owners.

`TypeRef`, `ScalarTypeRef`, `AbiRef`, `ProgramRef`, `SlotRef` and `ValueRef` are
`u32` references to their respective bundle or function tables. ScalarTypeRef must
additionally decode to an admitted `Scalar` root. `sequence<T>` and `optional<T>`
use the previously defined encodings; `pair<A,B>` writes A then B with no tag.
`i32` and `i128` use four and 16 little-endian two's-complement bytes. `bool` is one
0/1 byte; `utf8_data` is a length-prefixed UTF-8 byte string with NUL preserved.
Boxed Rust payloads are inline records on the wire, with no allocation marker.
Fields named `arg0`, `arg1` and so on correspond to positional MIR variant fields.

The nominal `struct_id`, `options_struct_id`, `enum_id`, `error_enum`, `event_enum`,
`tuple_id` and `resource` fields are canonical TypeRef records of the required
nominal kind, never process-local definition ids. A resource reference here names
its full canonical resource definition through the owned resource root; it does
not assert that the particular operand owns the resource. Ordinary field, variant,
payload and array ordinals remain integers in their selected type's coordinate
space. Those roles must not be conflated by a generic u32 remapper.

A `FnSignatureFacts` payload becomes an AbiRef formed from its complete validated
operand/result types and signature facts. For FnAddr/Closure, recover the explicit
function type from the containing value's validated type; for indirect/RawCall,
correlate the explicit parameter/return fields. The ABI's modes, return summaries
and cleanup must agree; a caller cannot replace only the signature facts while
retaining a conflicting type list. Lifted targets separately check their trailing
capture ABI as described above.

The baseline inventory accounts for all 335 variants and 793 direct fields without
a default mapping. StaticData and the four ColumnBatch variants have explicit native-only exclusions;
the inventory is not permission to copy every native field into the public wire.
The two parallel Rvalues omit `work_weight` from the source record: initial
lowering sets it to `PAR_MAP_DEFAULT_WORK_WEIGHT`, and `annotate_par_map_work`
computes the native cost hint after source admission and event erasure. Access
derivation must not read this hint. Actual native validation still checks its
admitted 1/2/4 values; changing only this hint cannot change the access bundle.
Named compound records and `*Tag` enums use the exact secondary schemas below;
a compound name without that schema is not a completed codec. Native transfer recipes
also remain distinct work: preserving all fields does not determine what storage
a native operation returns or invalidates.

| Tag | Rvalue | Payload fields |
| --- | --- | --- |
| 0 | `ArenaAlloc` | `handle:AccessOperand, count:AccessOperand, elem:TypeRef` |
| 1 | `ArenaBegin` | None |
| 2 | `ArrayBuilderAppend` | `builder:AccessOperand, data:AccessOperand` |
| 3 | `ArrayBuilderBuild` | `builder:AccessOperand` |
| 4 | `ArrayBuilderNew` | `elem:TypeRef, region:optional<AccessOperand>` |
| 5 | `ArrayBuilderPush` | `builder:AccessOperand, value:AccessOperand, scalar:TypeRef` |
| 6 | `ArrayBuilderPushStr` | `builder:AccessOperand, value:AccessOperand` |
| 7 | `Bin` | `arg0:BinOpTag, arg1:AccessOperand, arg2:AccessOperand` |
| 8 | `BoxClone` | `arg0:AccessOperand, arg1:AccessOperand` |
| 9 | `BoxGet` | `arg0:AccessOperand` |
| 10 | `BufferAppend` | `buffer:AccessOperand, data:AccessOperand` |
| 11 | `BufferBytes` | `arg0:AccessOperand` |
| 12 | `BufferCapacity` | `arg0:AccessOperand` |
| 13 | `BufferLen` | `arg0:AccessOperand` |
| 14 | `BufferNew` | `arg0:AccessOperand` |
| 15 | `BufferPut` | `buffer:AccessOperand, value:AccessOperand, scalar:TypeRef, be:bool` |
| 16 | `BuilderNew` | `capacity:AccessOperand` |
| 17 | `BuilderToString` | `arg0:AccessOperand` |
| 18 | `BuilderWriteBool` | `arg0:AccessOperand, arg1:AccessOperand` |
| 19 | `BuilderWriteChar` | `arg0:AccessOperand, arg1:AccessOperand` |
| 20 | `BuilderWriteFloat` | `arg0:AccessOperand, arg1:AccessOperand` |
| 21 | `BuilderWriteInt` | `arg0:AccessOperand, arg1:AccessOperand` |
| 22 | `BuilderWriteStr` | `arg0:AccessOperand, arg1:AccessOperand` |
| 23 | `BuilderWriteStrIntStr` | `arg0:AccessOperand, arg1:AccessOperand, arg2:AccessOperand, arg3:AccessOperand` |
| 24 | `BytesAsStr` | `bytes:AccessOperand, out:SlotRef` |
| 25 | `BytesRead` | `bytes:AccessOperand, offset:AccessOperand, scalar:TypeRef, be:bool` |
| 26 | `Call` | `arg0:AccessDirectTarget, arg1:sequence<AccessOperand>` |
| 27 | `CallIndirect` | `callee:AccessOperand, args:sequence<AccessOperand>, param_tys:sequence<TypeRef>, ret_ty:TypeRef, signature:AbiRef` |
| 28 | `CallIndirectWithCleanup` | `arg0:AccessIndirectCallWithCleanup` |
| 29 | `CallWithCleanup` | `arg0:AccessDirectCallWithCleanup` |
| 30 | `CapturesGroup` | `caps:AccessOperand, index:AccessOperand, out:SlotRef` |
| 31 | `Cast` | `operand:AccessOperand, from:TypeRef, to:TypeRef` |
| 32 | `ChildKill` | `child:AccessOperand, sig:AccessOperand` |
| 33 | `ChildWait` | `child:AccessOperand, out:SlotRef` |
| 34 | `Chunks` | `src:AccessOperand, n:AccessOperand, elem:TypeRef` |
| 35 | `CliCommand` | `name:AccessOperand` |
| 36 | `CliFlag` | `cmd:AccessOperand, kind:CliFlagKindTag, name:AccessOperand, default:optional<AccessOperand>` |
| 37 | `CliGetBool` | `parsed:AccessOperand, name:AccessOperand` |
| 38 | `CliGetI64` | `parsed:AccessOperand, name:AccessOperand` |
| 39 | `CliGetStr` | `parsed:AccessOperand, name:AccessOperand` |
| 40 | `CliParse` | `cmd:AccessOperand, args:AccessOperand, out:SlotRef` |
| 41 | `CliUsage` | `cmd:AccessOperand` |
| 42 | `CloneIn` | `value:AccessOperand, handle:AccessOperand` |
| 43 | `Closure` | `lifted:ProgramRef, captures:sequence<AccessOperand>, capture_tys:sequence<TypeRef>, signature:AbiRef` |
| 44 | `CodecBatchColumn` | `batch:AccessOperand, index:AccessOperand, kind:CodecPutKindTag` |
| 45 | `CodecBatchColumns` | `arg0:AccessOperand` |
| 46 | `CodecBatchFind` | `arg0:AccessOperand, arg1:AccessOperand` |
| 47 | `CodecBatchKind` | `arg0:AccessOperand, arg1:AccessOperand` |
| 48 | `CodecBatchName` | `arg0:AccessOperand, arg1:AccessOperand` |
| 49 | `CodecBatchRows` | `arg0:AccessOperand` |
| 50 | `CodecColumnAt` | `column:AccessOperand, index:AccessOperand, kind:CodecPutKindTag` |
| 51 | `CodecColumnLen` | `arg0:AccessOperand` |
| 52 | `CodecEncoderFinish` | `arg0:AccessOperand` |
| 53 | `CodecEncoderNew` | `rows:AccessOperand, out:SlotRef` |
| 54 | `CodecEncoderPut` | `encoder:AccessOperand, name:AccessOperand, values:AccessOperand, kind:CodecPutKindTag` |
| 55 | `CodecOpen` | `arg0:AccessOperand` |
| 56 (reserved; reject) | `ColumnBatchAppend` | Native materialization only; no public source-access payload. |
| 57 (reserved; reject) | `ColumnBatchCreate` | Native materialization only; no public source-access payload. |
| 58 (reserved; reject) | `ColumnBatchRow` | Native materialization only; no public source-access payload. |
| 59 (reserved; reject) | `ColumnBatchSoa` | Native materialization only; no public source-access payload. |
| 60 | `Command` | `cmd:AccessOperand, args:AccessOperand` |
| 61 | `CommandCwd` | `command:AccessOperand, dir:AccessOperand` |
| 62 | `CommandEnv` | `command:AccessOperand, name:AccessOperand, value:AccessOperand` |
| 63 | `CommandEnvClear` | `command:AccessOperand` |
| 64 | `CommandMaxCapture` | `command:AccessOperand, limit:AccessOperand` |
| 65 | `CommandRun` | `command:AccessOperand, out:SlotRef` |
| 66 | `CommandRunBytes` | `command:AccessOperand, out:SlotRef` |
| 67 | `CommandTimeout` | `command:AccessOperand, ns:AccessOperand` |
| 68 | `CompressCompress` | `kind:CompressKindTag, data:AccessOperand, level:AccessOperand, out:SlotRef` |
| 69 | `CompressDecompress` | `kind:CompressKindTag, data:AccessOperand, out:SlotRef` |
| 70 | `ConnReader` | `arg0:AccessOperand` |
| 71 | `ConnWriter` | `arg0:AccessOperand` |
| 72 | `ConstArray` | `elems:sequence<AccessConstElement>, elem:TypeRef` |
| 73 | `CryptoAead` | `cipher:AeadCipherTag, dir:AeadDirTag, key:AccessOperand, nonce:AccessOperand, input:AccessOperand, aad:AccessOperand, out:SlotRef` |
| 74 | `CryptoArgon2` | `arg0:AccessArgon2` |
| 75 | `CryptoCtEqual` | `a:AccessOperand, b:AccessOperand` |
| 76 | `CryptoDigestFinish` | `arg0:AccessOperand` |
| 77 | `CryptoDigestNew` | None |
| 78 | `CryptoDigestUpdate` | `digest:AccessOperand, data:AccessOperand` |
| 79 | `CryptoHash` | `algo:HashAlgoTag, data:AccessOperand` |
| 80 | `CryptoHkdf` | `salt:AccessOperand, ikm:AccessOperand, info:AccessOperand, len:AccessOperand, out:SlotRef` |
| 81 | `CryptoHmac` | `key:AccessOperand, data:AccessOperand` |
| 82 | `CryptoPrivateKeyFromPem` | `algorithm:SignatureAlgorithmTag, pem:AccessOperand, out:SlotRef` |
| 83 | `CryptoPublicKeyFromJwk` | `arg0:AccessSignatureJwk` |
| 84 | `CryptoPublicKeyFromPem` | `algorithm:SignatureAlgorithmTag, pem:AccessOperand, out:SlotRef` |
| 85 | `CryptoRandom` | `out:AccessOperand` |
| 86 | `CryptoSign` | `algorithm:SignatureAlgorithmTag, key:AccessOperand, message:AccessOperand, out:SlotRef` |
| 87 | `CryptoVerify` | `arg0:AccessSignatureVerify` |
| 88 | `CsvDecode` | `struct_id:TypeRef, options_struct_id:TypeRef, input:AccessOperand, arena:AccessOperand, options:AccessOperand, out:SlotRef` |
| 89 | `DictEncode` | `base:SlotRef, struct_id:TypeRef, key_field:u32, out_ids:AccessOperand, out_dict:AccessOperand` |
| 90 | `DictField` | `base:SlotRef, idx:u32` |
| 91 | `DictLookup` | `ids:AccessOperand, n:AccessOperand, dict:AccessOperand, out:AccessOperand` |
| 92 | `DnsResolve` | `host:AccessOperand, out:SlotRef` |
| 93 | `EncodingDecode` | `kind:EncodingKindTag, input:AccessOperand, out:SlotRef` |
| 94 | `EncodingEncode` | `kind:EncodingKindTag, data:AccessOperand` |
| 95 | `EnumPayload` | `enum_id:TypeRef, variant:u32, slot:u32, operand:AccessOperand` |
| 96 | `EnumTagEq` | `enum_id:TypeRef, scrutinee:AccessOperand, variant:u32` |
| 97 | `EnvGet` | `name:AccessOperand, out:SlotRef` |
| 98 | `EnvSet` | `name:AccessOperand, value:AccessOperand` |
| 99 | `Field` | `arg0:SlotRef, arg1:sequence<u32>` |
| 100 | `FileCreateRw` | `path:AccessOperand, out:SlotRef` |
| 101 | `FileLen` | `file:AccessOperand` |
| 102 | `FileOpenRw` | `path:AccessOperand, out:SlotRef` |
| 103 | `FilePread` | `file:AccessOperand, buffer:AccessOperand, offset:AccessOperand` |
| 104 | `FilePwrite` | `file:AccessOperand, data:AccessOperand, offset:AccessOperand` |
| 105 | `FnAddr` | `target:AccessAlignTarget, signature:AbiRef` |
| 106 | `FrameInnerJoin` | `left:AccessOperand, right:AccessOperand, max_pairs:AccessOperand, kind:FrameJoinKindTag, out:SlotRef` |
| 107 | `FsCreateDir` | `path:AccessOperand` |
| 108 | `FsCreatePrivateTempDir` | `prefix:AccessOperand, out:SlotRef` |
| 109 | `FsExists` | `path:AccessOperand` |
| 110 | `FsIsDir` | `path:AccessOperand, out:SlotRef` |
| 111 | `FsReadBytesView` | `path:AccessOperand, arena:AccessOperand, out:SlotRef` |
| 112 | `FsReadDir` | `path:AccessOperand, out:SlotRef` |
| 113 | `FsReadFile` | `path:AccessOperand, out:SlotRef` |
| 114 | `FsReadFileView` | `path:AccessOperand, arena:AccessOperand, out:SlotRef` |
| 115 | `FsRemove` | `path:AccessOperand` |
| 116 | `FsRemoveEmptyDir` | `path:AccessOperand` |
| 117 | `FsTree` | `kind:FsTreeKindTag, args:sequence<AccessOperand>, output:AccessFsTreeOutput` |
| 118 | `FsWriteFile` | `path:AccessOperand, data:AccessOperand` |
| 119 | `FsWriteFileBuilder` | `path:AccessOperand, builder:AccessOperand` |
| 120 | `GatherColumnI64` | `source:AccessOperand, struct_id:TypeRef, field:u32, out:AccessOperand` |
| 121 | `GroupAgg` | `keys:AccessOperand, vals:AccessOperand, out_keys:AccessOperand, out_vals:AccessOperand, op:GroupOpTag` |
| 122 | `GroupAggMultiStr` | `base:SlotRef, struct_id:TypeRef, key_field:u32, aggs:sequence<pair<GroupOpTag,optional<u32>>>, out_keys:AccessOperand, out_vals:sequence<AccessOperand>` |
| 123 | `GroupAggStr` | `base:SlotRef, struct_id:TypeRef, key_field:u32, value_field:optional<u32>, op:GroupOpTag, out_keys:AccessOperand, out_vals:AccessOperand` |
| 124 | `GroupAggStrCols` | `keys:AccessOperand, vals:AccessOperand, out_keys:AccessOperand, out_vals:AccessOperand, op:GroupOpTag` |
| 125 | `HeapAlloc` | `arg0:AccessOperand, arg1:AccessOperand` |
| 126 | `HeapAllocBuf` | `count:AccessOperand, elem:TypeRef` |
| 127 | `HttpAccept` | `server:AccessOperand, out:SlotRef` |
| 128 | `HttpBody` | `req:AccessOperand, data:AccessOperand` |
| 129 | `HttpClient` | None |
| 130 | `HttpClientGet` | `client:AccessOperand, url:AccessOperand, out:SlotRef` |
| 131 | `HttpClientMaxResponseBodyBytes` | `client:AccessOperand, limit:AccessOperand` |
| 132 | `HttpClientPost` | `client:AccessOperand, url:AccessOperand, body:AccessOperand, out:SlotRef` |
| 133 | `HttpClientRequest` | `client:AccessOperand, req:AccessOperand, out:SlotRef` |
| 134 | `HttpClientRequestStream` | `client:AccessOperand, req:AccessOperand, out:SlotRef` |
| 135 | `HttpClientTimeout` | `client:AccessOperand, ns:AccessOperand` |
| 136 | `HttpCtxBody` | `ctx:AccessOperand` |
| 137 | `HttpCtxHeader` | `ctx:AccessOperand, name:AccessOperand, out:SlotRef` |
| 138 | `HttpCtxMethod` | `ctx:AccessOperand` |
| 139 | `HttpCtxPath` | `ctx:AccessOperand` |
| 140 | `HttpCtxUpgradeReady` | `ctx:AccessOperand` |
| 141 | `HttpGetMany` | `client:AccessOperand, urls:AccessOperand, max_concurrency:AccessOperand, out:SlotRef` |
| 142 | `HttpHeader` | `req:AccessOperand, name:AccessOperand, value:AccessOperand` |
| 143 | `HttpHeadersContainsToken` | `headers:AccessOperand, name:AccessOperand, token:AccessOperand, exact:bool` |
| 144 | `HttpHeadersCount` | `headers:AccessOperand, name:AccessOperand` |
| 145 | `HttpHeadersTokensValid` | `headers:AccessOperand, name:AccessOperand` |
| 146 | `HttpParse` | `data:AccessOperand, out:SlotRef` |
| 147 | `HttpRbBody` | `rb:AccessOperand, data:AccessOperand` |
| 148 | `HttpRbHeader` | `rb:AccessOperand, name:AccessOperand, value:AccessOperand` |
| 149 | `HttpReadStreamHeader` | `stream:AccessOperand, name:AccessOperand, out:SlotRef` |
| 150 | `HttpReadStreamRead` | `stream:AccessOperand, buffer:AccessOperand, out:SlotRef` |
| 151 | `HttpReadStreamSse` | `stream:AccessOperand` |
| 152 | `HttpReadStreamStatus` | `stream:AccessOperand` |
| 153 | `HttpRequest` | `method:AccessOperand, url:AccessOperand` |
| 154 | `HttpRequestMaxResponseBodyBytes` | `req:AccessOperand, limit:AccessOperand` |
| 155 | `HttpRequestTimeout` | `req:AccessOperand, ns:AccessOperand` |
| 156 | `HttpRespBody` | `resp:AccessOperand` |
| 157 | `HttpRespHeader` | `resp:AccessOperand, name:AccessOperand, out:SlotRef` |
| 158 | `HttpRespStatus` | `resp:AccessOperand` |
| 159 | `HttpRespond` | `ctx:AccessOperand, rb:AccessOperand` |
| 160 | `HttpRespondStream` | `ctx:AccessOperand, rb:AccessOperand, out:SlotRef` |
| 161 | `HttpRespondUpgrade` | `ctx:AccessOperand, rb:AccessOperand, out:SlotRef` |
| 162 | `HttpResponseBuilder` | `status:AccessOperand` |
| 163 | `HttpServe` | `host:AccessOperand, port:AccessOperand, out:SlotRef, shared:bool` |
| 164 | `HttpSseStreamLastEventId` | `stream:AccessOperand` |
| 165 | `HttpSseStreamNext` | `stream:AccessOperand, buffer:AccessOperand, present:SlotRef, retry_present:SlotRef, retry_ms:SlotRef, event:SlotRef, data:SlotRef, last_event_id:SlotRef` |
| 166 | `HttpSseStreamRetryMs` | `stream:AccessOperand` |
| 167 | `HttpStreamFinish` | `stream:AccessOperand` |
| 168 | `HttpStreamReject` | `stream:AccessOperand, rb:AccessOperand` |
| 169 | `HttpStreamSend` | `stream:AccessOperand, chunk:AccessOperand, event:bool` |
| 170 | `HttpUpgradeDeadline` | `upgrade:AccessOperand, timeout_ns:AccessOperand` |
| 171 | `HttpUpgradeReadExact` | `upgrade:AccessOperand, out:AccessOperand, count:AccessOperand` |
| 172 | `HttpUpgradeShutdown` | `upgrade:AccessOperand` |
| 173 | `HttpUpgradeWrite` | `upgrade:AccessOperand, data:AccessOperand` |
| 174 | `Index` | `arg0:SlotRef, arg1:AccessOperand` |
| 175 | `IndexColumn` | `base:AccessOperand, index:AccessOperand, field:u32, struct_id:TypeRef` |
| 176 | `IndexField` | `arg0:SlotRef, arg1:AccessOperand, arg2:sequence<u32>` |
| 177 | `IndexFieldPtr` | `base:AccessOperand, index:AccessOperand, path:sequence<u32>, struct_id:TypeRef` |
| 178 | `IndexPtr` | `base:AccessOperand, index:AccessOperand, struct_id:TypeRef` |
| 179 | `IntArith` | `op:BinOpTag, mode:ArithModeTag, int_ty:TypeRef, a:AccessOperand, b:AccessOperand` |
| 180 | `IoCopy` | `arg0:AccessOperand, arg1:AccessOperand` |
| 181 | `JsonDecode` | `struct_id:TypeRef, input:AccessOperand, out:SlotRef, arena:optional<AccessOperand>` |
| 182 | `JsonDecodeArray` | `elem:TypeRef, input:AccessOperand, out:SlotRef` |
| 183 | `JsonDecodeScalar` | `scalar:TypeRef, input:AccessOperand, out:SlotRef` |
| 184 | `JsonDecodeSoa` | `struct_id:TypeRef, input:AccessOperand, out:SlotRef, arena:AccessOperand` |
| 185 | `JsonDecodeStructArray` | `struct_id:TypeRef, input:AccessOperand, out:SlotRef, arena:optional<AccessOperand>` |
| 186 | `JsonDecodeUnion` | `enum_id:TypeRef, input:AccessOperand, out:SlotRef, arena:optional<AccessOperand>` |
| 187 | `JsonDoc` | `input:AccessOperand, arena:AccessOperand, out:SlotRef` |
| 188 | `JsonDocAsScalar` | `scalar:TypeRef, doc:AccessOperand, out:SlotRef` |
| 189 | `JsonDocAsStr` | `doc:AccessOperand, out:SlotRef` |
| 190 | `JsonDocAt` | `doc:AccessOperand, index:AccessOperand, out:SlotRef` |
| 191 | `JsonDocElems` | `doc:AccessOperand, arena:AccessOperand, out:SlotRef` |
| 192 | `JsonDocGet` | `doc:AccessOperand, key:AccessOperand, out:SlotRef` |
| 193 | `JsonDocKey` | `doc:AccessOperand, index:AccessOperand, out:SlotRef` |
| 194 | `JsonDocKind` | `doc:AccessOperand` |
| 195 | `JsonDocLen` | `doc:AccessOperand` |
| 196 | `JsonEncode` | `pieces:sequence<AccessTemplatePiece>, max_bytes:optional<AccessOperand>, out:SlotRef` |
| 197 | `JsonOwnedDecode` | `plan:AccessOwnedJsonPlan, input:AccessOperand, out:SlotRef` |
| 198 | `JsonScanNew` | `input:AccessOperand` |
| 199 | `JsonScanNext` | `scanner:AccessOperand, struct_id:TypeRef, cursor:SlotRef, row:SlotRef` |
| 200 | `Load` | `arg0:SlotRef` |
| 201 | `LogEnabled` | `arg0:AccessOperand, arg1:AccessOperand` |
| 202 | `LogFlush` | `arg0:AccessOperand` |
| 203 | `LogLine` | `arg0:AccessOperand, arg1:AccessOperand, arg2:AccessOperand` |
| 204 | `LogLineBuilder` | `arg0:AccessOperand, arg1:AccessOperand, arg2:AccessOperand` |
| 205 | `LogNew` | `arg0:AccessOperand, arg1:AccessOperand` |
| 206 | `MakeDictEncoded` | `source:AccessOperand, ids:AccessOperand, dict:AccessOperand` |
| 207 | `MakeDynArray` | `ptr:AccessOperand, len:AccessOperand` |
| 208 | `MakeEnum` | `enum_id:TypeRef, variant:u32, payload:sequence<AccessOperand>` |
| 209 | `MakeError` | `enum_id:TypeRef, tag:AccessOperand, code:AccessOperand` |
| 210 | `MakeSlice` | `arg0:SlotRef, arg1:i128` |
| 211 | `MakeTuple` | `tuple_id:TypeRef, elems:sequence<AccessOperand>` |
| 212 | `MakeVec` | `elems:sequence<AccessOperand>, elem:TypeRef, n:u32` |
| 213 | `MaskAny` | `mask:AccessOperand, n:u32` |
| 214 | `MathOp` | `fn_:MathFnTag, ty:TypeRef, operands:sequence<AccessOperand>` |
| 215 | `OptionIsSome` | `arg0:AccessOperand` |
| 216 | `OptionNone` | None |
| 217 | `OptionSome` | `arg0:AccessOperand` |
| 218 | `OptionUnwrap` | `arg0:AccessOperand` |
| 219 | `OsHost` | `out:SlotRef` |
| 220 | `OsIdentity` | `out:SlotRef` |
| 221 | `ParMapParallel` | `src:AccessParallelSource, func:AccessAlignTarget, stages:sequence<AccessParMapStage>, captures:sequence<AccessOperand>, capture_tys:sequence<TypeRef>, elem_in:TypeRef, elem_out:TypeRef` |
| 222 | `ParMapReduce` | `src:AccessParallelSource, func:AccessAlignTarget, captures:sequence<AccessOperand>, capture_tys:sequence<TypeRef>, elem_in:TypeRef, elem_out:TypeRef` |
| 223 | `PathComponent` | `kind:PathComponentKindTag, path:AccessOperand` |
| 224 | `PathJoin` | `a:AccessOperand, b:AccessOperand` |
| 225 | `PathNormalize` | `path:AccessOperand` |
| 226 | `ProcessCpuCount` | None |
| 227 | `ProcessExec` | `cmd:AccessOperand, args:AccessOperand` |
| 228 | `ProcessLive` | `kind:ProcessLiveKindTag, args:sequence<AccessOperand>, out:optional<SlotRef>` |
| 229 | `ProcessSpawn` | `cmd:AccessOperand, args:AccessOperand, out:SlotRef` |
| 230 | `RandNext` | `rng:SlotRef` |
| 231 | `RandRange` | `rng:SlotRef, lo:AccessOperand, hi:AccessOperand` |
| 232 | `RandSample` | `rng:SlotRef, xs:AccessOperand, k:AccessOperand, elem:TypeRef` |
| 233 | `RandSeed` | `seed:optional<AccessOperand>, out:SlotRef` |
| 234 | `RandShuffle` | `rng:SlotRef, xs:AccessOperand, elem:TypeRef` |
| 235 | `RawAlloc` | `arg0:AccessOperand` |
| 236 (reserved; reject) | `RawCall` | Native finalization only; source calls use authenticated `DbBridgeCall`. |
| 237 | `RawIsNull` | `arg0:AccessOperand` |
| 238 | `RawLoad` | `ptr:AccessOperand, offset:AccessOperand, scalar:ScalarTypeRef` |
| 239 | `RawNull` | None |
| 240 | `RawOffset` | `ptr:AccessOperand, offset:AccessOperand` |
| 241 | `RawPointerLoad` | `ptr:AccessOperand, offset:AccessOperand` |
| 242 | `ReaderBuffered` | `arg0:AccessOperand` |
| 243 | `ReaderOpen` | `path:AccessOperand, regular_only:bool, out:SlotRef` |
| 244 | `ReaderOpenBeneath` | `root:AccessOperand, relative:AccessOperand, out:SlotRef` |
| 245 | `ReaderOpenBeneathSingleLink` | `root:AccessOperand, relative:AccessOperand, out:SlotRef` |
| 246 | `ReaderRead` | `arg0:AccessOperand, arg1:AccessOperand` |
| 247 | `ReaderReadLine` | `arg0:AccessOperand, arg1:AccessOperand` |
| 248 | `ReaderStdin` | None |
| 249 | `RegexCaptures` | `regex:AccessOperand, text:AccessOperand, out:SlotRef` |
| 250 | `RegexCompile` | `pattern:AccessOperand, out:SlotRef` |
| 251 | `RegexFind` | `regex:AccessOperand, text:AccessOperand, start:AccessOperand, out:SlotRef` |
| 252 | `RegexFindAll` | `regex:AccessOperand, text:AccessOperand, out:SlotRef` |
| 253 | `RegexGroupCount` | `regex:AccessOperand` |
| 254 | `RegexGroupIndex` | `regex:AccessOperand, name:AccessOperand` |
| 255 | `RegexIsMatch` | `regex:AccessOperand, text:AccessOperand` |
| 256 | `RegexReplace` | `regex:AccessOperand, text:AccessOperand, repl:AccessOperand, all:bool` |
| 257 | `RegexSplit` | `regex:AccessOperand, text:AccessOperand, out:SlotRef` |
| 258 | `RenameNoReplace` | `source:AccessOperand, destination:AccessOperand` |
| 259 | `ResourceBorrow` | `owner:AccessOperand, resource:TypeRef` |
| 260 | `ResourceFromRaw` | `raw:AccessOperand, resource:TypeRef, parent:optional<AccessOperand>, abort_on_null:bool` |
| 261 | `ResourceIntoRaw` | `owner:AccessOperand, resource:TypeRef` |
| 262 | `ResourceRaw` | `reference:AccessOperand, resource:TypeRef` |
| 263 | `ResourceViewFromRaw` | `owner:AccessOperand, ptr:AccessOperand, len:AccessOperand, resource:TypeRef, view:AccessResourceViewKind, allow_null_if_empty:bool, check_nonnegative_len:bool, check_alignment:u32, check_utf8:bool` |
| 264 | `ResultErr` | `arg0:AccessOperand` |
| 265 | `ResultIsOk` | `arg0:AccessOperand` |
| 266 | `ResultOk` | `arg0:AccessOperand` |
| 267 | `ResultUnwrapErr` | `arg0:AccessOperand` |
| 268 | `ResultUnwrapOk` | `arg0:AccessOperand` |
| 269 | `RunBytesView` | `out:AccessOperand, err:bool` |
| 270 | `RunOutputView` | `out:AccessOperand, err:bool` |
| 271 | `Select` | `cond:AccessOperand, a:AccessOperand, b:AccessOperand` |
| 272 | `SliceIndex` | `arg0:AccessOperand, arg1:AccessOperand` |
| 273 (reserved; reject) | `SliceIndexNoalias` | Projects to SliceIndex tag 272; no native scope id is serialized. |
| 274 | `SliceLen` | `arg0:AccessOperand` |
| 275 | `SlicePtr` | `arg0:AccessOperand` |
| 276 | `SoaAlloc` | `handle:AccessOperand, len:AccessOperand, struct_id:TypeRef` |
| 277 | `SoaColumn` | `base:SlotRef, struct_id:TypeRef, field:u32` |
| 278 | `SoaGather` | `base:AccessOperand, index:AccessOperand, struct_id:TypeRef` |
| 279 | `SpawnTask` | `tg:AccessOperand, closure:AccessOperand, capture_tys:sequence<TypeRef>, r:TypeRef, fallible:bool` |
| 280 | `SqliteCallbackDescriptor` | `arg0:AccessSqliteCallback` |
| 281 (reserved; reject) | `StaticData` | Native-materialization-only. No public access payload; source-program derivation and wire decoding reject this tag. |
| 282 | `StaticDescriptorView` | `ptr:AccessOperand, offset:u32` |
| 283 | `StrClone` | `arg0:AccessOperand` |
| 284 | `StrFinderFind` | `plan:AccessOperand, haystack:AccessOperand` |
| 285 | `StrFinderNew` | `needle:AccessOperand` |
| 286 | `StrLit` | `arg0:utf8_data` |
| 287 | `StrPredicate` | `kind:StrPredKindTag, haystack:AccessOperand, needle:AccessOperand` |
| 288 | `StrTrim` | `kind:StrTrimKindTag, recv:AccessOperand` |
| 289 | `SubSlice` | `base:AccessOperand, start:AccessOperand, len:AccessOperand, elem:TypeRef` |
| 290 | `TcpAccept` | `listener:AccessOperand, out:SlotRef` |
| 291 | `TcpConnect` | `host:AccessOperand, port:AccessOperand, timeout_ns:AccessOperand, out:SlotRef` |
| 292 | `TcpListen` | `host:AccessOperand, port:AccessOperand, out:SlotRef` |
| 293 | `TcpReadTimeout` | `conn:AccessOperand, ns:AccessOperand` |
| 294 | `TcpWriteTimeout` | `conn:AccessOperand, ns:AccessOperand` |
| 295 | `Template` | `arg0:sequence<AccessTemplatePiece>, arg1:optional<AccessOperand>` |
| 296 | `TemplateHtmlNew` | `resource:TypeRef` |
| 297 | `TemplateHtmlRaw` | `resource:TypeRef, output:AccessOperand, value:AccessOperand` |
| 298 | `TemplateHtmlToString` | `resource:TypeRef, output:AccessOperand` |
| 299 | `TemplateHtmlWrite` | `resource:TypeRef, output:AccessOperand, value:AccessOperand` |
| 300 | `TgBegin` | None |
| 301 | `TgWaitResult` | `tg:AccessOperand, fallible:bool` |
| 302 | `TimeFormat` | `kind:TimeFormatKindTag, ns:AccessOperand, out:SlotRef` |
| 303 | `TimeInstant` | None |
| 304 | `TimeNow` | None |
| 305 | `TimeParse` | `kind:TimeFormatKindTag, input:AccessOperand, out:SlotRef` |
| 306 | `TimeSleep` | `ns:AccessOperand` |
| 307 | `TupleIndex` | `tuple:AccessOperand, index:u32` |
| 308 | `UdpBind` | `host:AccessOperand, port:AccessOperand, out:SlotRef` |
| 309 | `UdpRecvFrom` | `sock:AccessOperand, buffer:AccessOperand` |
| 310 | `UdpSendTo` | `sock:AccessOperand, data:AccessOperand, host:AccessOperand, port:AccessOperand` |
| 311 | `Un` | `arg0:UnOpTag, arg1:AccessOperand` |
| 312 | `Use` | `arg0:AccessOperand` |
| 313 | `Utf8Valid` | `data:AccessOperand` |
| 314 | `VecDot` | `a:AccessOperand, b:AccessOperand, elem:TypeRef, n:u32` |
| 315 | `VecExtract` | `vec:AccessOperand, lane:u32, elem:TypeRef` |
| 316 | `VecInsert` | `vec:AccessOperand, value:AccessOperand, lane:u32` |
| 317 | `VecLoad` | `slice:AccessOperand, index:AccessOperand, elem:TypeRef, n:u32, align:optional<u32>` |
| 318 | `VecMinMax` | `vec:AccessOperand, elem:TypeRef, n:u32, max:bool` |
| 319 | `VecSum` | `vec:AccessOperand, elem:TypeRef, n:u32` |
| 320 | `VecSumWhere` | `vec:AccessOperand, mask:AccessOperand, elem:TypeRef, n:u32` |
| 321 | `WriterCreate` | `path:AccessOperand, out:SlotRef` |
| 322 | `WriterCreateExclusive` | `path:AccessOperand, out:SlotRef` |
| 323 | `WriterCreateExclusiveBeneath` | `root:AccessOperand, relative:AccessOperand, out:SlotRef` |
| 324 | `WriterFlush` | `arg0:AccessOperand` |
| 325 | `WriterStd` | `fd:i32, buffered:bool` |
| 326 | `WriterWrite` | `arg0:AccessOperand, arg1:AccessOperand` |
| 327 | `WriterWriteBuilder` | `arg0:AccessOperand, arg1:AccessOperand` |
| 328 | `XmlAttributeCount` | `arg0:AccessOperand` |
| 329 | `XmlAttributeName` | `reader:AccessOperand, index:AccessOperand` |
| 330 | `XmlAttributeValue` | `reader:AccessOperand, index:AccessOperand` |
| 331 | `XmlName` | `reader:AccessOperand` |
| 332 | `XmlNext` | `reader:AccessOperand, event_enum:TypeRef` |
| 333 | `XmlParse` | `input:AccessOperand, error_enum:TypeRef, cleanup:ValueRef` |
| 334 | `XmlText` | `reader:AccessOperand` |

## Compound access records and discriminator tags

Each compound record uses the envelope's scalar, sequence, optional and reference
encodings. Box indirection in the Rust representation adds no wire field. Fields
are listed in exact wire order.

The two source-semantic Rvalue extensions use `u16` tags after the unchanged
baseline inventory. Native tags remain reserved and reject in the source codec.

| Extension tag | Record | Exact payload order |
| --- | --- | --- |
| 335 | `DbDescriptor` | `witness:AccessDbBridgeWitness` (Query or Command only), `driver:DbDriverTag`, `options:sequence<AccessDbStaticOption>` |
| 336 | `DbBridgeCall` | `bridge:AccessDbBridgeRecord`, `callee:AccessOperand`, `args:sequence<AccessOperand>`, `param_tys:sequence<TypeRef>`, `ret_ty:TypeRef`, `signature:AbiRef` |

`AccessDbBridgeRecord` and its witness encoding are the operation/witness
subrecord fixed above. `DbDriverTag` is one byte: 0 AnySupportedDriver,
1 SQLiteOnly, 2 PostgreSQLOnly. All other values reject. The constructor witness
fixes its result type; the bridge's selected signature fixes parameter modes,
return roots and cleanup. A valid tag never substitutes for source formation or
operation/ABI validation.

`AccessDbStaticOption` reuses plan 17's existing exact option encoding, including
its two one-byte owner/variant fields: Common/0 carries one CheckPolicy byte
(0 DeclaredOnly, 1 CheckedOptional, 2 CheckedRequired); SQLite/0 carries major,
minor and patch as three u32le fields; PostgreSQL/0 carries parameter name then
canonical type name as two length-prefixed UTF-8 strings. Owners are 0 Common,
1 SQLite and 2 PostgreSQL. No other owner/variant is admitted. The sequence is
strictly ordered by complete encoded option bytes and contains the effective
Common/Check entry exactly once. Length prefixes participate in PostgreSQL
payload ordering; ordinary UTF-8 lexical name order is not equivalent. Use the
existing pure option/Params validators for name, embedded-NUL, duplicate,
driver-scope and canonical type rules; decode must not silently sort or insert
missing defaults into a noncanonical wire record.

| Compound record | Fields |
| --- | --- |
| AccessDirectCallWithCleanup | `target:AccessAlignTarget`, `args:sequence<AccessOperand>`, `cleanup:ValueRef` |
| AccessIndirectCallWithCleanup | `callee:AccessOperand`, `args:sequence<AccessOperand>`, `param_tys:sequence<TypeRef>`, `ret_ty:TypeRef`, `signature:AbiRef`, `cleanup:ValueRef` |
| AccessArgon2 | `password:AccessOperand`, `salt:AccessOperand`, `m_cost:AccessOperand`, `t_cost:AccessOperand`, `parallelism:AccessOperand`, `len:AccessOperand`, `out:SlotRef` |
| AccessSignatureJwk | `algorithm:SignatureAlgorithmTag`, `first:AccessOperand`, `second:optional<AccessOperand>`, `out:SlotRef` |
| AccessSignatureVerify | `algorithm:SignatureAlgorithmTag`, `key:AccessOperand`, `message:AccessOperand`, `signature:AccessOperand`, `out:SlotRef` |
| AccessSqliteCallback | `target:AccessAlignTarget`, `signature:AbiRef`, `effect:FnEffectTag`, `family_version:u32` |
| AccessParMapStage | `kind:AccessParMapStageKind`, `func:optional<AccessAlignTarget>`, `captures:sequence<AccessOperand>`, `capture_tys:sequence<TypeRef>`, `elem_in:TypeRef`, `elem_out:TypeRef` |
| AccessOwnedJsonPlan | `root:TypeRef` |

AccessOwnedJsonPlan's root must be a nominal struct satisfying the existing owned
JSON grammar. The complete definition graph is already in CanonicalTy. Validate
the actual MIR's `OwnedJsonGraphPlanV3` against the same producer-owned rebuilt
plan before deriving this root; do not serialize its process-local record ids or
trust supplied field names/types. The decoded access layout rebuilds the same
plan from its validated canonical definition graph. This does not add a MIR-to-
interface crate dependency or change the existing owned-JSON wire/runtime codec.

AccessSqliteCallback folds its explicit params/modes/return/summary/cleanup facts
into one complete canonical ABI. The target ABI, inferred effect and existing
family-version contract must all agree; an effect byte is not independent proof
that the referenced function is Pure. Existing native-entry seeds and signature
validation remain prerequisites.

Tagged compound records start with a `u8` discriminator:

| Record | Exact alternatives and payloads |
| --- | --- |
| AccessParallelSource | `0 Materialized(value:AccessOperand)`; `1 VirtualChunks(base:AccessOperand,width:AccessOperand,element:TypeRef)` |
| AccessParMapStageKind | `0 Map`; `1 Filter`; `2 FilterStrContains`; `3 Project(field:u32)`; `4 FilterField(field:u32)` |
| AccessFsTreeOutput | `0 None`; `1 Owner(slot:SlotRef)`; `2 Metadata(slot:SlotRef)`; `3 Bytes(slot:SlotRef)`; `4 Bool(slot:SlotRef)`; `5 CursorNext(entry:SlotRef,present:SlotRef)` |
| AccessResourceViewKind | `0 StrUtf8`; `1 Slice(element:ScalarTypeRef)` |

The actual native StaticData still retains all byte images, target/layout and
alignment requirements, zero pointer windows, non-overlapping relocations and
consumer-specific callable ABI validation. Those are implementation/producer
inputs, not automatically an exported access record. The earlier full-byte/tree
wire representation is excluded because it violates the descriptor-private cache
contract. Use the semantic descriptor records above. Their producer-to-materialized-body qualification and compound transfer owners
are implementation acceptance under this same record.

AccessParMapStage's kind fixes function/capture presence: Map and Filter retain a
qualified target; compiler-generated field/string kinds have no arbitrary target.
Their admitted capture counts/types, element types and field selections use the
existing exact stage owner. FsTreeOutput and ColumnBatchInput similarly correlate
with the selected native operation/schema; decoding a valid tag alone does not
admit every Cartesian combination.

The following tags are each one `u8`, with precisely the alternatives listed.
Their numbers are canonical protocol values, not casts of Rust/native enum
representations. All other bytes reject before interpretation.

| Tag type | Alternatives |
| --- | --- |
| `BinOpTag` | 0 `Add`, 1 `Sub`, 2 `Mul`, 3 `Div`, 4 `Rem`, 5 `Eq`, 6 `Ne`, 7 `Lt`, 8 `Le`, 9 `Gt`, 10 `Ge`, 11 `And`, 12 `Or`, 13 `BitAnd`, 14 `BitOr`, 15 `BitXor`, 16 `Shl`, 17 `Shr` |
| `UnOpTag` | 0 `Neg`, 1 `Not`, 2 `BitNot` |
| `ArithModeTag` | 0 `Saturating`, 1 `Checked` |
| `MathFnTag` | 0 `Abs`, 1 `Min`, 2 `Max`, 3 `Sqrt`, 4 `Floor`, 5 `Ceil`, 6 `Round`, 7 `Trunc`, 8 `Pow`, 9 `Fma` |
| `SignatureAlgorithmTag` | 0 `Rs256`, 1 `Es256`, 2 `Ed25519` |
| `FnEffectTag` | 0 `Pure`, 1 `Impure`, 2 `Unknown` |
| `AeadCipherTag` | 0 `Aes256Gcm`, 1 `ChaCha20Poly1305` |
| `AeadDirTag` | 0 `Seal`, 1 `Open` |
| `CliFlagKindTag` | 0 `Bool`, 1 `Str`, 2 `I64` |
| `CodecPutKindTag` | 0 `I64`, 1 `F64`, 2 `Bool`, 3 `Str` |
| `CompressKindTag` | 0 `Gzip`, 1 `Zstd` |
| `EncodingKindTag` | 0 `Utf8Lossy`, 1 `Base64`, 2 `Base64Url`, 3 `Hex`, 4 `Percent`, 5 `PercentPath`, 6 `Form`, 7 `Html` |
| `FrameJoinKindTag` | 0 `I64`, 1 `Str` |
| `GroupOpTag` | 0 `Sum`, 1 `Min`, 2 `Max`, 3 `Count` |
| `HashAlgoTag` | 0 `Sha1`, 1 `Sha256`, 2 `Sha512` |
| `PathComponentKindTag` | 0 `Base`, 1 `Dir`, 2 `Ext` |
| `StrPredKindTag` | 0 `Contains`, 1 `StartsWith`, 2 `EndsWith`, 3 `Find`, 4 `Rfind`, 5 `EqIgnoreCase` |
| `StrTrimKindTag` | 0 `Both`, 1 `Start`, 2 `End` |
| `TimeFormatKindTag` | 0 `Rfc3339`, 1 `Rfc3339Ms`, 2 `Rfc1123`, 3 `BasicIso`, 4 `BasicDate` |
| `FsTreeKindTag` | 0 `DirectoryOpen`, 1 `DirectoryCursor`, 2 `CursorNext`, 3 `DirectoryMetadata`, 4 `DirectoryMetadataAt`, 5 `DirectoryOpenDir`, 6 `DirectoryOpenRead`, 7 `DirectoryOpenReadSingleLink`, 8 `DirectoryCreateNew`, 9 `DirectoryCreateDir`, 10 `DirectoryRemoveFile`, 11 `DirectoryRemoveDir`, 12 `DirectorySetMode`, 13 `ReaderMetadata`, 14 `WriterMetadata`, 15 `FileMetadata`, 16 `ReaderSetMode`, 17 `WriterSetMode`, 18 `FileSetMode`, 19 `DirectoryReadLink`, 20 `DirectoryMetadataFollow`, 21 `DirectoryAccess`, 22 `DirectoryAccessAt`, 23 `DirectoryCreateSymlink` |
| `ProcessLiveKindTag` | 0 `CommandNewSession`, 1 `CommandStdoutTo`, 2 `CommandStderrTo`, 3 `CommandStart`, 4 `ChildId`, 5 `ChildStatus`, 6 `ChildTryWait`, 7 `ChildReadStdout`, 8 `ChildReadStderr`, 9 `ChildPoll`, 10 `ChildKillGroup`, 11 `ChildGroupMembers`, 12 `RunOutputStatus`, 13 `RunBytesStatus`, 14 `SignalNumber`, 15 `ProcessTable`, 16 `SignalNew`, 17 `SignalNext`, 18 `SignalClose`, 19 `ScopeStart`, 20 `ScopeId`, 21 `ScopeOwnerId`, 22 `ScopeStatus`, 23 `ScopeTryWait`, 24 `ScopeWait`, 25 `ScopeReadStdout`, 26 `ScopeReadStderr`, 27 `ScopePoll`, 28 `ScopeKill`, 29 `ScopeKillGroup`, 30 `ScopeChildren`, 31 `ScopeReap`, 32 `ScopeRelease`, 33 `MemberKill`, 34 `MemberFinished`, 35 `MemoryNew`, 36 `MemoryWrite`, 37 `MemorySeal`, 38 `SealedLen`, 39 `SealedReadAt`, 40 `Executable`, 41 `ImageLen`, 42 `ImageReadAt`, 43 `CommandImage`, 44 `CurrentImage`, 45 `UserNamespace`, 46 `InheritFile`, 47 `InheritNamespace` |

`AccessTemplatePiece` also begins with a `u8` tag:

| Tag | Payload |
| --- | --- |
| 0 Static | `text:utf8_data` |
| 1 IntHole | `value:AccessOperand` |
| 2 StrHole | `value:AccessOperand` |
| 3 BoolHole | `value:AccessOperand` |
| 4 CharHole | `value:AccessOperand` |
| 5 FloatHole | `value:AccessOperand` |
| 6 JsonStrHole | `value:AccessOperand` |
| 7 OwnedJsonRecords | `value:AccessOperand`, `plan:AccessOwnedJsonPlan` |
| 8 OptionField | `value:AccessOperand`, `name:utf8_data` |
| 9 OptionStructField | `value:AccessOperand`, `name:utf8_data`, `record:TypeRef` |
| 10 PopComma | None |
| 11 StructArrayField | `value:AccessOperand`, `record:TypeRef` |
| 12 ScalarArrayField | `value:AccessOperand`, `element:ScalarTypeRef` |
| 13 UnionValue | `value:AccessOperand`, `union:TypeRef` |

Retain piece order and the exact admitted operand/type/descriptor correlation.
Static text and key names are data, not function names; they retain the existing
JSON/template escaping semantics. PopComma remains its existing template action,
not a control-flow or observation-release event. Typed text holes consume their
validation observations before publishing independent owned template bytes.
Owned/borrowed JSON subrecords use their existing exact element grammar.

These tables specify 21 scalar discriminator enums with 158 alternatives and
14 template-piece alternatives. The static/descriptor compound fields are fixed above. The envelope's staged
error order governs malformed input, each selected operation fixes contextual
field presence, and the complete vectors provide independent codec examples. The field/tag inventory
does not replace operation-specific input/output and validation-observation
transfer recipes.

## MIR inventory partition

This partition is exhaustive at the investigation baseline: 335 distinct Rvalue
variants in 25 groups, checked for both omissions and duplicate membership. A
group identifies the transfer work to close; it is not a blanket rule assigning
one origin to every result. In particular, regex matches expose byte offsets,
not borrowed text, and a copied native input does not taint its independent output.

| Transfer family | Exact current variants |
| --- | --- |
| local value / place / projection | `Use`, `Load`, `Field`, `Select`, `SoaColumn`, `BoxGet`, `Index`, `IndexField`, `IndexColumn`, `SoaGather`, `MakeTuple`, `TupleIndex`, `MakeSlice`, `SliceLen`, `SlicePtr`, `SliceIndex`, `SliceIndexNoalias`, `SubSlice`, `IndexPtr`, `IndexFieldPtr` |
| scalar arithmetic / scalar vectors | `Un`, `Cast`, `Bin`, `IntArith`, `MathOp`, `MakeVec`, `VecExtract`, `VecInsert`, `VecSumWhere`, `VecDot`, `VecMinMax`, `VecSum`, `MaskAny`, `VecLoad` |
| active tagged values | `OptionSome`, `OptionNone`, `OptionIsSome`, `OptionUnwrap`, `ResultOk`, `ResultErr`, `ResultIsOk`, `ResultUnwrapOk`, `ResultUnwrapErr`, `MakeEnum`, `MakeError`, `EnumTagEq`, `EnumPayload` |
| ordinary / indirect calls and closures | `Call`, `CallWithCleanup`, `FnAddr`, `Closure`, `CallIndirect`, `CallIndirectWithCleanup` |
| static read-only producers | `StrLit`, `ConstArray`, `StaticDescriptorView`, `SqliteCallbackDescriptor` |
| native materialization only | `StaticData`; rejected by source-access derivation and public wire decoding, retained by final native validation |
| raw / resource representation boundary | `RawCall`, `RawAlloc`, `RawNull`, `RawLoad`, `RawPointerLoad`, `RawOffset`, `RawIsNull`, `ResourceFromRaw`, `ResourceBorrow`, `ResourceRaw`, `ResourceIntoRaw`, `ResourceViewFromRaw` |
| allocation and shallow content | `ArenaBegin`, `HeapAlloc`, `ArenaAlloc`, `HeapAllocBuf`, `SoaAlloc`, `MakeDynArray`, `BoxClone`, `CloneIn` |
| column-batch native materialization only | `ColumnBatchCreate`, `ColumnBatchAppend`, `ColumnBatchRow`, `ColumnBatchSoa`; excluded from the source codec, retained by final native validation |
| group / dictionary / chunk materialization | `GroupAgg`, `GroupAggStrCols`, `GroupAggStr`, `GroupAggMultiStr`, `DictEncode`, `MakeDictEncoded`, `DictField`, `GatherColumnI64`, `DictLookup`, `Chunks`, `FrameInnerJoin` |
| synthesized task / parallel applications | `TgBegin`, `SpawnTask`, `TgWaitResult`, `ParMapParallel`, `ParMapReduce` |
| byte predicates and copied search plan | `StrPredicate`, `StrFinderNew`, `StrFinderFind`, `Utf8Valid`, `CryptoCtEqual` |
| input-derived byte subviews | `StrTrim`, `BytesAsStr`, `PathComponent` |
| fresh byte copies / encoding | `StrClone`, `Template`, `JsonEncode`, `PathJoin`, `PathNormalize`, `EncodingEncode`, `EncodingDecode`, `CompressCompress`, `CompressDecompress`, `CryptoHash`, `CryptoHmac`, `CryptoHkdf`, `CryptoAead`, `CryptoArgon2`, `CryptoSign` |
| owned builder / buffer mutation and views | `BuilderNew`, `BuilderWriteStr`, `BuilderWriteInt`, `BuilderWriteBool`, `BuilderWriteChar`, `BuilderWriteFloat`, `BuilderWriteStrIntStr`, `BuilderToString`, `TemplateHtmlNew`, `TemplateHtmlWrite`, `TemplateHtmlRaw`, `TemplateHtmlToString`, `BufferNew`, `BufferBytes`, `BufferLen`, `BufferCapacity`, `BytesRead`, `BufferPut`, `BufferAppend`, `CryptoRandom` |
| array-builder retained elements | `ArrayBuilderNew`, `ArrayBuilderPush`, `ArrayBuilderPushStr`, `ArrayBuilderAppend`, `ArrayBuilderBuild` |
| JSON / CSV input-dependent carriers | `JsonDecode`, `JsonOwnedDecode`, `JsonDecodeArray`, `JsonDecodeScalar`, `JsonDecodeStructArray`, `JsonDecodeSoa`, `CsvDecode`, `JsonDecodeUnion`, `JsonDoc`, `JsonDocKind`, `JsonDocGet`, `JsonDocAt`, `JsonDocAsStr`, `JsonDocAsScalar`, `JsonDocLen`, `JsonDocKey`, `JsonDocElems`, `JsonScanNew`, `JsonScanNext` |
| XML and codec native carriers | `XmlParse`, `XmlNext`, `XmlName`, `XmlAttributeCount`, `XmlAttributeName`, `XmlAttributeValue`, `XmlText`, `CodecOpen`, `CodecBatchRows`, `CodecBatchColumns`, `CodecBatchName`, `CodecBatchKind`, `CodecBatchFind`, `CodecBatchColumn`, `CodecColumnLen`, `CodecColumnAt`, `CodecEncoderNew`, `CodecEncoderPut`, `CodecEncoderFinish` |
| digest / signature handles | `CryptoDigestNew`, `CryptoDigestUpdate`, `CryptoDigestFinish`, `CryptoPrivateKeyFromPem`, `CryptoPublicKeyFromPem`, `CryptoPublicKeyFromJwk`, `CryptoVerify` |
| file / reader / writer / log operations | `FsReadFile`, `FsCreatePrivateTempDir`, `ReaderOpen`, `ReaderOpenBeneath`, `ReaderOpenBeneathSingleLink`, `WriterCreate`, `WriterCreateExclusive`, `WriterCreateExclusiveBeneath`, `ReaderStdin`, `WriterStd`, `ReaderRead`, `ReaderBuffered`, `ReaderReadLine`, `WriterWrite`, `WriterWriteBuilder`, `WriterFlush`, `LogNew`, `LogEnabled`, `LogLine`, `LogLineBuilder`, `LogFlush`, `IoCopy`, `FileCreateRw`, `FileOpenRw`, `FilePread`, `FilePwrite`, `FileLen`, `FsWriteFile`, `FsWriteFileBuilder`, `FsExists`, `FsRemove`, `FsCreateDir`, `FsTree`, `FsIsDir`, `FsRemoveEmptyDir`, `RenameNoReplace`, `FsReadDir`, `FsReadFileView`, `FsReadBytesView` |
| network / process / host operations | `DnsResolve`, `TcpConnect`, `ConnReader`, `ConnWriter`, `TcpReadTimeout`, `TcpWriteTimeout`, `TcpListen`, `TcpAccept`, `UdpBind`, `UdpSendTo`, `UdpRecvFrom`, `ProcessLive`, `ProcessSpawn`, `ChildWait`, `ChildKill`, `ProcessExec`, `EnvGet`, `EnvSet`, `TimeNow`, `OsHost`, `OsIdentity`, `ProcessCpuCount`, `TimeInstant`, `TimeSleep`, `TimeFormat`, `TimeParse` |
| regex offsets / owned replacement | `RegexCompile`, `RegexIsMatch`, `RegexFind`, `RegexFindAll`, `RegexSplit`, `RegexReplace`, `RegexCaptures`, `RegexGroupCount`, `RegexGroupIndex`, `CapturesGroup` |
| random source / destination / copy | `RandSeed`, `RandNext`, `RandRange`, `RandShuffle`, `RandSample` |
| CLI copied native storage | `CliCommand`, `CliFlag`, `CliParse`, `CliGetBool`, `CliGetI64`, `CliGetStr`, `CliUsage` |
| command copied native storage | `Command`, `CommandCwd`, `CommandTimeout`, `CommandMaxCapture`, `CommandEnv`, `CommandEnvClear`, `CommandRun`, `CommandRunBytes`, `RunOutputView`, `RunBytesView` |
| HTTP native storage / views / streams | `HttpRequest`, `HttpHeader`, `HttpBody`, `HttpRequestTimeout`, `HttpRequestMaxResponseBodyBytes`, `HttpClientTimeout`, `HttpClientMaxResponseBodyBytes`, `HttpParse`, `HttpRespStatus`, `HttpRespHeader`, `HttpRespBody`, `HttpClient`, `HttpClientGet`, `HttpClientPost`, `HttpClientRequest`, `HttpClientRequestStream`, `HttpReadStreamStatus`, `HttpReadStreamHeader`, `HttpReadStreamRead`, `HttpReadStreamSse`, `HttpSseStreamLastEventId`, `HttpSseStreamRetryMs`, `HttpSseStreamNext`, `HttpGetMany`, `HttpServe`, `HttpAccept`, `HttpCtxMethod`, `HttpCtxPath`, `HttpCtxHeader`, `HttpHeadersCount`, `HttpHeadersTokensValid`, `HttpHeadersContainsToken`, `HttpCtxUpgradeReady`, `HttpCtxBody`, `HttpResponseBuilder`, `HttpRbHeader`, `HttpRbBody`, `HttpRespond`, `HttpRespondStream`, `HttpRespondUpgrade`, `HttpUpgradeReadExact`, `HttpUpgradeWrite`, `HttpUpgradeDeadline`, `HttpUpgradeShutdown`, `HttpStreamSend`, `HttpStreamFinish`, `HttpStreamReject` |

The baseline statement inventory has 27 variants; the proposed source-use event
variant below is not yet implemented. `Let` derives the selected value
transfer. `Store`, `StoreField`, `StoreIndex`, `StoreElemField`,
`StoreElemFieldPtr`, `StoreColumn`, `PtrStore`, `PtrStoreNoalias` and `VecStore`
are safe storage actions whose destination authority and contained-value transfer
are separate. `StoreConstArray` copies static initializer elements into a local
fixed-array slot: the destination storage remains writable, and string element
views retain their read-only bytes. It is not equivalent to `ConstArray`.

`DropFlagInit`, `NullTupleField`, `NullStructField` and `NullElemField` alter
selected descriptor state without granting new ownership or readable backing.
`ArenaEnd`, `RawFree`, `ColumnBatchDrop`, `Drop`, `DropElem`, `DropElemField`
and `DropValue` retain existing lifecycle validation. `ColumnBatchFinish` must
preserve the native carrier's exact contained views. `TgWait` and `TgEnd` consume
the existing task-control contract. `BorrowedElementReservation` remains an inert
reservation marker. `RawStore` alone is the explicit unsafe raw write action;
ordinary indexed writes inside an unsafe block do not become RawStore.

All five terminators (`Goto`, `Branch`, `Return`, `ReturnWithCleanup`,
`Unreachable`) retain actual control order. Cleanup bits do not confer access
authority. Runtime targets of `Call`, native operation-kind discriminators and
parallel stage kinds require closed secondary inventories; this top-level count
is not evidence that those records have been covered.

The direct runtime-call whitelist is narrower than `RuntimeKey::ALL`: printing
(`Print`, `PrintStr`, `PrintBool`, `PrintChar`, `PrintF32`, `PrintF64`) and
`Hash64`/`Hash128` only read inputs and return no view-bearing value.
`ProcessExit`, `ProcessAbort`, `DivFail`, `BoundsFail`, `Utf8BoundaryFail`,
`LenMismatchFail` and `RangeFail` do not return. The existing
`direct_runtime_key_is_valid` rejects every other runtime key as a direct MIR
call. Native Rvalue contracts, rather than arbitrary runtime names, qualify the
other operations. Preserve that whitelist and test its agreement with the new
access classifier.

## Exact native transfer records

### Buffer storage and publication

The Buffer producer owns writable byte storage. This authority comes from the
actual native Buffer implementation and its closed type formation, not from a
general rule that every Move handle owns writable views. `BufferBytes` copies the
cached pointer established by an exclusive storage update; it does not create an
exclusive Rust reference during shared getter calls. The pointer/storage owner
landed before this capability and remains a prerequisite.

| Rvalue | Retained state and publication |
| --- | --- |
| BufferNew | Publish a fresh Buffer handle with logical length zero. The read-window capacity is the admitted request or zero after non-positive/invalid size or failed reserve; no caller view is retained. A zero-length buffer supplies no readable/writable element merely because its descriptor permission is Writable. |
| BufferBytes | Publish a byte descriptor for the current initialized prefix of that Buffer's backing, with Writable authority. Repeated getters alias the same current backing. Preserve its owner/generation lifetime; reading the descriptor does not itself end observations. Reverse observers are not forward validity requirements of raw bytes. |
| BufferLen / BufferCapacity | Return scalar logical length or read-window capacity from the same current Buffer state. Do not confuse initialized length with reserved capacity or publish a byte view. |
| BufferPut | Append the typed scalar's width in the selected byte order, update logical length and capacity, and preserve the native storage-update/generation contract. No caller view is retained. Both endian choices have the same access effect. End the observations/lifetimes required by the existing source-visible Buffer mutation action. |
| BufferAppend | Read the completed byte source and copy its bytes into the destination Buffer before publishing its new length. Retain no source descriptor or validation observation in the copied bytes. Self-overlap follows the existing snapshot-before-truncate/growth native path; the access pass must not turn this supported copy into a no-alias requirement. Preserve destination mutation and subsequent getter alias/generation rules. |

BufferPut/Append can grow or replace native storage. They do not grant a stale
pre-growth view a new lifetime, and a later getter cannot revive an old ended
validation observation. Buffer Drop releases its own storage exactly once under
the existing lifecycle owner. Reader/HTTP/file/random operations that fill a
Buffer use that same writable storage producer, but their status, partial-write
and reset behavior require their own action records; this table does not assign
them a fresh independent result buffer by default.

### Reader/file/random Buffer actions

These actions take the existing Buffer handle, not an arbitrary byte slice. Their
ordinary typed formation, live-handle and destination-place requirements remain
prerequisites. Consume completed arguments in source order, apply the existing
source-visible Buffer mutation effect, and publish scalar/status and Buffer state
together. Error and EOF alternatives cannot preserve an old nonempty prefix by
default. Raw byte contents acquire no UTF-8/codec observation until the caller
performs the corresponding explicit validation.

| Rvalue | Reached outcomes for valid handles |
| --- | --- |
| ReaderRead | Fill the existing Buffer's read window; nonnegative `n` publishes exactly the initialized prefix of length `n`, including zero at EOF or zero capacity. A retained buffered-reader lookahead is copied first, into independent destination storage. Window-preparation failure or non-interrupted I/O error publishes length zero and a negative status. EINTR retries within the same action. |
| ReaderReadLine | Clear the destination, accumulate copied line-body bytes, and publish its actual body length on success. The nonnegative return counts consumed stream bytes including the terminator, so it is not interchangeable with BufferLen. True EOF publishes length zero; a final unterminated line publishes its body. Line-limit rejection or refill error clears the destination and returns a negative status. The reader's retained surplus stays in the reader, not as a view into the caller Buffer. |
| FilePread | A negative offset terminates before window preparation. Otherwise publish the read count and exact initialized prefix together; EOF/zero capacity publishes length zero, and preparation/I/O error publishes zero length with a negative status. EINTR retries. The file offset operation does not retain a Buffer view. |
| CryptoRandom | Fill the existing full read-window capacity and set logical length to that capacity; zero capacity publishes length zero. CSPRNG failure has no normal continuation. This is an in-place Buffer action with unit result, not a producer of another owned Buffer. |
| ReaderBuffered | Consume the reader binding and return the same native reader identity, with its lookahead state enabled. Preserve the existing source nulling and exactly-one-owner cleanup. The type remains Reader; the checked buffered-reader fact does not allocate a new abstract owner identity. Re-buffering preserves existing lookahead. Its internal lookahead does not retain a caller byte/text descriptor. |

The native owners are `align_rt_io_reader_read`, `align_rt_io_reader_read_line`,
`align_rt_io_file_pread`, `align_rt_crypto_random`, `Buffer::prepare_uninit_window`
and the buffered-reader constructor; MIR's `lower_count_or_status_result` owns
the signed-status result envelope. Their defensive null-handle ABI paths do not
qualify an otherwise malformed source/MIR handle. Runtime allocation and retry
behavior stays unchanged. Owners must cover nonempty success, EOF, zero window,
preparation/error reset and old getter/validation aliases, with ReaderReadLine's
terminator count separated from its published body length.

### Frame join publication

`FrameInnerJoin` reads two validated codec columns and `max_pairs` in their
existing order. Both I64 and Str alternatives consume the columns' codec
observations; Str also consumes the key text. The runtime's index/hash tables are
temporary implementation storage. On status zero, the output is a fresh ordinary
owned AoS array of `RowPair { left: i64, right: i64 }`, allocated with the existing
runtime allocator and filled through its mutable pointer. Its outer storage is
writable and its scalar elements retain no input view or validation observation.
The later application of those indices to a source column is a separate checked
source read.

The output starts as `{null, 0}`. No matches or an empty right input returns that
empty success. Status -1 maps to InvalidLimit and -2 to LimitExceeded; neither
publishes an array payload. Defensive positive native statuses have no normal
source continuation under `lower_frame_inner_join`'s abort arm. A count/fill
inconsistency frees the candidate allocation before aborting, rather than
publishing a partial result. Source-event completion/frontier rules remain
separate from the result's own independent profile; an output record cannot
silently retire an enclosing eager source snapshot.

The owners are `frame_inner_join_impl`, the i64/str v1 native entrypoints,
`lower_frame_inner_join`, the existing `pkg_frame` suite and new call-boundary
observation controls. Keep I64/Str, empty/nonempty, invalid/exceeded limit and
later input use/output-only use as explicit transfer alternatives.

### Regex and repeated string search transfers

The runtime owners are `align_runtime::align_rt_regex_*` and
`align_rt_str_finder_*`; LLVM's corresponding Rvalue arms own output-slot
initialization and status-to-source-result construction. These records separate
stored offsets from borrowed text. Each reached text operand has its source-use
and eager-completion checks before the action. The action itself does not mutate
source text or its validation observations.

| Rvalue | Retained state and publication |
| --- | --- |
| RegexCompile | Read the pattern and create an independent compiled handle on status zero. Invalid pattern, ABI shape or resource limit publishes no handle and leaves the output null. The compiled handle retains no pattern view or caller validation observation. |
| RegexIsMatch | Read the compiled handle and text; return a scalar Boolean. Retain no text descriptor. |
| RegexFind | Read handle/text; success writes one scalar `regex_match` offset pair, absence publishes no match. Neither result alternative contains a text view. |
| RegexFindAll | Replace the output with a fresh owned `array<regex_match>` of scalar offset pairs; an empty result has no element profile. The outer array has its ordinary writable storage. Runtime status is zero; defensive invalid ABI inputs leave the preinitialized empty array. No source text view or observation is retained in the array. |
| RegexSplit | The same owned offset-array transfer as RegexFindAll, with between-match spans. This is not an array of borrowed substrings. For valid empty text, the result contains one empty span; callers create any actual substring through their later ordinary text-slice operation. |
| RegexReplace | Read handle/text/replacement for both `all` values. Publish independent owned text, including copying the no-match borrowed-result alternative. An empty result has no allocated byte backing. All resulting text-to-byte views are read-only and detached from input validation observations. |
| RegexGroupCount | Read only the compiled handle; scalar count, no contained source view. |
| RegexGroupIndex | Read handle/name; scalar group index or missing sentinel. Retain no name view. |
| RegexCaptures | On a match, publish a fresh handle containing only copied integer span pairs; a missing group has the existing negative sentinel. No match publishes no handle and keeps the output null. The handle retains no source text view or validation observation. |
| CapturesGroup | Read the stored span pair. A participating group publishes a scalar match, a missing group publishes none, and an invalid group index has no normal continuation. No text view is created. |
| StrFinderNew | Copy the needle into the independently owned finder plan (`Finder::into_owned`). The plan retains no needle descriptor or validation observation. |
| StrFinderFind | Read the plan's copied needle and the supplied haystack; scalar match/missing result. Haystack validation is consumed at this action, not stored in the plan. |

Plan/regex/capture Drop frees only its own native state under the existing owner;
it cannot end an input text observation that was not retained. A later caller
slice using returned offsets must still check its actual source text. Owners
must pair the offset-only handle/array cases with that later text use, and pair
RegexReplace's no-match copy with an overlapping input-byte write. The restricted
native ABI's defensive null cases are not evidence that malformed MIR may bypass
the normal typed producer checks.

### HTTP response copies and receive publication

| Rvalue | Retained state and publication |
| --- | --- |
| HttpRequest | Copy method and URL into a fresh request builder; no caller string descriptor is retained. |
| HttpHeader | Read name/value, preserve their existing validation order and append independent copied header strings to the request. Invalid header syntax terminates; no caller view is retained. |
| HttpBody | Copy the byte source into request-owned storage, replacing the prior body. Empty input still marks a present body. Mutating the request does not mutate the original byte backing. |
| HttpParse | Consume a complete response byte view and parse into independently owned native response storage. Success publishes a fresh handle; malformed/truncated/framing/limit failure leaves out null. Header offsets and decoded body refer to native-owned copies, not the caller's input view. |
| HttpRespStatus | Read the response and return an independent scalar status. |
| HttpRespHeader | Lookup name without retaining it. Some publishes ReadOnly text over response-owned header backing; None publishes no text payload. Repeated getters may alias. |
| HttpRespBody | Publish a ReadOnly byte descriptor over response-owned ordinary or bounded body storage; both alternatives and empty bodies have the same permission. Keep response lifetime and repeated-getter aliasing, not caller parse-input lifetime. |
| HttpReadStreamStatus | Read the final-head status and return an independent scalar. |
| HttpReadStreamHeader | Read name and final head; Some publishes ReadOnly text over the stream decoder's header storage, borrowing the stream. Lookup name is not retained, and None has no text payload. |
| HttpReadStreamRead | Mutate the existing caller Buffer and stream cursor. Clear count/logical length before the native read. Success publishes exactly the initialized written prefix and count together, including EOF zero. Error leaves count/length zero; any staged bytes are not a published prefix. Existing positive-capacity/source-state admission and terminal-error behavior remain prerequisites. |
| HttpReadStreamSse | Consume raw-stream ownership into the existing SSE type, preserving the same native pointer and dependent-client provenance. No allocation or I/O is added by the transition. Source nulling and exactly-one-owner cleanup remain mandatory. |
| HttpSseStreamLastEventId | Publish ReadOnly text over the stream-owned committed-id vector, with the stream cursor/owner lifetime. This backing is distinct from the id field copied into the caller Buffer by next. |
| HttpSseStreamRetryMs | Publish an independent optional scalar from committed stream state; the native negative sentinel means None. |
| HttpSseStreamNext | Mutate stream and caller Buffer, then publish either one complete event or no event. Some(event) has event/data/last_event_id text slices into the same newly initialized caller Buffer prefix and an inline optional retry scalar. Attach a fresh UTF-8 observation to exactly those three text leaves, on the post-action Buffer backing. Text-to-bytes remains ReadOnly while a separate BufferBytes getter remains Writable and can end that observation. None/error publishes no text payload; the native output envelope and logical Buffer length are zero on those alternatives. |

The native response decoder owns its copied input and framing state; Buffer window
publication uses MaybeUninit storage and publishes only initialized bytes. SSE next
copies decoded event/data/id into that caller window before forming its text views.
Plan 57's implemented local UTF-8 producer covers this native validation site
as well as explicit BytesAsStr. Interprocedural replay must preserve that
post-action observation instead of forming one from an older argument snapshot. SSE cursor and
Buffer generation lifetime checks remain separate from this byte observation.
An ordinary next call cannot reuse an older observation identity as newly Live.
The scalar retry leaf and absent event/error alternatives acquire no text profile.

Owners are the corresponding `align_rt_http_*` entrypoints, the response decoder,
`HttpSseState` publication and the `http_sse_stream`/local observation suites.
Input-copy versus native-getter identity, body storage alternatives, stream-header
lifetime, repeated next, copied/early-used text and raw Buffer alias writes are
separate acceptance cells. The following client and server tables supply their
other operation records; native callback application retains its separate
closed-entry contract. No Http handle is qualified by shell ownership alone.

### HTTP client completion and batch capture

| Rvalue | Retained state and publication |
| --- | --- |
| HttpClient | Create a fresh native client with empty connection/request-buffer pools and default scalar options. Its internal synchronization grants no arbitrary inner-view writability. |
| HttpClientTimeout / HttpClientMaxResponseBodyBytes | Update the client's scalar default through the existing native owner. Invalid input retains the native termination contract. No caller view is retained or mutated. |
| HttpRequestTimeout / HttpRequestMaxResponseBodyBytes | Update the request's scalar override. Preserve the existing default/override validation semantics and do not publish a byte view. |
| HttpClientGet / HttpClientPost | Read client, URL and POST body through the scoped exchange. Success publishes an independent complete response handle; errors leave out null. URL/body views are not retained in the response or pool. Connection reuse remains native state, not a caller-byte alias. |
| HttpClientRequest | Consume the completed request handle, including its failure cleanup, and publish an independent complete response only on success. Preserve source nulling and the existing native moved-request release on early exits. |
| HttpClientRequestStream | After the existing valid-handle preconditions, consume request ownership and publish a dependent stream on success, or no stream on failure. Retain the client's owner provenance in the stream, not the consumed request's input strings. The later stream body/header actions use their own records. |
| HttpGetMany | Copy input URLs into independent scoped request storage before workers run. Publish a fresh owned header array of independently owned response handles in input order only when every exchange succeeds. On any exchange failure, wait for the scoped workers, free successful responses and publish no array, preserving the lowest-input-index error. Invalid concurrency terminates; an empty URL collection succeeds empty. |

The GetMany header allocation uses the ordinary runtime allocator and mutable
handle-slot stores before publication, so its outer descriptor is Writable.
Contained response handles retain their distinct ownership and every header/body
getter remains ReadOnly. Moving/replacing an element uses the existing owned
response-array cleanup; a copied header cannot duplicate response ownership.
Neither GetMany nor a complete single response becomes client-borrowed merely
because its socket originated in the pool. The dependent raw/SSE stream does keep
its existing client lifetime. No task result escapes before scoped workers finish.

The owners are `align_rt_http_client_*`, `align_rt_http_get_many`, the shared
`http_client_perform` exchange and Sema's explicit HttpClientRequestStream
client-root mapping. Acceptance cells distinguish copied request inputs,
consumed requests, independent complete responses, client-dependent streams,
empty/full/failed batches and returned response getter views. Their existing
network/resource limits and pool synchronization are prerequisites, not new
performance or resource promises of this analysis.

### HTTP server views and response ownership

| Rvalue | Publication and reached continuation |
| --- | --- |
| HttpServe | Read host and port, bind the selected ordinary/shared listener and publish a fresh owned server only on success. Host bytes are not retained. Native parked-connection state does not confer caller-byte authority. |
| HttpAccept | Read/mutate the server's connection state and publish an owned request context with its own parse buffer. The context owns its connection and shares the parking-control Arc; it does not borrow the server's parse storage. Failed publication yields no context. |
| HttpCtxMethod / HttpCtxPath / HttpCtxHeader | Publish ReadOnly text into the context parse buffer; optional header absence has no payload. Header lookup consumes the name without retaining it. The headers view has the same context identity, not a separately owned resource. |
| HttpCtxBody | Publish a ReadOnly byte descriptor into the same context buffer, including empty bodies. It retains the context lifetime and never acquires Buffer-style Writable authority. |
| HttpHeadersCount / HttpHeadersTokensValid / HttpHeadersContainsToken / HttpCtxUpgradeReady | Read context/header state and any name/token operands, then publish independent scalars. Preserve the exact-token discriminator and existing validation/lookup behavior; no input view is retained by the scalar. |
| HttpResponseBuilder | Allocate an independent builder containing the supplied status scalar. Status validation belongs to response serialization; this constructor does not add an earlier rejection. |
| HttpRbHeader / HttpRbBody | Read source views and copy their bytes into builder-owned storage. Header validation retains its existing termination behavior. Neither operation retains caller byte descriptors. |
| HttpRespond | Consume both context and builder before native early-return paths. Serialize/send or fail, then release both owners, with the existing connection close/parking behavior. Publish only status, not a view into either consumed owner. |
| HttpRespondStream | Borrow the context and consume the builder. On success, move the connection into a fresh stream with an independently serialized pending head; the spent context keeps its parse buffer and existing views. On failure, publish no stream and preserve the native distinction between an unspent context and a connection already transferred. |
| HttpRespondUpgrade | Borrow context and consume builder after the existing output/handle checks. Validate and allocate before moving the fd. Success publishes a fresh upgrade owner after sending the handshake; failures after fd transfer close through upgrade cleanup, while earlier failures leave the context unspent. Context parse-buffer views remain context-owned. |
| HttpUpgradeReadExact | Mutate the existing output Buffer and upgrade state. For a live operation, reset logical length before reading; success publishes exactly count initialized bytes. Partial read failures publish no initialized prefix and poison/close the upgrade. Invalid count terminates before reset, and already spent/poisoned handle rejection preserves the earlier output state. The access effect conservatively ends overlapping observations whenever this source mutation is reached. |
| HttpUpgradeWrite | Read bytes during the synchronous write without retaining or modifying them. Mutate the upgrade's deadline/poison/connection state as required; status has no byte dependency. |
| HttpUpgradeDeadline / HttpUpgradeShutdown | Mutate scalar/native connection state, retaining the existing bounds, sticky error and idempotent shutdown behavior. Publish only status. |
| HttpStreamSend | Borrow and mutate stream state; read chunk bytes without retaining their descriptor. The event discriminator selects existing SSE formatting. Plain empty send is a no-op and retains the pending head; other sends can commit the head and poison on failure. No caller byte writes occur. |
| HttpStreamFinish | Consume stream on every valid-handle completion, write any pending head/terminator as permitted by poison state, then close. Publish status only. |
| HttpStreamReject | Consume both stream and replacement builder before early returns. Replace only a still-pending head; any rejection or write failure releases both owners. No descriptor into serialized builder/stream storage escapes. |

The server's parking Arc outlives server destruction when an accepted context
still owns a reference; a dead parking slot causes close rather than parking.
This native ownership does not introduce a source borrow from context to server.
Conversely, every context getter keeps its existing context borrow. Respond
consumes that owner, while RespondStream and RespondUpgrade preserve its parse
buffer after spending the connection. Those operations cannot share one blanket
consume-all-operands rule.

The producer owners are `align_rt_http_accept`, `align_rt_http_ctx_*`,
`align_rt_http_respond*`, `align_rt_http_upgrade_*`, `align_rt_http_stream_*`
and the matching MIR moved-slot nulling paths. Acceptance cells distinguish
context lifetime from connection state, copied builder inputs, borrowed context
versus consumed builder, pre-transfer and post-transfer errors, exact-read partial
failure, empty send and all terminal cleanup paths. These records preserve the
existing networking/error contract and make no new transport performance promise.

### File contents and directory materialization

| Rvalue | Publication and reached continuation |
| --- | --- |
| FsReadFile | Read the path and publish a fresh owned string only after complete I/O and UTF-8 validation. Both direct-read and fallback-copy implementations produce independent owned text; failure publishes no string payload. Path storage is not retained. |
| FsReadDir | Read path and publish a fresh owned array of independently owned name strings. OwnedStringList stages name allocations and transfers them to a fresh mutable header allocation only on complete success. A mid-enumeration/size error frees staged payloads and publishes no array. Non-UTF-8 names are omitted under the existing contract. |
| FsReadFileView | Read path and publish read-only text whose lifetime is the supplied arena. The regular-file path uses a PROT_READ private mapping; the fallback copies into arena memory. Both have the same ReadOnly publication contract and retain no path descriptor. UTF-8 or I/O failure publishes no view. |
| FsReadBytesView | The same arena-owned mapping/fallback alternatives, without text validation. Every successful descriptor is ReadOnly, including copy fallback and empty output; no runtime path grants the caller writable permission. A separate caller conversion is needed to publish validated text. |
| FsCreatePrivateTempDir | Consume prefix, create the directory under the existing native preconditions and publish an independent owned path string on success. Failure publishes no path payload and leaves the caller-visible header empty under its existing valid-output ABI. No prefix view is retained. |

The ordinary directory header array has Writable outer storage. Its owned string
elements retain their individual ownership, and every later string-to-byte view
is ReadOnly. Moving, replacing or dropping an element uses the existing generic
owned-array cleanup; a header copy cannot duplicate its owned string payload.
Mapped/copy-fallback views instead have the existing arena release contract and
no per-view Drop. Their read-only authority is source-visible and uniform;
it is not a runtime test of whether this invocation happened to allocate bytes.
The existing external-file stability and mapping-failure contracts remain intact.
This analysis does not claim to model filesystem identity or external file writes.

The native owners are `align_rt_fs_read_file`, `OwnedStringList::{push,
into_array, publish}`, `fs_read_view_impl`, `read_file_view_into_arena` and the
private-temp-directory producer. Acceptance owners pair ordinary owned content
with arena mappings/fallbacks, empty/nonempty results, read-only returned byte
views, directory element move/replacement and failed partial materialization.
Source argument use and eager snapshots remain separate from output independence.

### Command capture ownership and getters

| Rvalue | Retained state and publication |
| --- | --- |
| Command | Read the command path and full argv, including argv[0], then copy them into owned C-string storage in a fresh command handle. Invalid invocation terminates before a handle is published. Retain no caller text/array descriptor. |
| CommandCwd | Read and copy the directory into the existing command, replacing its prior copied cwd. Invalid path terminates; no caller view is retained. |
| CommandEnv | Read name/value and copy both into command-owned environment overrides. Preserve native name-then-value validation and later override behavior; no source backing is retained. |
| CommandEnvClear | Update the existing command's environment-clear setting; no view result or caller backing mutation. |
| CommandTimeout / CommandMaxCapture | Update scalar configuration on the command. Invalid negative input terminates. Zero timeout and zero capture limit keep their different existing meanings; neither publishes a byte descriptor. |
| CommandRun | Read the live command and capture both streams into independent native storage. Success publishes a fresh RunOutput only after both streams pass UTF-8 validation. Setup/run/capture/text-validation errors publish no result handle. The command remains live, and the result retains no input command/name/argv view. |
| CommandRunBytes | The same independent captured-stream ownership, without the text-validation step. Success publishes RunBytes; errors publish no handle. Captured bytes have no validation observation until an explicit caller conversion. |
| RunOutputView | Both err discriminator alternatives publish ReadOnly text over the selected captured stdout/stderr backing, borrowing the RunOutput owner. Repeated getters for one stream alias that backing; the two streams are distinct. An empty stream has no element, not fresh writable storage. |
| RunBytesView | Both alternatives publish ReadOnly byte descriptors over the selected RunBytes backing. The shared Rust Vec getter provides no write authority, regardless of fresh capture ownership or length. Borrow the selected owner generation and preserve repeated-getter aliasing. |

The native owners are `marshal_cmd_argv`, `align_rt_command_*`,
`run_command_capture`, and the four `align_rt_run_{output,bytes}_{stdout,stderr}`
getters. For valid ABI output slots, run methods clear the result to null before
setup; no partial capture escapes a failed result. Defensive invalid pointers
cannot be used to infer successful source initialization. Existing timeout,
capture-limit, kill/reap and Drop behavior remains unchanged. Drop of a captured
result ends its getters' backing; later mutation/drop of the independent Command
does not end it. The existing `ProcessLive` status operations return scalar
observations of the corresponding captured owner and remain separately typed.

Owners cross text/binary and stdout/stderr with empty/nonempty capture, repeated
runs, command-input copies and returned getter views. A successful explicit owned
copy of a read-only capture must permit mutation without changing another getter;
no direct returned slice may launder that original read-only permission.

### CLI copied inputs and parsed views

The `align_rt_cli_*` owners copy command names, flag names, string defaults and
parsed string values into native owned storage. Input text is consumed at the
source action, but these copies retain no caller text descriptor or validation
observation. Command and parsed handles have separate native ownership.

| Rvalue | Retained state and publication |
| --- | --- |
| CliCommand | Read and copy the name into a fresh command handle with an empty flag table. Retain no caller view. |
| CliFlag | Read command/name and the discriminator-specific default. Register copied name/default storage in the existing command. Bool has no default operand; I64 has a scalar default; Str copies its text default. The action mutates the command, not the caller's name/default backing. |
| CliParse | Read the command and reached argv entries according to the existing parser, skipping argv[0]. On success publish a fresh parsed handle whose strings and default strings are independently copied, not borrowed from argv or the command. On any parse error publish no handle and leave the output null. Repeated flags replace their prior parsed value. The command remains live on both outcomes. |
| CliGetBool / CliGetI64 | Read parsed handle and lookup name; publish an independent scalar. Unknown name or wrong kind terminates. Retain no lookup-name view. |
| CliGetStr | Read parsed handle/name and publish read-only text into that parsed handle's owned String storage. The result borrows the parsed owner, not the lookup name or original argv/default input. Repeated getters may alias that storage. Wrong kind/unknown name terminates. |
| CliUsage | Read command and render a fresh owned string. The result retains neither the command nor its original name/default inputs. Its text-to-bytes result is read-only. |

A source-level argv expression still receives its ordinary whole-value use and
eager-completion checks; the runtime's skipped program-name element is not an
exception to that recipe. Native argument-reading details must not waive those
checks. Command Drop ends its own copied storage, and parsed Drop ends the
getter views' backing. Owners cover default/explicit/repeated strings, parse
error with subsequent command use, detached original inputs, and getter use
across function return while the parsed owner remains live.

### Incremental digest transfers

`CryptoDigestNew` creates a fresh independently owned SHA-256 context.
`CryptoDigestUpdate` requires the existing exclusive digest access, consumes a
readable byte input and updates only the digest state; EVP retains no byte
pointer. Invalid length/extent, overlap with the digest shell, accumulated-length
exhaustion or provider failure has no normal continuation. The explicit native
non-overlap check remains a prerequisite, not an inferred permission for a
caller-supplied arbitrary byte view.

`CryptoDigestFinish` consumes the handle and publishes a fresh ordinary owned
32-byte array, filled through the runtime allocator's mutable pointer. Its outer
storage is Writable and retains no update-input observation. The consumed handle
is nulled under the existing Move/cleanup owner; provider failure terminates
without a returned array. Drop of an unfinished digest releases its context,
and the moved-from null slot remains harmless under the existing Drop ABI.
The owners are `crypto_digest::{align_rt_crypto_digest_new,
align_rt_crypto_digest_update, align_rt_crypto_digest_finish,
align_rt_crypto_digest_free}`. Access controls must pair repeated updates and
empty input with independent input writes after update and output mutation after
finish; none of those paths gains an input alias merely because digest output is
derived from the input's values.

### One-shot crypto and asymmetric key publication

Each input is consumed by the shared source-use/completion recipe before the
native action. None of these operations retains a caller byte/text descriptor in
its output. Their internal engine allocations and ownership remain unchanged;
this access contract introduces no cryptographic algorithm or resource promise.

| Rvalue | Publication and reached continuation |
| --- | --- |
| CryptoCtEqual | Read the byte inputs as required by the native length/equality operation and return an independent Boolean. No input mutation or view publication. Source argument checks remain required even on the differing-length short circuit. |
| CryptoHash | Sha1/Sha256/Sha512 produce fresh ordinary owned byte arrays of 20/32/64 bytes through align_rt_alloc and a mutable copy. Outer array authority is Writable; provider/length failure terminates without an array. |
| CryptoHmac | Read key/data and publish a fresh 32-byte owned array through the same allocator/copy rule. Provider failure or unexpected length terminates; no key or data view is retained. |
| CryptoHkdf | Read salt/ikm/info and requested length. Success publishes a fresh independently owned Buffer with exactly the requested initialized length. Invalid length or native error publishes no Buffer and leaves out null. Preserve native validation and its status mapping. |
| CryptoArgon2 | Read password/salt and the typed numeric options. Success publishes a fresh Buffer of the admitted requested length; validation/provider/allocation error publishes no Buffer and leaves out null. Preserve the existing parallelism, iteration, memory and output-length validation order. |
| CryptoAead | Both ciphers and both directions read key/nonce/input/aad. Seal publishes fresh ciphertext-plus-tag Buffer. Open publishes its fresh plaintext Buffer only after authentication succeeds; staged unauthenticated bytes never enter an abstract successful result. Every error publishes no Buffer and leaves out null. No input backing is modified. |
| CryptoPrivateKeyFromPem / CryptoPublicKeyFromPem | Read PEM text and create independent algorithm-specific native key ownership on status zero. Errors leave the key output null. Parsed key material retains no PEM pointer or validation observation. Existing key-kind and constructing-thread requirements remain native prerequisites. |
| CryptoPublicKeyFromJwk | Read already-decoded byte components, not source JWK text. Rs256 and Es256 require first and second operands; Ed25519 requires first only and the native absent-second representation is exactly null/zero. Success publishes independent public-key ownership; error leaves out null. Neither component is retained as a caller view. |
| CryptoSign | Read the matching private-key handle and message; success publishes a fresh owned Buffer for the signature. Errors leave out null. The key remains live, and the signature retains no message/key view. |
| CryptoVerify | Read matching public-key handle, message and signature. On status zero publish an independent Boolean; wrong signature length can return false without an engine verification. Error publishes no successful Boolean payload. The native truth slot starts false, but that initialization does not turn an error into an Ok result. |

`publish_buffer` transfers an independently built byte Vec into Buffer ownership
and supplies its cached writable getter pointer. Therefore successful Buffer
results use the Buffer publication rules above; they are not ordinary array
headers despite both representing fresh bytes. Asymmetric signing uses its
fallible buffer-owner publisher with the same ownership distinction. Hash, HMAC
and digest Finish instead return the ordinary array ABI. Key Drop frees only its
own native state; an update/sign/verify action cannot end a caller observation
merely because it reads its bytes. Existing source ownership and thread rules
still apply to the handles.

Owners are the exact `align_rt_crypto_*` entrypoints, `publish_buffer`,
`crypto_asymmetric::{key_from_pem, import_jwk, sign, verify, buffer_owner}` and
their MIR result-envelope lowering. The acceptance matrix pairs both AEAD
ciphers/directions with successful and failed publication, all key algorithm and
JWK-presence alternatives, independent source/output mutations, key reuse and
native failure paths. Defensive malformed ABI input behavior never qualifies
malformed source producers.

### Encoding, compression and copied paths

| Rvalue | Publication and reached continuation |
| --- | --- |
| EncodingEncode | Every admitted Utf8Lossy/Base64/Base64Url/Hex/Percent/PercentPath/Form/Html alternative reads its typed input and publishes independent owned text. Even unchanged valid UTF-8, unescaped HTML and unescaped path characters are copied. Text-to-bytes authority is ReadOnly; the copy retains no input observation. Preserve size-overflow/allocation termination. |
| EncodingDecode | Base64/Base64Url/Hex/Percent/Form success publishes a fresh Buffer through decode_into, including successful empty output. Invalid input publishes no Buffer and leaves out null. Utf8Lossy/PercentPath/Html are invalid decoder discriminators and reject structurally before replay. Decoded bytes have no UTF-8 observation until a separate conversion validates them. |
| CompressCompress | Gzip and Zstd consume bytes and level, then publish a fresh Buffer through publish_buffer on success. An out-of-range level terminates under the existing programmer-error contract; other native error statuses publish no Buffer and leave out null. No compressed output retains its input backing. |
| CompressDecompress | Both formats publish only a fully produced fresh Buffer on success. Corrupt, truncated, over-limit or native-error alternatives publish no Buffer and leave out null; internal partial output does not become a source result. Output bytes gain no text/codec validation merely by decompression. |
| PathJoin | Read both paths and publish independent owned text. The empty-left/empty-right alternatives explicitly clone the other operand; they are not borrowed-return shortcuts. |
| PathNormalize | Read path and publish independent owned text from the fresh mutable output allocation. Even an already normalized path is copied. Lexical normalization retains no caller view and creates no filesystem-derived ownership. |
| Utf8Valid | Read bytes and return an independent Boolean. This predicate alone neither publishes text nor installs a reusable validation observation in its input descriptor. A later BytesAsStr conversion uses its own checked successful publication. |

`owned_str_exact`, the corresponding `align_rt_*` entrypoints, `decode_into`,
`publish_buffer` and the exact LLVM discriminator mapping own these records.
Successful empty output has no readable element even when its Buffer can later
supply writable storage. The source-use/frontier recipe remains separate from
copy independence. Owners must cross encode/decode discriminator admission,
empty/unchanged/transformed inputs, failure publication, input mutation after an
owned copy and output mutation for successful byte Buffers. Existing native
format validation, limits and allocation behavior remain unchanged.

### Direct process launch and wait

| Rvalue | Publication and reached continuation |
| --- | --- |
| ProcessSpawn | Read command and every argv header/text leaf, marshal independent native C strings, then publish a fresh owned child only after setup/exec acknowledgement. Failure publishes no child. No command/argv view remains a child dependency. |
| ChildWait | Borrow/mutate child state and publish the existing independent typed termination/resource scalar record on success. Retain the existing undrained-capture and cached-result behavior; no capture byte descriptor is returned by wait. |
| ChildKill | Read/mutate the child/native signal state and return only status. No byte alias is created. |
| ProcessExec | Read and marshal command/argv. Failure returns its existing error status with no retained caller view; success has no continuation in the current process image. Do not synthesize a returning unit or discard earlier source obligations when the success path does not return. |

The marshal helper copies every argv text rather than retaining a caller header
array. ProcessLive's capture, scope, inherited-authority and byte-read operations
have the separate complete kind inventory below. Direct launch does not replace
those roles with a generic child owner shortcut. Acceptance includes invalid
nested input, failed launch publication, argv-copy independence and the explicit
nonreturning exec success path. Existing process cleanup/reaping and unsafe ABI
preconditions remain the owners of native lifetime behavior.

### File and log handle effects

| Rvalue | Publication and reached continuation |
| --- | --- |
| ReaderOpen / ReaderOpenBeneath / ReaderOpenBeneathSingleLink | Read path/root-relative inputs and publish an independently owned reader fd/shell on success. The opened reader retains no caller path bytes or directory-handle borrow. Keep the exact regular-file/beneath/single-link validation kind. |
| WriterCreate / WriterCreateExclusive / WriterCreateExclusiveBeneath | Read location inputs and publish an independent owned writer only on success, preserving truncate/exclusive/beneath distinctions. No caller path descriptor is retained. |
| ReaderStdin / WriterStd | Create the existing I/O shell over the selected process fd, preserving its native owns-fd and buffering flags. No caller view is created. |
| WriterWrite / WriterWriteBuilder | Read bytes or the builder's accumulated bytes and synchronously write or copy them into writer-owned buffering. Retain no source descriptor; the builder is borrowed, not consumed. |
| WriterFlush | Mutate writer buffer/fd state and return status, without publishing a view. |
| LogNew | Consume the writer into a new logger shell and preserve all lifetime dependencies of that writer, including any borrowed connection owner. Source nulling prevents separate writer Drop; logger Drop owns its release. |
| LogEnabled | Read scalar level/latch state and publish an independent Bool. |
| LogLine / LogLineBuilder | Preserve eager source evaluation, then the existing native latch/level gate. An enabled message is read and transformed through the owned writer; a suppressed native record does not read message bytes. Neither branch retains the source text/builder descriptor. Source-use checks already required by eager evaluation are not erased by suppression. |
| LogFlush | Flush the owned writer and expose existing first-error state. No input/result view is retained. |
| IoCopy | Read/mutate reader and writer state using independent internal transfer storage. Return scalar byte count/status; no internal Buffer descriptor escapes or aliases a caller Buffer. |
| FileCreateRw / FileOpenRw | Read path and publish an independent owned offset-I/O file handle on success. |
| FilePwrite | Read source bytes during the offset write, retain no descriptor, mutate file state and return scalar count/status. |
| FileLen | Read file metadata and return an independent scalar result. |
| FsWriteFile / FsWriteFileBuilder | Read path and supplied bytes/builder while performing the existing complete file write. Retain no input view; the builder remains caller-owned. |
| FsExists / FsRemove / FsCreateDir / FsIsDir / FsRemoveEmptyDir / RenameNoReplace | Read explicit path operands and perform the existing scalar/status OS operation. No descriptor escapes and no caller byte storage is modified. Preserve each exact operation discriminator and error behavior. |

The read-into-Buffer family has its separate mutation records above. Retained
FsTree operations use their exact secondary schema below rather than this
path-only rule. Native sink suppression, buffered copies and internal I/O scratch
cannot hide a source-evaluation event or introduce a new retained source alias.
Acceptance includes logger consumption of a connection-borrowing writer,
borrowed builder writes, suppressed/enabled logs, fd-owner independence and
read/write failure paths. Existing syscall, flush/Drop and concurrency contracts
remain unchanged.

### Network handles and owned host observations

| Rvalue | Publication and reached continuation |
| --- | --- |
| DnsResolve | Read host, complete native lookup and publish a fresh Writable array of independently owned address Strings. Native addrinfo and temporary formatting storage are released before return; no host or provider pointer is retained. Failure publishes no array. |
| TcpConnect / TcpListen / UdpBind | Read address/configuration inputs and publish an independent owned connection/listener/socket on success. No caller text descriptor is retained; failure publishes no handle. |
| TcpAccept | Use listener state and publish an independently owned connected socket. The connection does not borrow listener storage. |
| ConnReader / ConnWriter | Create an independent I/O shell over the same connection fd with no fd ownership. Preserve the existing connection lifetime in the shell; freeing the shell cannot close or independently own the connection. |
| TcpReadTimeout / TcpWriteTimeout | Mutate native connection configuration from a scalar duration. No caller bytes are retained or written. Existing exclusive/configuration and negative-duration contracts remain prerequisites. |
| UdpSendTo | Read socket, data and destination, send synchronously and return scalar count/status. No input view is retained or modified. |
| UdpRecvFrom | Mutate the supplied Buffer and socket state. Success publishes the initialized received prefix with logical length equal to the returned count, including zero capacity/empty datagrams. Native error after receive clears logical length; no successful payload is published. Preserve existing truncation and pre-admission failure behavior. The reached source Buffer action invalidates overlapping observations conservatively. |
| EnvGet | Read name and copy any present environment value into an independent owned String before publication. Absent and present-empty remain distinct; later EnvSet cannot invalidate the copied output. No getenv pointer becomes a source view. |
| EnvSet | Read name/value and copy through the existing native environment operation. Retain no caller descriptor and publish only status. Preserve the existing process-global concurrency contract; this proof does not introduce global synchronization. |
| OsHost | Publish an independently owned record after validating native text. system/release/machine and any admitted optional text are owned Strings; scalar count and absent option arms carry no view. Failure publishes no successful record. |
| OsIdentity / ProcessCpuCount / TimeNow / TimeInstant | Publish independent scalar/record values using existing platform and error contracts. No view profile is retained. |
| TimeFormat | Validate the exact existing format discriminator and timestamp bounds, then publish a fresh owned String only on success. |
| TimeParse | Read the complete supplied text under the selected format and publish an independent timestamp scalar on success. Failed parsing has no successful payload. |
| TimeSleep | Consume a scalar duration and retain no input or result view. |

The source lifetime of a borrowed connection I/O shell is independent from its
fresh shell allocation. The UDP Buffer publication is the ordinary byte writer
rail, not a read-only native getter. DNS/host/environment/time outputs use their
actual owned-string publication owners, followed by ordinary ReadOnly byte views
of strings. Acceptance closes each handle dependency, copied text independence,
Buffer action, optional/empty output and failed publication without widening OS,
network or process-global behavior.

### Random permutation and sampling

| Rvalue | Publication and reached continuation |
| --- | --- |
| RandSeed | Initialize independent scalar RNG state from the explicit seed or existing OS-seed rail. No view-bearing result or retained caller input exists. |
| RandNext / RandRange | Read/mutate RNG state and publish an independent scalar. Range keeps its existing bounds/termination behavior. |
| RandShuffle | Require authority for the reached Out slice action and mutate outer element storage, preserving length and the set of contained profiles while joining possible permutations. Scalar bytes may change; str elements move descriptors whose text remains ReadOnly with the original backing/observations. The source Out action remains conservative even when native length zero/one causes no swaps. |
| RandSample | Read the input run, mutate RNG state and publish a fresh Writable owned array of k selected Copy elements, preserving selected nested descriptor profiles. Source outer storage is not retained. Invalid k terminates before publication; k zero publishes no element profile. |

The admitted grammar is exactly the existing `rng_elem_ok` primitive Copy set,
which includes borrowed str and excludes owned String/aggregate extensions. The
native byte permutation cannot be modeled as making all nested text bytes
writable, and sample cannot be modeled as copying the text payloads. Acceptance
crosses byte/scalar/header elements, empty/single/full/partial cases, output
observation retention and source Out invalidation. The existing RNG/native
algorithm is unchanged; no new statistical or resource claim is introduced.

### Codec validated carriers and copied encoder inputs

| Rvalue | Publication and reached continuation |
| --- | --- |
| CodecOpen | Read and validate the complete supplied byte view. Success retains that same descriptor/backing and creates the existing Codec observation; failure publishes no batch. Validation neither copies input nor grants Writable authority. |
| CodecBatchRows / CodecBatchColumns | Read validated input metadata and return independent scalar counts. Their scalar result has no retained observer, but the reached batch read requires the original observation to remain live. |
| CodecBatchName | After ordinal validation, publish Some ReadOnly text into the retained batch input; None has no text payload. Preserve batch backing and Codec observation on successful text. |
| CodecBatchKind / CodecBatchFind | Read validated batch metadata and lookup text where supplied. Publish independent optional scalar/enum results, retaining no lookup-name input. |
| CodecBatchColumn | Validate ordinal and the exact requested kind, then publish the typed column carrier on success. Preserve the batch input backing and Codec observation; no independent column allocation exists. |
| CodecColumnLen | Return the stored independent row count, while preserving the existing source-use requirement on the carrier. Copying a count does not retain the column's byte observation. |
| CodecColumnAt | Guard row bounds before byte/offset loads. Scalar columns publish independent optional values. String columns publish Some ReadOnly text into the original byte input with its Codec observation; None has no text leaf. |
| CodecEncoderNew | Validate rows and publish an independent empty encoder on success. Failure publishes no handle. |
| CodecEncoderPut | Read name and the exact I64/F64/Bool/Str column input. Copy name and scalar bytes, or every string cell's bytes plus offsets, before insertion. No source descriptor is retained. Invalid name/count/duplicate/size/data input leaves the existing encoder's admitted columns unchanged under its native validation order. |
| CodecEncoderFinish | Consume the encoder, serialize all retained copied data into a fresh independent Buffer and release staged columns/shell. The Buffer has ordinary Writable byte authority. It does not acquire a Codec observation until a subsequent successful open validates that particular output generation. |

The existing guarded byte-addressed LLVM decoder and runtime validator/encoder
own these records. Plan 60's byte observations remain distinct from write
permission and lifetime. A string-cell read and a scalar metadata read both need
the live original carrier at their source-use point, even though only the former
retains the observation afterward. Encoder insertion instead completes a byte
copy; changing caller input afterward cannot invalidate its independent output.
Acceptance covers all four kinds, ordinal/row guards, scalar-versus-text result
retention, encoder failed insertion, copied names/string cells and finish/reopen.

### Text builders and retained array elements

| Rvalue | Publication and reached continuation |
| --- | --- |
| BuilderNew | Create empty independent builder storage with the existing capacity/arena and stack-versus-heap shell selection. Shell placement does not change payload authority. |
| BuilderWriteStr / BuilderWriteStrIntStr | Read each text operand in existing source order, then append copied/formatted bytes into the builder. Retain no input descriptors. Fused writing after source-event erasure must preserve the already-proved source uses. |
| BuilderWriteInt / BuilderWriteBool / BuilderWriteChar / BuilderWriteFloat | Format scalar input into builder-owned bytes without retaining a source view. Preserve signedness/width and formatting discriminators. |
| BuilderToString | Consume the builder and transfer its owned grow allocation to an independent String. Preserve source nulling and the canonical empty String representation. No earlier write operand remains a result dependency. |
| TemplateHtmlNew | Create an independent empty HTML builder. |
| TemplateHtmlWrite / TemplateHtmlRaw | Read complete UTF-8 source, append escaped or raw copied bytes respectively, and retain no source descriptor. Existing source/builder disjointness and native growth admission remain required. |
| TemplateHtmlToString | Consume the shell and transfer its payload to an independent owned String, preserving the native spent/nulling state. |
| ArrayBuilderNew | Create empty heap or explicit-region builder storage with the exact validated element layout. No element profile exists until an initialized push/append. |
| ArrayBuilderPush | Read/copy one admitted element into builder-owned element storage. Scalars add no view dependency. RegionPlain aggregate/header copies retain their exact nested profiles, observations and backing identities; copying a descriptor never creates independent nested bytes. |
| ArrayBuilderPushStr | Transfer one owned String allocation into the builder's element slot, preserving its allocation identity and the existing source nulling. This is the owned-string operation, not a borrowed str byte copy. |
| ArrayBuilderAppend | Read the initialized selected element run and copy each admitted Copy element profile. Heap mode's scalar restriction and region mode's exact RegionPlain grammar remain prerequisites. Empty runs add no element profile. Source outer storage is not retained, but nested view dependencies are. |
| ArrayBuilderBuild | Consume the source builder according to its existing type contract. Heap publication transfers element storage; region publication compacts initialized chunks into fresh contiguous arena storage. Both publish Writable outer storage, exact logical length and the accumulated contained profiles. Reallocation/compaction does not freshen nested descriptor backing or validation. |

Unfinished owned-string builders release every transferred String; completed
arrays use ordinary element Drop, and region Copy elements have no invented
owned cleanup. Stack-header native siblings implement the same transfers without
freeing caller-owned header storage. Generated arena templates publish ReadOnly
text copied into the selected arena, while arena-free templates use the owned
String finish rail. The legacy raw-FFI null-arena finish fallback is not an
additional compiler-generated source producer.

The runtime owners are builder write/finish helpers, TemplateHtml's sealed state
operations and ArrayBuilder's scalar/header/bytes push, append, build and Drop
families. Acceptance distinguishes copied text bytes from copied view headers,
heap ownership from region lifetime, empty/initialized element sets, owned-string
move/nulling/Drop and nested observations preserved through build and growth.

### XML consumed storage and cursor views

| Rvalue | Publication and reached continuation |
| --- | --- |
| XmlParse | Consume the owned input String and transfer its existing allocation into the successful reader, without copying or assigning a fresh identity to input bytes. Public parse failure frees that allocation; success adds one independent reader shell. Preserve the explicit cleanup auxiliary value and source nulling proof. No reader exists on Err. |
| XmlNext | Mutate the reader's cursor/current-event state and publish the independent optional event enum. Preserve existing invalidation of cursor-bound views even though input bytes remain unchanged. This operation grants no caller write permission on retained input. |
| XmlName / XmlAttributeName | Publish ReadOnly text into reader-owned input, with the existing current-cursor lifetime. Invalid state/index terminates according to the checked native status contract. A later next cannot retain a live name view merely because its physical bytes still exist. |
| XmlAttributeCount | Read current start-event state and publish an independent scalar. Existing state and count bounds apply. |
| XmlAttributeValue / XmlText | Read the selected input span and publish a fresh independently owned normalized String. Repeated calls allocate/copy independently; empty attribute values use the canonical empty owned representation. Successful results retain neither the reader nor its cursor observation. |

`std-design/xml.md` owns the source states and lifetimes. The native shape,
output-disjointness and cleanup gates in `xml.rs` and MIR producer validation
remain prerequisites. The transfer does not equate a native mechanically rejected
unsafe call with an accepted source ownership transfer. Actual native failure
statuses still go through their existing public-error versus hard-abort mapping.
Acceptance includes moved input identity, parse success/failure, name/attribute
cursor invalidation, copied normalized output independence and reader Drop.

### CSV column and text publication

CsvDecode validates its existing row descriptor, options and complete selected
cell conversion before publishing a successful SoA. A nonempty result has fresh
Writable arena column storage and exactly the validated row count. Scalar columns
retain no input dependency. Text descriptors are ReadOnly: ordinary cells refer
to input subslices; escaped cells refer to normalized bytes appended to the same
arena allocation as the columns. Both alternatives keep the existing source
input/arena lifetime obligations. Allocating a text copy does not grant mutable
byte access through a str field. Empty success publishes no row profile, and
failure publishes no successful SoA or initialized partial-row prefix.

The native producer `align_rt_csv_decode_soa_v1` and `write_cell` own the
input-versus-normalized distinction. Options retain the exact header/line-ending/
max_rows discriminators and established validation order. The analysis models
those values and status-dependent publication without promising a new allocation
bound or permitting malformed descriptor/options records. Acceptance separates
outer column writes, nested text-byte rejection, escaped/unescaped text,
scalar-only rows, empty input and conversion/limit failure.

### Shape-directed JSON materialization

| Rvalue | Publication and reached continuation |
| --- | --- |
| JsonDecode | Read complete input under the validated struct descriptor and publish the initialized struct only on success. Scalar leaves are independent; borrowed str leaves retain input/arena obligations, and nested owned arrays have independent Writable header/element storage with their contained leaf profiles. |
| JsonOwnedDecode | Consume the validated OwnedJsonGraphPlanV3 and read input to construct its exact independently owned output graph. Owned String leaves copy/decode input bytes, array<String> owns both its header spine and each String, and nested records/arrays use the plan's recursive ownership/Drop shape. No successful borrowed input leaf is synthesized. |
| JsonDecodeStructArray | Stage initialized rows, release partial owners on failure, and publish a fresh Writable owned AoS only after complete success. Each row receives the same exact field-profile transfer as JsonDecode. No partial row/array escapes through an error status. |
| JsonDecodeSoa | Validate/count then directly fill fresh Writable arena columns under the existing SoA row grammar. Scalar fields are independent; str fields have ReadOnly input-subslice or decoded-arena bytes. Empty arrays have no row profile. Failure publishes no successful columns. |
| JsonDecodeUnion | Select the admitted shape-directed variant and initialize only its payload. Apply the payload's descriptor transfer recursively; other variants have no live profiles. Trailing input after a successful payload still fails and releases its owned leaves before returning no successful union. |

For borrowed str descriptor leaves, clean values reference input and escaped
selected values require the existing supplied arena. Both produce ReadOnly text;
an absent arena does not authorize a hidden owned allocation. The existing
owned graph descriptor uses its distinct owned-string/owned-string-array
operations and must remain tied to the checked graph, not selected merely by a
matching output byte width. Options initialize only the present payload; missing
or null None does not retain an invented text observer. Nested array allocation
creates outer write authority while preserving nested borrowed dependencies.

Use the existing descriptor/graph validation as the type formation prerequisite
and one shared type-directed profile materializer for these families, rather than
independent flattened per-field implementations. It must distinguish owned and
borrowed text, scalar/array/string-array/nested-struct/union/Option paths and
success versus partial failure. Reuse the profile DAG's compact recursive layout
handling and initialized-content rules. No native offset table alone authenticates
an incompatible source type. Acceptance includes each leaf kind, nested array and
union/Option presence, speculative decode fallback cleanup, trailing failure,
escaped/clean text, empty rows and the direct-owned graph boundary.

### JSON scalar arrays and fused scanner rows

| Rvalue | Publication and reached continuation |
| --- | --- |
| JsonDecodeArray | The admitted primitive scalar discriminator parses the complete input into temporary scalar bytes, then publishes a fresh owned Writable array only on success. Empty arrays have no element profile. Malformed input publishes no successful array. The historical native comment about str elements does not widen the actual bool/float/integer descriptor implementation or its checked shape. |
| JsonDecodeScalar | Read the complete input and publish an independent scalar on status zero. Failed parsing, range validation or trailing input has no successful scalar payload, even if the native output slot was partially written. |
| JsonScanNew | Retain the input descriptor itself as the scanner representation. No tape, copy, arena or independent backing is created. Preserve input lifetime and observations; constructing a scanner does not complete all later row validation. |
| JsonScanNext | Read the retained input and current cursor. Status zero initializes the one-row fixed struct-array slot and advances the cursor; status one is exhaustion without a new row; status two is malformed input without an admitted row. Successful borrowed text leaves are ReadOnly subslices of scanner input. Their input observations survive row descriptor copies; the cursor/status scalars carry none. |

The scanner's row schema must pass both the existing descriptor grammar and
`json_scan_row_is_copy`, including a valid recursive DropPlan with no owned
leaf. The row slot is ordinary writable local storage; that authority never
transfers to text bytes referenced by its fields. Reusing the row slot overwrites
headers, not input bytes. The no-arena scanner rejects selected escaped strings
through the existing malformed-row path rather than allocating silently. Cursor
zero triggers whole-input UTF-8 validation, while later row parsing still reads
retained input; the access proof must check that read on every reached step.

`lower_json_scan_reduce` branches on native status before entering stages or
folding a row. This status/initialization relationship belongs in the native
transfer record and cannot be replaced by marking the row initialized on every
call. Accept only the validated scanner/struct/one-element-slot/cursor shape,
including imported descriptors. Existing success, exhaustion and malformed rows,
scalar-only arrays, borrowed row fields, copied rows and input writes between
reached steps close the applicable materialization and observer cells.

### JSON document tape and accessor transfers

The successful `JsonDoc` producer owns fresh arena tape storage while retaining
its input byte range. Tape ownership does not make those input bytes writable.
Each document profile carries the input lifetime and validation observations plus
the tape arena lifetime. This follows the existing Sema borrow/region contract,
including the conservative input dependency of escaped string accessors. The
source-use recipe checks the document before each accessor, even when that
accessor returns an independent scalar or a missing result.

| Rvalue | Retained state and publication |
| --- | --- |
| JsonDoc | Validate and parse the input; status zero publishes a root handle into fresh arena tape containing the original input pointer. Malformed, oversized or otherwise rejected input publishes no successful handle. Retain the input profile on the successful document, with the arena lifetime; neither parsing nor tape allocation grants write authority on the input. |
| JsonDocGet | Read document and key. Publish a handle into the same tape for the first matching member, or the same-tape Missing sentinel. Retain the document dependency, including on Missing; do not retain the lookup key. |
| JsonDocAt | Read document and index. Publish the selected array child or same-tape Missing sentinel for wrong kind, negative/out-of-range index or Missing input. Preserve the receiver dependency without inventing a child allocation. |
| JsonDocAsStr / JsonDocKey | Some publishes read-only text. Escape-free text aliases the input; escaped text has fresh decoded bytes in the tape arena. Both retain the existing document/input lifetime and observation obligations. None publishes no text payload. Key rejects non-object or out-of-range selection through None. No writable byte descriptor follows from arena allocation of escaped text. |
| JsonDocElems | Publish fresh arena storage holding copied document handles, with ordinary Writable outer slice authority established by the allocation and mutable initialization. Each initialized element still refers to the original tape and retains the document's input dependency. Object elements are member values, not keys. Empty/non-container/Missing produces an empty slice with no element profile. The outer storage lifetime includes the allocation arena and receiver lifetime. |
| JsonDocKind / JsonDocLen | Read the document and return independent scalar kind/count; Missing/non-container behavior follows the existing native tags and zero count. No document dependency is retained in the scalar result. |
| JsonDocAsScalar | The admitted i64/f64/bool discriminator reads the document and publishes Some of an independent scalar or None. Numeric access can read the retained original number token; scalar independence at completion cannot waive that input read. No view is published. |

`align_rt_json_doc_parse`, `doc_write_str`, the `align_rt_json_doc_*`
accessors and Sema's `JsonDocElements` storage initializer are the producer
owners. The element buffer is allocated with `Arena::alloc_uninit` and filled
through a mutable pointer before publication; this is distinct from granting
writable authority to an arbitrary shared native-container getter. Replacing a
handle in that outer slice affects that element profile, not the shared tape or
an earlier copied handle. Empty results and native defensive null paths do not
initialize an element that the source can load without its normal bounds check.
Owner cases must distinguish input-backed and escaped text, Missing handles,
empty/nonempty element buffers, copied versus replaced handles, and independent
scalar output followed by an overlapping input write. Enclosing eager source
snapshots still follow the shared frontier recipe.

## Secondary discriminator inventory

This author table closes discriminator membership separately from the 335-variant
partition. It does not replace the exact native output/observation transfer
records. The structural producer validates each discriminator, operand schema,
output type and admitted combination before access derivation.

| Discriminator and operations | Exact alternatives | Access distinction |
| --- | --- | --- |
| `StrPredKind` / `StrPredicate` | `Contains`, `StartsWith`, `EndsWith`, `Find`, `Rfind`, `EqIgnoreCase` | Consume both text inputs; scalar-only result. |
| `StrTrimKind` / `StrTrim` | `Both`, `Start`, `End` | Consume text; preserve selected input backing and validation observations. |
| `PathComponentKind` / `PathComponent` | `Base`, `Dir`, `Ext` | Consume text; preserve input-derived substring observations. |
| `CodecPutKind` / column projection and encoder put | `I64`, `F64`, `Bool`, `Str` | Scalar kinds expose no contained text; `Str` selects the codec input-derived text column. Encoder put copies encoded bytes. |
| `GroupOp` / group aggregates | `Sum`, `Min`, `Max`, `Count` | Scalar aggregate outputs; borrowed key text preserves the exact key profile. Count has no value-field input. |
| `FrameJoinKind` / `FrameInnerJoin` | `I64`, `Str` | Both read validated codec-column inputs; string keys additionally read their text bytes. The result is an independent owned array of scalar row-index pairs, not copied source columns or borrowed key text. |
| `EncodingKind` / encoding | `Utf8Lossy`, `Base64`, `Base64Url`, `Hex`, `Percent`, `PercentPath`, `Form`, `Html` | Encode reads raw bytes and creates independent text; admitted decode creates independent writable buffer bytes. `Utf8Lossy`, `PercentPath`, `Html` have no decode operation. |
| `CompressKind` / compress/decompress | `Gzip`, `Zstd` | Read input bytes; fresh writable output buffer only on successful publication. |
| `HashAlgo` / hash | `Sha1`, `Sha256`, `Sha512` | Read bytes; independent owned byte array of 20, 32 or 64 bytes. |
| `AeadCipher` × `AeadDir` | `{Aes256Gcm, ChaCha20Poly1305}` × `{Seal, Open}` | All four read key/nonce/input/AAD and publish fresh buffer on success; no retained input view. |
| `SignatureAlgorithm` / PEM, JWK, sign, verify | `Rs256`, `Es256`, `Ed25519` | PEM consumes text; JWK consumes already-decoded byte components. Signature output is a fresh Buffer; verify reads message/signature and yields scalar status/truth. JWK has two components for Rs256/Es256 and one for Ed25519. |
| `CliFlagKind` / `CliFlag` | `Bool`, `Str`, `I64` | Name is text; only Str consumes a text default, copied into native storage. Bool has no default operand. |
| `TimeFormatKind` / format/parse | `Rfc3339`, `Rfc3339Ms`, `Rfc1123`, `BasicIso`, `BasicDate` | Format publishes owned text; parse consumes input text and has a scalar output. |
| `ResourceViewKind` / raw view | `StrUtf8`, `Slice(Scalar)` | Exact allowed scalar/layout and UTF-8 checks remain mandatory. Validation does not create writable permission for unavailable raw backing. |
| `ParMapStageKind` | `Map`, `Filter`, `FilterStrContains`, `Project { field }`, `FilterField { field }` | Map instantiates its target/captures and replaces the element profile; filters preserve it; Project selects one field. String filter consumes selected text and its needle. |
| `ParallelSource` | `Materialized`, `VirtualChunks` | Materialized loads element profiles; chunks publish source-backed slices with the source permission and contents. No independent chunk backing is invented. |
| `ColumnBatchInput` | `Scalar`, `View { ptr, len }` | The checked column schema selects scalar versus variable-width copy. Variable-width append copies into batch-owned storage; it does not retain the source pointer. |
| `FsTreeOutput` | `None`, `Owner`, `Metadata`, `Bytes`, `Bool`, `CursorNext { entry, present }` | Use the exact kind/output pairing. Byte/name outputs are independent owned byte arrays; metadata is scalar. A missing entry does not publish an initialized name. |

Boolean axes also remain explicit: `ReaderOpen.regular_only`,
`WriterStd.buffered`, `BytesRead.be`, `BufferPut.be`, `VecMinMax.max`,
`RegexReplace.all`, `RunOutputView.err`, `RunBytesView.err`, `HttpServe.shared`,
`HttpHeadersContainsToken.exact` and `HttpStreamSend.event` preserve each family's
transfer while selecting its existing behavior. Both run-view alternatives select
separate native stdout/stderr backing with read-only byte publication. Both endian
values leave access unchanged. Both regex replacement modes publish owned text.

`SpawnTask.fallible` and `TgWaitResult.fallible` retain normal-result versus
error-result reachability and the existing lowest-spawn-index error contract.
`ResourceFromRaw.abort_on_null`, `ResourceViewFromRaw.allow_null_if_empty`,
`check_nonnegative_len`, `check_alignment` and `check_utf8` remain checked safety
axes, never writability evidence. Native entry/callback and task-result flow still
need their exact program records.

### Retained process and filesystem operation schemas

For every `ProcessLiveKind`, read input roles from the producer-owned `inputs`
schema. `ChildReadStdout`, `ChildReadStderr`, `ScopeReadStdout`, `ScopeReadStderr`,
`SealedReadAt` and `ImageReadAt` are the six OutBytes writers. Each checks writable
permission and invalidates potentially overlapping observations even on a fallible
call; no success-only guess may erase a possible partial native write.

`ChildGroupMembers`, `ProcessTable`, `ScopeChildren` and `ScopeReap` publish fresh
array backing. Their exact element records contain scalar observations or native
member handles, not borrowed byte views. Native owner production and transitions
(`CommandStart`, `SignalNew`, `ScopeStart`, `MemoryNew`, `MemorySeal`, `Executable`,
`CommandImage`, `CurrentImage`, `UserNamespace`) retain existing ownership and
nulling rules; no outer handle grants a permission to an unspecified inner view.
`MemoryWrite` reads and copies its byte input into file storage. `CommandImage`
copies argv text. `CommandStdoutTo`, `CommandStderrTo`, `InheritFile` and
`InheritNamespace` retain the existing native-handle dependency only.

The remaining exact process kinds have scalar/unit effects on their own native
state: `CommandNewSession`, `ChildId`, `ChildStatus`, `ChildTryWait`, `ChildPoll`,
`ChildKillGroup`, `RunOutputStatus`, `RunBytesStatus`, `SignalNumber`, `SignalNext`,
`SignalClose`, `ScopeId`, `ScopeOwnerId`, `ScopeStatus`, `ScopeTryWait`, `ScopeWait`,
`ScopePoll`, `ScopeKill`, `ScopeKillGroup`, `ScopeRelease`, `MemberKill`,
`MemberFinished`, `SealedLen`, `ImageLen`. They do not write arbitrary retained
byte descriptors. These groups cover all 48 kinds.

The 24 `FsTreeKind` values use the same role-driven input rule. `DirectoryReadLink`
publishes owned byte copies; `CursorNext` publishes an optional fresh entry name.
`DirectoryOpen`, `DirectoryCursor`, `DirectoryOpenDir`, `DirectoryOpenRead`,
`DirectoryOpenReadSingleLink` and `DirectoryCreateNew` publish native owners.
`DirectoryMetadata`, `DirectoryMetadataAt`, `ReaderMetadata`, `WriterMetadata`,
`FileMetadata`, `DirectoryMetadataFollow`, `DirectoryAccess` and `DirectoryAccessAt`
publish scalar records or booleans. `DirectoryCreateDir`, `DirectoryRemoveFile`,
`DirectoryRemoveDir`, `DirectorySetMode`, `ReaderSetMode`, `WriterSetMode`,
`FileSetMode` and `DirectoryCreateSymlink` have no byte-view output. Raw path inputs
are byte reads; none is an OutBytes destination. Error/absence edges follow the
existing exact scratch initialization schema.


## Statement and terminator transfer inventory

27 Stmt variants classified exactly once; five Term variants.

| Transfer family | Variants |
| --- | --- |
| ordered Rvalue evaluation | `Let` |
| exact local descriptor replacement | `Store`, `StoreField`, `StoreConstArray` |
| element write / descriptor-content update | `StoreIndex`, `PtrStore`, `PtrStoreNoalias`, `VecStore`, `StoreElemField`, `StoreElemFieldPtr`, `StoreColumn` |
| existing lifetime/cleanup ending | `ArenaEnd`, `Drop`, `DropElem`, `DropElemField`, `DropValue`, `ColumnBatchFinish`, `ColumnBatchDrop` |
| selected descriptor nulling | `DropFlagInit`, `NullTupleField`, `NullStructField`, `NullElemField` |
| explicit unsafe contract | `RawFree`, `RawStore` |
| ordered task applications | `TgWait`, `TgEnd` |
| existing reservation proof | `BorrowedElementReservation` |

Goto retains the state. Branch checks its Boolean operand and selects feasible
edges from the finite control domain. Return and ReturnWithCleanup consume the
completed returned value and publish normal-return profiles/anchored effects;
the cleanup bit retains its existing ABI prerequisite, not a second access mode.
Unreachable publishes no return, while earlier write obligations remain recorded.

All source lifetime, range, no-alias and cleanup proofs remain prerequisites.
An exact inline element/path store may replace descriptor contents; dynamic
element and summary-location updates stay weak. A scalar byte store checks the
destination view permission and ends overlapping UTF-8/codec observations.
Pointer stores generated for source-safe writes/materialization retain their
typed producer origin; being represented as an LLVM pointer grants no authority.
StoreConstArray initializes the local copied array as writable while each
contained text/static descriptor retains its own read-only origin. Nulling
replaces only its selected descriptor profile and preserves completed aliases.
Dropping a local root does not erase observations retained by another live root.
TgEnd, like TgWait, must apply retained task bodies before dropping the group.
RawFree/RawStore follow the conditional unsafe transfer above and grant no safe
view permissions. The native reservation statement remains a source-admission
prerequisite and is then projected out; its source observation events survive.

The wire tables and exact native recipes above complete this transfer partition.
