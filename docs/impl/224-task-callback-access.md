# Task callback access requirements

## Reproduction and capability

`apps_web_root::the_serve_loop_answers_request_after_request` fails at
`pkg.web.serve`'s spawn. The existing fixed point assigns worker
`Contained(2), Storage(2)`: opaque route callbacks conservatively access their
environments and all supplied arguments. `pkg.ws` imports this rejected module.
A route can contain a closure writing borrowed backing. Dropping unknown effects,
recovering only known targets at a mixed join, or choosing one worker is not a fix.

Restore the existing direct package API and concurrent workers using producer-owned
callback requirements and result-flow facts. Do not mutate imported nominal types,
introduce a package-name exemption, bypass capture checks, or copy user data. No
runtime ABI/layout/allocation change or performance claim is included.

## Authoritative analysis record

All records are compiler-owned, inferred and unwritten in source. Logical parameter
ordinals include lifted captures after explicit parameters. Function-value argument
ordinals always count explicit arguments only. Projection paths use the existing
callback leaf traversal: StructField(u32), TupleElement(u32), EnumPayload(u32 variant,
u32 element), OptionSome, ResultOk, ResultErr, ArrayElement. Every path must resolve
to exactly a callable leaf in the declared parameter/result type; enum arms remain
distinct and array elements conservatively share one cell.

| Record | Exact meaning |
| --- | --- |
| CallbackInput | `{ parameter: u32, path: Vec<CallbackProjection> }`, relative to the owning function. |
| CallbackRead | `Environment` or `Argument(u32)`. Environment excludes writes/exclusive access to captured backing; Argument excludes contained backing and exclusive storage access to that argument. I/O and fresh local writes remain allowed. An explicit BorrowMut/Out argument never satisfies Argument, even for a no-op target. |
| CallbackRequirement | `{ input: CallbackInput, read: CallbackRead }`. Every possible callable at that input leaf must satisfy the read obligation. Unavailable evidence never satisfies it. |
| CallbackEffects | `{ environment_write: bool, argument_writes: Vec<u32> }`. A complete conservative may-write/exclusive-access fact for a callable value. Empty and false prove read-only. Argument indices are sorted unique and in range; mode-based exclusive accesses are included. |
| CallbackSource | `Parameter(CallbackInput)`, `Known(CallbackEffects)`, or `Unknown`. A returned leaf records all possible sources. Known cases may be joined by union of writes and OR of environment_write; Unknown absorbs other cases for proof purposes, never disappears at a known join. |
| CallbackReturn | `{ path: Vec<CallbackProjection>, sources: Vec<CallbackSource> }`. One row per callable leaf of the return type, in canonical path order. Empty sources mean a statically absent payload, never an unavailable present value. Missing/unresolved facts use Unknown. Returned formal callbacks retain their exact source path; constructors substitute completed actual facts across unit boundaries. |
| ConditionalAccess | `{ requirement: CallbackRequirement, roots: Vec<MutableAccessRoot> }`. These roots may be accessed unless the identified callable satisfies the read obligation. Roots retain existing Contained/Storage meanings. A coincident unconditional write remains unconditional. |
| MutableAccessEffects | `{ direct: Vec<MutableAccessRoot>, conditional: Vec<ConditionalAccess> }`. `MutableAccessSummary = Option<MutableAccessEffects>` replaces the old vector. None still accesses all compatible inputs. Ordinary access/retention consumers conservatively union direct and unresolved conditional effects. Task checking may discharge a conditional only with complete callable proof or an exported requirement. |
| CallbackContract | `{ requirements: Vec<CallbackRequirement>, returns: Vec<CallbackReturn> }`; optional on function records. None is unavailable, not empty. Concrete source producers infer Some; generic templates carry None and concrete instances recompute. Imported Some requires ordinary producer certification. |

`serve`, not `pkg.web.types`, owns the route callback obligations. Its contract
names parameter 2 and the callable paths through Route/Handler/MiddlewareList.
`get`, `route` and other constructors independently publish return-flow sources;
their contracts do not depend on a later importer. Imported nominal definitions,
ordinary function-type identity, nested callable equality and writable container
element types remain unchanged. This adds no directional callable subtyping.

## Inference and safety authority

Use per-function/local/result callback leaves, following `EffectBoundaries`, rather
than shared nominal type or globally interned FnTy cells. Seed formal leaves as
Parameter sources, exact named values as Known effects, and unavailable producers
as Unknown. Explicit constructors, projections, assignments, branch/loop joins,
returns, captures, generic instances and direct constructor calls preserve sources.
An indirect result without a complete source mapping is Unknown.

Access inference carries unconditional roots separately from conditional causes.
At an opaque invocation, each completed argument and the environment produce their
own read obligation and possible roots. Resolve Known sources immediately; retain
Parameter causes; Unknown gives unconditional conservative access. Direct helpers
substitute both parameter roots and callback-source paths. Persist conditional
access so an imported helper has the same behavior with no body available.
An earlier conditional write capable of changing callback contents remains a
guard on subsequent conditional access and task requirements. A return after such
an unresolved write publishes Unknown instead of an unconditional guarantee.

A spawn rejects unconditional captured writes. A conditional captured action adds
a requirement on its formal source, or proves a Known source compliant, or rejects
Unknown. A direct call checks the callee's requirements against completed actual
callback facts; formal sources forward the requirement to the caller. Requirements,
return-flow facts and access summaries reach one finite may fixed point before
publication. A copied callable payload keeps its completed value. A view keeps the
completed descriptor, but callback contents reached through that descriptor must
be revalidated at invocation after every eager operand; rebinding a local must not
redirect this check to a different view.

Unknown by-value arguments select mutable reachable backing by typed occurrence,
not copied local headers or immutable str bytes. A mutable slice keeps its backing
even with str elements. Ctx.body remains writable request-local storage while
route-derived prefix/pattern strings cannot become mutable authority. Explicit
BorrowMut/Out always select storage. Unknown/cyclic/malformed types fail closed.

Callback facts must not outlive their storage proof. Union every direct callback
assignment. A write/exclusive call capable of replacing callback-bearing backing
invalidates reachable callback contents before a subsequent read. A conservative
fallback may invalidate all already-live local callback contents when exact alias
identity is unavailable; it must not preserve a known initializer across an unknown
alias write. Fresh construction and completed earlier directly copied callable snapshots remain
separate. Invalidation also reaches already-completed operands whose callback facts
depend on referenced contents. Branch/loop observations never regain a discarded
guarantee without a new independent value construction. Branch and loop joins retain an invalidated alternative. This is analysis
precision only; no runtime freeze, clone or ownership change is introduced.
Borrowed parameter places, borrowed match bindings and completed borrowed arguments
are also replaceable contents, even without an ArrayElement projection. Invalidation
reaches those places while preserving already-copied by-value callbacks. A call's
returned view uses post-action contents. Missing projections after a recursive type
cut are Unknown; only an explicit absent constructor proves an empty payload.
All expression walkers share completion publication. Both loop-break walkers,
explicit/transparent returns and `?` error exits contribute callable result sources.
Tuple projection and binding retain the same HIR flow; source-level callback-bearing
tuple cases remain excluded by the existing tuple element restrictions.

### Higher-order boundary

This capability does not add a new higher-order function-type contract or variance.
Taking a function value whose target has callback requirements on explicit
parameters is rejected; call that function directly. This prevents an opaque
higher-order call from hiding a newly constrained task consumer behind an ordinary
unconstrained function signature. Existing unconstrained sequential higher-order
functions retain their behavior. Named and lifted function-value construction use
the same check, including imported targets and known/unknown joins.
Pipeline stage and terminal targets use this formation rule even though their HIR
stores a target name directly. `map_err` uses its ordinary callable operand.
An unavailable contract rejects a consumer of callback-bearing inputs, including
pipeline elements, captures and accumulators; callback-free scalar consumers keep
their existing effect checks.

A closure's requirements on lifted capture parameters are checked against completed
captures, or forwarded to the enclosing function's formal sources. Directly copied
callable payloads and independent inline contents retain this proof. Ordinary
reusable or returned closure formation is rejected when discharge depends on
replaceable callback-bearing referenced storage. Immediate literal task construction
is checked at the spawn action, after all capture operands, when existing pending
capture protection begins; this admits the route-slice capture in `serve`. Its requirements on explicit parameters still reject ordinary value
formation. A literal zero-argument task has no explicit parameters and checks its
captures through the same authority. A returned closure cannot carry an undischarged
capture requirement. Callable results that cannot provide complete guarantees stay
Unknown, preserving fail-closed behavior rather than adding general effect polymorphism.

## Canonical representation and validation

Interface21 changes the existing mutable_access field in each function record from
an optional root vector to optional MutableAccessEffects, and inserts the optional
CallbackContract immediately after it. It changes no IType::Fn encoding. Existing
function/cache identity includes both complete fields and the version. Generic
None records and unavailable/malformed imports remain fail-closed.

All counts/ordinals are u32 LE, option and bool bytes are canonical 0/1. Paths are
u32 count then tags: 0 StructField+u32, 1 TupleElement+u32, 2 EnumPayload+two u32,
3 OptionSome, 4 ResultOk, 5 ResultErr, 6 ArrayElement. CallbackInput is parameter
then path. CallbackRead tags are 0 Environment, 1 Argument+u32. CallbackRequirement
is input then read. CallbackEffects is bool environment_write then counted argument
ordinals. CallbackSource tags are 0 Parameter+input, 1 Known+effects, 2 Unknown.
CallbackReturn is path then counted sources. ConditionalAccess is requirement then
counted existing roots (tag0 Contained/tag1 Storage followed by ordinal).
MutableAccessEffects encodes direct roots then conditional rows. CallbackContract
encodes requirements then return rows. Ordering is lexicographic in the declared
field/tag order; all sets are strictly sorted and unique. Matching conditional rows
merge their roots. Matching return paths merge sources. No process-local identity
or source pathname enters these records.

Validation order: version and byte-order decoding/truncation/tag/bool validation;
record structure, canonical ordering, parameter ordinal ranges and generic prohibition
at decode/import admission; then, after dependency types are reconstructed, producer
certification, parameter/result path resolution, argument ranges and mode restrictions,
complete return-leaf inventory and exact source/result callable-signature compatibility
(including modes, arity and nested identity). Semantic replay follows these checks.
No record is used before validation. Replay resets
and recomputes every new fact, compares full records, and rejects forged requirements,
return guarantees and conditional effects before MIR. Independent bytes include:

```text
unavailable access: 00
empty access: 01 00000000 00000000
unavailable callback contract: 00
empty callback contract: 01 00000000 00000000
parameter 2 root callable, Environment requirement: 02000000 00000000 00
Known callback, no writes: 01 00 00000000
Unknown source: 02
```

## Implementation closure matrix

| Axis | Authority and discriminating owner |
| --- | --- |
| Type formation / nominal identity | Per-function callback leaves and producer-owned contracts. Three separately compiled modules (types, constructor, task consumer), unchanged nominal hashes, cached constructor body changed from reader to writer. |
| Construction, copying, replacement, return | Complete callback-source flow and alias invalidation. Direct/field/index/constructor-return readers and captured/by-value writers; mutable aliases replacing a formerly safe callback; saved direct callable snapshots and fresh controls; alias replacement between closure formation/use or in a later eager operand, returned dependent closures, and immediate literal task publication. |
| Task access / lifetime | Existing action authority plus conditional causes. Extend `task_capture_lifetime_and_access_matrix`, `task_capture_physical_storage_and_pipeline_matrix` and `task_progress`; preserve unconditional writes, explicit exclusive calls, caller pending writes, frame reuse and post-wait controls. |
| Control and generic flow | Parameterized if/match/else/?/map_err/loop/replacement/return/capture forwarding. Mixed safe/unsafe and known/unknown target negatives; existing callable availability/return-provenance negatives. |
| Higher-order formation | Explicit-parameter obligation function values reject; direct calls work; captured requirements discharge before closure publication. Local/imported/returned/field and nested opaque forwarding negatives. Ordinary sequential writer callback remains accepted. |
| Whole/per-unit/import/cache | Driver accepted/rejected dispatch families in both modes, exported constructors, generic wrappers, cold/hit/edited/restored interfaces. No provider-local example can certify an unseen callback. |
| HIR/interface/MIR | Shared semantic validator; independent codec bytes, all truncations, wrong tags/order/paths/arity, incomplete leaf inventory, stale and forged replay, complete interface/cache schema goldens. |
| Ownership/runtime | No runtime strategy, allocation, move/nulling or Drop change. Existing task cleanup/early exit owners and request-local byte-write positives retain the boundary. |
| Packages | Existing web root/serve/prefork/middleware/stream/upgrade/multipart and ws/close owners compile and execute. Long stream plus other workers stays required. Required bounded checking must include the real package import shape. |
| Upgrade reset reporting | The restored `apps_ws_close` owner exposes Darwin timeout rearming after peer reset: `setsockopt` returns EINVAL while SO_ERROR retains ECONNRESET. On EINVAL only, read a checked integer SO_ERROR; a nonzero native error owns failure, otherwise retain the original option failure. Never report successful arming after failure. Both read/write loops poison and close through their existing shared failure transition; ABI, allocation, deadline and Drop stay unchanged. `http_upgrade_reset_error_survives_deadline_rearm` covers both directions, sticky replay, healthy/no-pending and failed-query controls; the full `apps_ws_close` owner verifies wire behavior. |
| Release | Restore web/ws to prebuilt corpus after owners pass. Local cache/layout/parity checks, all platform release gates and actual published artifact checks. Synchronize English/Japanese compatibility caveats. |

The boundary may exceed 1,000 handwritten lines: source flow, all access consumers,
serialized certification and caller enforcement form one safety capability. Splitting
a dormant producer from its consumer would duplicate proof or permit unsafe dispatch.
No benchmark is required because the change makes no performance/resource promise.

The author consistency pass binds every record to formation/flow and codec owners,
conditional effects to task/control owners, and unchanged ownership to lifecycle
owners. The first independent strategy review rejected nominal/FnTy strengthening,
missing constructor transfer, unspecified variance/alias rules and a nonexistent
codec boundary. This revision resolves those findings with per-function contracts,
explicit return sources, no new subtype relation, alias invalidation, and exact IFnSig
placement. The focused continuation identified stale capture/view observations; the
final revision above distinguishes stable copied payloads from replaceable contents,
rejects dependent reusable closures, and checks literal tasks at publication. It also
requires exact callable compatibility on parameter-to-return source mappings.
The original dirty main-worktree patches remain separate evidence.
