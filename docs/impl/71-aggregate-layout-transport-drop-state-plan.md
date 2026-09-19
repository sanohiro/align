# Aggregate layout, transport, and drop state

Status: plan of record for issues
[1076](https://github.com/sanohiro/align/issues/1076),
[1077](https://github.com/sanohiro/align/issues/1077), and
[1078](https://github.com/sanohiro/align/issues/1078). The design is complete;
implementation starts only after one fresh independent adversarial review of
this ledger. Evidence baseline: Align `9583d8c8`, LLVM 22.1.8.

These issues expose one boundary. A tagged value's physical size determines its
argument and result transport; that transport carries the value's cleanup
state; and a mutable borrow need only transport cleanup state when the callee
can change it. Solving the three independently would create incompatible
signatures or leave one optimization blind to another. This document fixes the
complete contract and divides it into four ordered, independently useful
capabilities.

## 1. Scope and sequence

```text
PR 1  #1076  one union-storage representation for user sums, Option, and Result;
               tag-directed construction, projection, Drop, JSON, and callbacks
PR 2  #1078  one per-parameter drop-state effect; conservative indirect-call
               adapters; one MIR drop-state simplifier and proved move-out nulling
PR 3  #1077  target-selected parameter and cleanup-result transport; tag-only
               result inspection
PR 4  #1077  destination construction for fresh whole values, with explicit
               partial-initialization control-transfer cleanup
```

The order is strict. PR 1 establishes the sizes PR 3 classifies. PR 2 fixes the
cleanup and mutable-borrow signatures PR 3 normalizes. PR 4 consumes the final
result transport without changing its signatures. Each PR is mergeable:
PR 1 reduces every tagged value without changing call transport, PR 2 removes
unnecessary cleanup traffic with the existing transport, and PR 3 consumes both
final ABI forms. PR 4 removes fresh-value staging as a separate MIR ownership
capability.

PR 1 and PR 3 may each exceed roughly 1,000 changed hand-written lines. PR 1
must change the representation producer and every projection, construction,
Drop, serialization, callback, and ABI-layout consumer together; a partial
change misreads values. PR 3 must normalize definitions, declarations, calls,
returns, parameters, cleanup channels, and function values together; a partial
change creates incompatible LLVM signatures. Keeping each closed
producer-to-consumer chain in one capability avoids duplicate proof and lowers
integration risk.

No language syntax, source annotation, runtime export, allocation mode, error
value, evaluation order, or foreign ABI changes. Native extern and runtime
signatures remain owned by their existing ledgers. The work applies only to
compiler-owned program callables and compiler-internal representation.

## 2. Public-contract ledger

This section is authoritative. Although most records are compiler-internal,
the tagged representation and serialized drop-state effect cross compilation
units and therefore receive the full public-contract treatment.

### 2.1 Tagged union storage

```text
Surface           physical layout of every non-recursive user sum, Option<T>,
                  and Result<T,E>

Exact type        Tag is i32 for a user sum and i8 for Option/Result. Each
                  variant has a logical-to-physical vector of exactly its
                  source payload arity:
                    OmittedUnit
                    Stored { llvm_type, offset:u64, size:u64, align:u64 }
                  A source Unit payload always maps to OmittedUnit. It has no
                  LLVM field, SSA value, storage, load, store, move, or Drop;
                  construction validates/evaluates its Unit operand and writes
                  nothing, while projection synthesizes the ordinary valueless
                  Unit result. Every other payload maps to Stored. P_v is the
                  unpacked LLVM struct of only those Stored fields in source
                  order. Let S_v and A_v be P_v's ABI
                  allocation size and alignment. Let S=max(S_v), A_p=max(A_v),
                  A=max(tag ABI alignment,A_p), and P=align_up(tag size,A_p).
                  When S>0 the sum is { Tag, U }, where U is
                  { [0 x Anchor], [S x i8] }; Anchor is the first P_v in
                  declaration order whose ABI alignment is A_p. The zero-sized
                  anchor gives U alignment A_p without occupying storage. The
                  outer ABI size is align_up(P+S,A). When every P_v has S_v=0,
                  the sum is { Tag } and has no U field. Unit-only variants and
                  empty payload structs have S_v=0, A_v=1 and do not by
                  themselves create storage.

                  Variant ordinals and tag meanings do not change: user sums
                  use declaration order; Option is None=0, Some=1; Result is
                  Ok=0, Err=1. There is always an explicit tag. No niche
                  encoding is admitted.

Inputs/defaults   complete reachable type graph plus the selected target data
                  layout. There is no ambient or byte-threshold default.
Errors            reject before module publication on an incomplete/recursive
                  layout, arithmetic overflow, unsupported address space,
                  unavailable target layout, or inconsistent semantic and LLVM
                  layouts or logical-to-physical payload maps. Existing rejection of unsupported explicitly
                  over-aligned payloads remains. When several inputs are
                  invalid, graph/type validity precedes target layout, which
                  precedes size arithmetic, which precedes LLVM-body agreement.
Ownership         the tag selects the sole live payload. Move and Drop inspect
                  only that tag and active payload. Inactive storage has no
                  owner and is never read.
Allocation        inline only; no new heap, arena, or host allocation in the
                  generated program. Compiler layout tables are scoped to one
                  emission.
Owner             align_sema owns semantic size/alignment; align_codegen_llvm
                  owns LLVM bodies and byte-offset access. Their independently
                  computed values must agree before publication.
Artifact/cache    structural over the complete reachable type-definition graph,
                  variant order, payload order, compiler/LLVM identity, target
                  triple, data layout, CPU/features, and profile. Existing
                  compiler build and codegen keys carry these inputs. The unit
                  interface continues to serialize semantic enum definitions;
                  no physical-layout certificate is added.
Prerequisite      checked, finite, non-recursive types and the selected target.
Acceptance        §5 PR 1 matrix and exact semantic/LLVM layout twins on every
                  supported native CI target.
Benchmark         required local representation measurement: allocation size
                  and construction stores for small and largest variants. No
                  latency threshold is a correctness gate.
Mirrors           docs/impl/05-backend-llvm.md, docs/impl/07-roadmap.md,
                  docs/open-questions.md, and HANDOFF.md. The language surface,
                  ownership rule, and source semantics in draft.md and
                  docs/language-spec.md do not change.
```

Construction writes the tag and only the selected Stored payload fields. Padding and
inactive bytes are unspecified and may remain uninitialized. Projection first
selects the storage field, then uses byte GEPs at the target-computed offsets
of P_v; OmittedUnit synthesizes Unit without touching storage. It never indexes
the former flattened aggregate. Drop switches on the
tag before forming or reading a payload place. JSON, diagnostics, matching,
callbacks, copies, and returns likewise observe only the tag and active fields.
No equality, hashing, serialization, cache identity, or foreign boundary may
observe padding or inactive bytes.

A niche representation is deliberately refused. Owned headers and pointers
can be null or empty during construction and after moves, scalar payloads may
use their full domain, and niche availability is target- and ownership-
dependent. A niche would introduce a second tag, Drop, cache, and cross-unit
rule for no source-level capability. The explicit tag preserves one uniform
representation.

### 2.2 Drop-state effect and indirect-call adapter

```text
Surface           one effect-state cell per parameter of every program function

Exact type        enum DropStateEffect {
                      NotApplicable, Invariant, MayChange, Deferred
                  }
                  IFnSig stores a vector whose length is exactly params.len(),
                  indexed by parameter ordinal. A concrete droppable BorrowMut
                  is Invariant or MayChange. Every concrete non-droppable
                  BorrowMut and every non-BorrowMut parameter is NotApplicable.
                  A generic template records Deferred at every BorrowMut
                  ordinal and NotApplicable elsewhere; a template has no
                  physical callable ABI. Invariant means the concrete body performs no
                  operation that can change ownership state: no move-out,
                  replacement, conditional release, or call through an edge
                  whose corresponding effect is MayChange or unknown.
                  MayChange is the conservative state.

Derivation        after generic substitution and ordinary monomorph body
                  checking, derive a fresh concrete vector from that
                  monomorph's resolved parameter types and checked MIR; never
                  copy or specialize the template's Deferred cells. Complete
                  all reachable monomorphization first, then compute the least
                  fixed point over the complete concrete direct program call
                  graph. A function starts Invariant for a parameter and becomes
                  MayChange when its body performs a listed operation or
                  delegates that parameter to a MayChange/unknown direct,
                  imported, closure, function-value, raw, native, or malformed
                  edge. Pure recursive SCCs with no changing operation remain
                  Invariant. The result is deterministic by function and
                  parameter ordinal, independent of traversal order.

Inputs/defaults   checked concrete MIR bodies and imported non-generic IFnSig
                  records. Imported generic bodies are instantiated and
                  analyzed in the consumer exactly like local templates. An
                  unresolved callable or unavailable internal analysis fact
                  becomes MayChange before any ABI is formed. A malformed,
                  incomplete, duplicate, wrong-arity, or wrong-mode
                  authenticated interface record is rejected as specified next.
Errors            an authenticated interface record that disagrees with the
                  declared signature is rejected before body/codegen mutation.
                  Multi-invalid validation order is signature graph, parameter
                  arity/modes/types, vector length, enum tag, per-ordinal mode
                  and generic/concrete admissibility, resolved droppability,
                  then call-graph substitution. Deferred in a concrete function,
                  Invariant/MayChange in a template, or a changing tag on a
                  non-droppable/non-BorrowMut parameter rejects.
Ownership         Invariant passes only the non-null data pointer. The caller
                  retains and does not expose its drop flag; the callee creates
                  no proxy, entry load, or writeback. MayChange retains the
                  current {data pointer, drop-flag pointer} pair; the caller
                  owns the flag and the callee may update it before return.
Allocation        none for the effect. Adapter thunks are compiler-generated
                  code and allocate no runtime storage beyond the existing
                  conservative cleanup proxy where required.
Owner             align_mir derives the effect and owns drop-state
                  simplification; align_interface serializes it;
                  align_codegen_llvm consumes it.
Artifact/cache    bump interface FORMAT_VERSION from 12 to 13. In write_fn and
                  read_fn, immediately after return_cleanup and before
                  producer_certification, encode one u32 little-endian length
                  followed by one u8 per parameter: 0=NotApplicable,
                  1=Invariant, 2=MayChange, 3=Deferred. No other tag is valid.
                  The length must equal the preceding parameter count. The same
                  bytes occur in write_surface, so interface_hash covers them;
                  complete artifact serialization reuses that surface. The
                  existing parameter-mode/certificate byte golden becomes a v13
                  semantic-to-byte golden containing all four tags, and an
                  independently assembled v13 byte vector must decode to the
                  expected semantic record. Mutated length, tags, modes,
                  generic-body presence, and droppability each reject. Unit
                  cache frontend_schema consumes FORMAT_VERSION and therefore
                  misses v12 artifacts. Whole-program and per-unit compilation
                  serialize the same semantic record.
Prerequisite      checked MIR ownership operations and complete imported
                  interfaces.
Acceptance        §5 PR 2 matrix, including generic Copy/Move substitutions,
                  local/imported template instantiation, whole/per-unit equality,
                  recursive SCCs, v13 byte goldens, malformed fallback, and
                  conservative indirect calls.
Benchmark         required local code-shape/count measurement because the plan
                  promises fewer flag allocas, loads, stores, and branches. No
                  fixed client count becomes a gate.
Mirrors           docs/impl/04-mir.md, docs/impl/05-backend-llvm.md, the
                  interface-format owner, and HANDOFF.md.
```

The source function type remains independent of a particular target body's
effect, so the canonical ABI for function values and unknown indirect calls
stays conservative. When a named or imported Invariant function is converted
to a function value, the compiler emits or reuses a local adapter with the
canonical `{ptr,ptr}` parameter. The adapter calls the plain-pointer core and
leaves the caller-owned flag unchanged. A join of Invariant and MayChange
targets therefore has one function type. Closures and unresolved targets keep
MayChange. Adapter identity is structural over target symbol, instantiated
signature, effect vector, codegen target, and compiler build; it is emitted
once per module and never serialized as a new source symbol.

The existing `simplify_known_drop_flags` becomes the single
`simplify_drop_state` authority. `DropFlagInit` gains an explicit origin:

```text
DropFlagInitKind::InitializeEmpty  construction/unwind safety; never removed
DropFlagInitKind::MoveOut          source nulling paired with a false flag
```

A MoveOut nulling operation may be removed only when the paired false state
dominates every reachable successor and no path reads any byte of the storage
before a complete reinitialization or exit. The proof includes exceptional
edges, branch and loop joins, early returns, `?`, `map_err`, Drop, replacement,
and malformed MIR. Runtime-selected ownership, a possible read, an incomplete
write, an unknown edge, or missing origin retains nulling. Removing nulling
does not make reading a moved value legal; checked MIR already rejects that
source program. The proof only removes a redundant safety write after validity
has been established.

### 2.3 Program aggregate transport

```text
Surface           invocation-local physical transport plan for each complete
                  compiler-owned program signature

Exact type        ParamTransport = Direct | IndirectByVal {
                      pointee LLVM type, ABI size:u64, ABI align:u64,
                      address_space:u32
                  }
                  ResultTransport = Direct | IndirectSRet {
                      result LLVM type, ABI size:u64, ABI align:u64,
                      address_space:u32
                  }
                  CleanupTransport = None | DirectPair |
                      SRetValueCleanupOut | DirectValueCleanupOut

                  None applies when no cleanup bit exists. DirectPair retains
                  the direct {T,i1} result. If {T,i1} is indirect and T is
                  indirect, SRetValueCleanupOut has physical form
                  `void f(ptr sret(T), ptr cleanup_out, params...)`. LLVM
                  requires an sret function to return void. If {T,i1} is
                  indirect and T is direct, DirectValueCleanupOut has physical
                  form `T f(ptr cleanup_out, params...)`. In both split forms,
                  cleanup_out points to one caller-owned i8, aligned to one,
                  whose stored byte is canonically 0 or 1.

Classification   LLVM 22 target lowering classifies the full calling
                  convention, types, attributes, target triple/data layout,
                  CPU/features, and register context. Parameters are classified
                  in complete source order because preceding parameters affect
                  register availability. No byte threshold or architecture
                  table is legal. The immutable plan is built once before
                  definitions, declarations, calls, or returns are rewritten.

Inputs/defaults   complete logical signature after drop-state effects and tagged
                  layout, selected TargetMachine, and effective attributes.
                  There is no ambient default.
Errors            unavailable classifier, overflow, unsupported attribute,
                  native/foreign owner, signature mismatch, or inconsistent
                  definition/declaration/call plans rejects before module
                  mutation/publication. Validation order is logical signature,
                  ownership/effects, callable owner, target context, classifier
                  result, then cross-edge agreement.
Ownership         byval storage belongs to the caller for the duration of the
                  call and isolates by-value mutation. sret value storage is
                  caller-owned and becomes initialized only on normal return.
                  cleanup_out belongs to the caller and is read only after
                  normal return. No pointer is exposed to Align source.
Allocation        entry-block stack storage only for a fallback byval copy,
                  result value, or cleanup byte. No loop-time dynamic alloca,
                  heap allocation, runtime export, or changed region owner.
Owner             the matched LLVM C++ bridge classifies; one shared LLVM module
                  normalizer consumes the plan for definitions, declarations,
                  direct/indirect calls, parameters, and returns.
Artifact/cache    invocation-local LLVM handles are never serialized. Existing
                  structural semantic signature/interface plus compiler/LLVM,
                  target, CPU/features, profile/LTO, and type-graph identity
                  determine the same plan. DropStateEffect is the only new
                  interface field.
Prerequisite      PR 1 tagged layout, PR 2 drop-state effects, plan 67's proven
                  target return classifier and result-slot discipline.
Acceptance        §5 PR 3 matrix and independent native implicit/explicit ABI
                  cross-links on every supported target.
Benchmark         required local storage/copy measurement for the fixed large-
                  aggregate witnesses. No client frame-size or latency number
                  is a correctness gate.
Mirrors           docs/impl/05-backend-llvm.md, plan 67 cross-link, roadmap,
                  and HANDOFF.md. Source specifications remain unchanged.
```

`IndirectByVal` lowers the parameter to a pointer with `byval(T)`, exact
alignment, and only attributes independently justified by the logical mode.
Definitions, declarations, direct calls, checked function-value calls,
closures, instantiated generics, and imported program edges use the same plan.
Runtime, extern, callback, raw-descriptor, and other native-owned contracts do
not acquire this transport merely because their LLVM type looks similar.

Tag-only inspection of an indirect tagged result loads the tag from the result
slot and loads an active payload only inside the selected arm. It never creates
a whole aggregate load just to branch.

### 2.4 Destination placement and evaluation order

The eligible final destinations are deliberately narrow:

| Destination | Rule |
| --- | --- |
| Fresh whole local | eligible when it is the sole adjacent materialization and its address has not escaped |
| Caller sret result | eligible after all source return evaluation and cleanup required before publication |
| Fresh Move struct literal | eligible in PR 4 only in a fresh whole local or sret result; fields are evaluated once in source order into final storage, with a plan of initialized Move fields for reached control-transfer cleanup |
| Existing whole local replacement | ineligible; RHS must finish before the old value is dropped or overwritten |
| Record field replacement | ineligible for the same ordering and alias reason, including Copy fields unless an independent future proof establishes an already-formed, non-observable destination |
| Indexed/element destination | ineligible; current semantics evaluate index and RHS before the bounds action, so destination formation before the call would reorder a hard error and effects |
| Join, multiple use, exposed alias, volatile/atomic, native call | ineligible; retain the correctly aligned temporary and complete transfer |

PR 4's Move literal destination construction uses one MIR-owned initialization record,
not inferred LLVM stores. Each field becomes live only after its expression
falls through and its store completes. On `?` or another early return that
actually transfers control out of the expression, cleanup drops exactly the
already initialized Move fields in reverse source order. A hard trap, abort, or
divergent child has no successor cleanup edge and retains the language's
existing no-cleanup-after-termination semantics. Copy fields need no cleanup. The destination's whole
drop flag becomes true only after every field completes. A partial destination
is never published to a callee or source expression.

This refuses issue 1077's blanket field/element placement criterion. The
observed copies are worth removing only where semantics and aliasing prove the
final address. The rejected cases remain accepted source programs and use the
existing temporary path.

## 3. Cross-stage invariants

1. Semantic layout and LLVM layout agree on size, alignment, tag ordinal,
   every OmittedUnit/Stored mapping, payload offset, and nested field offset.
   Both are independently checked; one is not trusted as a certificate for the
   other.
2. Every byte-oriented consumer observes canonical memory. In particular an
   LLVM `i1` cleanup value is zero-extended and stored as a full `i8` 0 or 1
   before JSON, native, memcpy, or byte loads can observe it. An sret function
   returns void, so its cleanup state always uses this output byte.
3. Inactive union bytes and padding are never initialized for determinism and
   never observed. A copy may transfer them only as opaque storage of the same
   type; no semantic result depends on their contents.
4. Whole-program and per-unit compilation derive identical representation,
   effects, adapters, and physical signatures from the same structural inputs.
5. Source evaluation order, hard-error precedence, exactly-once Drop, arena
   exit, and allocation parity remain unchanged on every fallback and optimized
   path.
6. Every malformed, unavailable, unknown, or native-owned edge fails closed:
   union layout refuses publication, drop state becomes MayChange, and
   transport retains the established native contract or refuses normalization.

## 4. Implementation shape

### 4.1 PR 1: tagged storage

Replace semantic accumulated-payload layout with max-variant layout. Build one
codegen `TaggedLayout` table containing tag type, storage size/alignment,
anchor, and per-variant/per-field byte offsets. All enum `field_base` consumers
move to this table; `field_base` is removed from checked HIR if no semantic
consumer remains. Option and Result use the same table rather than sibling
special cases.

Construction and projection become address-based when memory is required.
SSA-only direct values may still use a typed active-payload temporary, but no
operation may form an LLVM aggregate whose fields imply simultaneous variant
liveness. Existing callbacks and JSON helpers use the shared accessors.

### 4.2 PR 2: drop-state model

Add the effect to the interface schema and compute the fixed point before
physical signatures. Use one exhaustive ownership-operation visitor and a
compile-time variant sweep so a new MIR operation cannot silently default to
Invariant. Generate conservative adapters at function-value formation. Rename
and extend the existing pass; do not add a second drop-state optimization pass.

Pair MoveOut nulling and the false flag in one analyzable MIR record or stable
identifier. The liveness proof treats any byte read, opaque call exposure, or
partial write as a use and preserves the nulling operation.

### 4.3 PR 3: aggregate transport

Extend plan 67's matched LLVM 22 classifier and immutable rewrite table to
parameters and split cleanup results. Normalize the complete program-owned
callable graph in one module pass. Revalidate tail/musttail and parameter
attributes after hidden parameter insertion. Verify before and after runtime
module merge as today.

PR 3 retains plan 67's whole-local result placement and otherwise keeps the
general call result, replacement, field, element, join, and alias temporary
paths. PR 4 separately adds a MIR destination-construction form only for
§2.4's fresh destinations. LLVM's ordinary optimizer may coalesce a fallback
when it independently proves safety; Align promises only the explicit eligible
cases.

## 5. Implementation closure matrices

### 5.1 PR 1: layout and active-payload access

| Cell | Required closure and owner |
| --- | --- |
| Formation/validation | user sums, Option, Result, tag-only, Unit-only, empty payload, mixed alignment, nested tagged payload, generic instantiation, zero-sized fields, overflow and malformed graphs; exact logical-to-physical map cardinality and semantic type-layout plus LLVM layout twins |
| Construction/move-in | every variant writes one tag and only active Stored fields; OmittedUnit is evaluated then synthesizes valueless Unit without storage; Copy and Move payloads, nested records/arrays/strings; enum/option/result construction owners and optimized IR store-count owner |
| Move-out/source nulling | active payload moves clear only its source ownership state; union bytes need no deterministic zero; existing move and owned-match owners |
| Drop/replacement/return | tag-directed exactly-once Drop for every ordinal, old-value replacement after RHS, direct/indirect returns and cleanup payloads; large-drop, enum-drop, reassign, move-return owners |
| Control flow | if/match/else/?/map_err, wildcard/or-pattern, branch/loop joins, early return and divergence; value-control and tagged-match owners |
| Serialization/native | JSON encodes/decodes only active fields; callbacks and task/error slots agree; foreign layout(C) rejection remains; JSON, task-group, callback, FFI negative owners |
| Whole/per-unit/cache | imported and generic definitions produce equal layout; compiler/LLVM/target/type edits miss the right cache; per-unit/interface/inprocess owners |
| Allocation/provenance | no new runtime allocation; nested owned payload keeps heap/arena owner and active-tag lifetime; return-provenance and allocation parity owners |
| Performance | exact sizes equal formula; small-variant construction has O(active payload) stores; local size/store measurement and direct-layout reverse controls |

### 5.2 PR 2: drop-state effect and simplification

| Cell | Required closure and owner |
| --- | --- |
| Formation/validation | exact params.len vector; all four v13 u8 tags; arity/mode/type/tag/order, generic/concrete and resolved-droppability checks; malformed authenticated records always reject, while only unavailable internal analysis and unknown callable edges become MayChange; independent encode/decode byte goldens and interface schema owners |
| Fixed point | direct chains, mutually recursive SCCs, imported facts, local/imported generic templates instantiated with Copy/non-droppable and Move/droppable arguments, invariant recursion, one changing edge, unknown indirect edge; parameterized MIR analysis owner |
| Move-in/out | move, replacement, conditional release, and delegation mark MayChange; plain mutation without ownership change remains Invariant; ownership-operation sweep tripwire |
| Call edges | direct/imported/generic use specialized ABI; function values, mixed target joins, closures and raw/native edges use conservative ABI; adapter identity/dedup owner |
| Entry/return | Invariant has plain ptr and no proxy/load/writeback; MayChange retains pair and exact writeback on normal/early/?/map_err exits; codegen ABI owner |
| Sweep folding | known live/dead states fold at branches and agreeing joins without losing exceptional-edge records; MIR structural owner plus executable Drop count |
| Nulling | only proved dead MoveOut nulling disappears; InitializeEmpty, runtime-selected, read-after, partial reinit, loop/join and malformed cases retain it; zeroing IR owner |
| Whole/per-unit/cache | serialized effects and adapters agree across unit boundaries and invalidate interface/codegen caches; per-unit and inprocess owners |
| Allocation/provenance | flag ownership remains caller-side; no heap change; arena, builder-freeze and exactly-once Drop owners |
| Performance | invariant witness has zero cleanup pair/proxy/load/writeback; caller flags become promotable; local IR and assembly counts with mutating reverse control |

### 5.3 PR 3: transport

| Cell | Required closure and owner |
| --- | --- |
| Formation/validation | full target context, sizes/alignments/address spaces, parameter register pressure, split cleanup forms, malformed/multi-invalid refusal before mutation; classifier unit owners |
| Definition/call agreement | direct/imported/generic/recursive, function values, captures, entry/export wrappers, whole/per-unit and partitioned emission; signature assertions and native cross-link matrix |
| By-value parameters | direct small controls; target-selected byval large forms; caller isolation, exact attributes and no callee entry rebuild; parameter transport owner on x86-64 and aarch64 |
| Cleanup results | direct pair, void sret value+cleanup_out, direct value+cleanup_out; canonical byte, normal-return initialization and terminating-call absence; return-transport owner |
| Destination placement | plan 67 fresh whole-local result placement composes with split cleanup; replacement, field, element, joins, multiple uses and aliases retain fallback; parameterized materialization owner |
| Tagged consumption | tag-only load, payload load only in selected arm, inactive storage unread; optimized IR owner for Result/Option/user sum |
| Move/Drop/replacement | source nulling after committed transfer, old destination intact through RHS, exactly-once cleanup on every transport; existing replacement and ownership owners |
| Native boundaries | runtime/extern/callback/raw descriptors retain their ledgers; no accidental marker selection; FFI, DB, task and callback owners; local DB verification if classified |
| Artifact/cache | no physical LLVM handle serialized; target/LLVM/CPU/type/effect changes select the right plan and cache entry; ThinLTO, per-unit and inprocess owners |
| Performance | whole-local cleanup result has no `{T,i1}` scratch; large byval has no field rebuild; fixed witness region/copy counts fall with small/direct and fallback reverse controls |

### 5.4 PR 4: fresh-value destination construction

| Cell | Required closure and owner |
| --- | --- |
| Formation/validation | destination is an unescaped fresh whole local or caller sret result with exact type, size, alignment and ownership plan; replacement, fields, elements, joins and aliases reject optimization and retain fallback |
| Construction | source-order once-only field evaluation and store; Copy/Move, nested owned fields, Unit and empty fields; whole completion flag only after the final field |
| Reached exits | `?`, explicit early return and other reached control transfers drop exactly initialized Move fields in reverse source order before leaving |
| Terminal paths | bounds/division and other hard traps, abort, and divergence have no successor cleanup and preserve the settled no-cleanup-after-termination rule |
| Move/Drop/allocation | no partial value is published; successful construction transfers each owner once; allocation/free parity and arena exit remain exact |
| Whole/per-unit | generic literals and caller sret destinations use the same MIR construction record and ownership plan in whole and per-unit compilation |
| Performance | eligible fresh Move literals have no full aggregate scratch or second field-store sequence; fallback and Copy reverse controls remain |

One invariant-level owner may close several cells when it would fail for the
same defect. New fixtures are required only where existing tests would not
detect the changed invariant. Benchmarks remain local evidence and do not
replace correctness owners.

## 6. Acceptance and delivery

Before each implementation PR, perform the author-side matrix-to-diff pass and
the Align compiler self-review. Run the narrow owners named by the changed
cells, then one fresh independent full-diff review and final-SHA preflight.
Target ABI work requires native owners on Linux x86-64, Linux aarch64, and
Apple Silicon through the existing platform matrix; do not infer one target's
answer from another.

The provider measurements close when:

```text
PR 1  tagged allocation size is tag + maximum payload (with ABI padding), and
      constructing a small variant writes no inactive payload
PR 2  invariant mutable borrows expose no cleanup pointer or round-trip, and
      proved-dead MoveOut storage receives no zero-fill
PR 3  eligible cleanup-bearing returns write T directly to the final slot,
      target-indirect parameters use byval, and tag tests avoid whole loads
PR 4  eligible fresh Move values construct once in final storage, with reached
      early exits dropping only initialized fields and terminal paths unchanged
```

Client counts in the issues are external qualification. They are remeasured
after the provider invariants pass, with compiler hash, LLVM identity, target,
CPU/features, profile/LTO mode, corpus revision, repetitions, and output digest
recorded. A changed digest, Drop count, allocation parity, error, or hard-error
order is a correctness failure regardless of a favorable performance number.

## 7. Author consistency pass

The ledger-to-prose pass is complete:

- every physical record has an exact type, inputs, defaults, validation order,
  ownership, allocation, owner, identity, prerequisite, acceptance owner,
  benchmark rule, and mirror set;
- the complete discriminator product is covered: three tag families,
  payload/no-payload and every variant; all four drop-state cells across
  generic/concrete and direct/indirect calls; direct/indirect T,
  cleanup/no-cleanup, and every destination class;
- the v13 interface format fixes the drop-state field position, u32 sequence
  width, all four u8 tags, exact cardinality and validation order, with
  independent semantic-to-byte and byte-to-semantic golden vectors;
- structural identities include the reachable type graph and effect vector;
- no operation changes process-global or connection-global state;
- there is no text/view wire input, encoding rule, embedded-NUL case, ambient
  configuration, runtime inspection surface, or new source example in scope;
- the four-PR order consumes no later capability, and each capability leaves
  one usable stable consumer; and
- the matrices name every formation, move, nulling, Drop, replacement, return,
  control-flow, generic, interface, allocation, and ABI cell required by the
  repository implementation gate.

If review changes a type, validation order, representation, effect meaning,
transport form, or destination boundary, update this ledger first and repeat
this pass before implementation.

## 8. Design review ledger

The fresh independent review of candidate `09685d55` found two P1 omissions.
Both changed the public record, so this revised ledger requires a fresh review
before implementation.

| Finding | Ledger correction | Closing owner |
| --- | --- | --- |
| A generic BorrowMut parameter may become droppable only after substitution, so a template vector could not select one physical ABI. | The four-state vector now gives every template BorrowMut a Deferred cell and no callable ABI. Each local or imported template is instantiated in the consumer, then its concrete vector is derived from substituted types and checked MIR before the complete concrete call-graph fixed point. | Copy/non-droppable and Move/droppable substitutions of local and imported templates, with whole/per-unit physical-signature equality and rejected Deferred concrete records. |
| The persisted IFnSig addition lacked exact bytes. | Interface format v13 fixes the field immediately after return_cleanup, an exact params.len u32 count, u8 tags 0 through 3, validation order, hash/cache participation, and independent encode/decode goldens. | Extended parameter/certificate semantic-to-byte golden, independently assembled byte-to-semantic vector, and mutated length/tag/mode/body/droppability rejection matrix. |

The reopened-axis review of candidate `4e4f74fa` found that the first revision
still mixed two implementation failure domains and omitted one physical type
class. This is a boundary redesign rather than another local patch:

| Finding | Redesigned boundary | Closing owner |
| --- | --- | --- |
| Unit payloads had semantic size zero but no exact LLVM representation. | PR 1 now owns an exact logical-to-physical payload map. Unit is OmittedUnit with no LLVM field or storage; every other payload is Stored with fixed offset/size/alignment. | Option/Result/user-sum Unit-only and mixed Unit/non-Unit layout twins, construction, projection, match, Drop and malformed-map owners. |
| The PR 2 matrix still described malformed interface records as a fallback. | Authenticated v13 shape/tag/mode/droppability errors reject everywhere. MayChange is limited to unavailable internal analysis and unknown callable edges. | Mutated artifact rejection matrix plus unknown-edge conservative control. |
| Destination construction promised cleanup after trap/divergence. | ABI transport remains PR 3. Partial destination initialization becomes PR 4, whose cleanup edges exist only for reached control transfers; terminal paths preserve no-cleanup semantics. | Separate early-return/`?` Drop owners and hard-trap/abort/divergence no-successor-cleanup owners. |
