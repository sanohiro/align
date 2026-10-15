# Mutable output retention and mutation authority

## Reproduced defect and capability boundary

On merged provider984583ca, updating only `Row.number` through `borrow mut Row`
for `Row { number: i64, text: str }` erases the unchanged `text` dependency.
Consuming its string owner before reading `row.text` then passes checking.
The same compile-only failure occurs with an owned numeric array beside `str`
or `array<str>`. No-call and no-op-call controls reject. Do not execute these
negative programs. A separate existing false refusal rejects a known function
value updating an ordinary owned numeric-array record (Plan194/R147).

Both problems meet at the mutable-output summary. Its self-only root set is
currently a no-mutation sentinel, so the producer deletes self roots after any
mutation. That loses unchanged sibling contents. Its may-unmodified branch
union can also certify a mixed unchanged/replacement join as unchanged.
`mutable_access` cannot replace the sentinel: it includes exclusive access even
for a no-op call. Separate output dependencies from mutation authority in the
existing summary, repair its producer and all consumers together, then reuse
that complete record for a uniquely resolved function value.

This is one safety capability, not a dormant producer/consumer split. A new
record without every checked-HIR/interface/call consumer would be unusable or
unsound. If migration exceeds 1,000 handwritten lines, this atomic boundary
avoids duplicate certification, codec and cache proof. It adds no syntax, runtime
ABI, allocations, new mutable place, or partial Move-field admission. General
K1, captured/multiple-target mutable-output precision and partial exclusive
Move fields remain deferred.

## Authoritative public analysis record

| Surface | Exact contract |
| --- | --- |
| Root | Existing `MutableRetentionRoot::{Contained(u32), Storage(u32)}` with tags 0 and 1. Ordinals refer to logical parameters, including lifted captures, never process-local IDs. |
| Destination | `MutableRetentionDestination { may_mutate: bool, roots: Vec<MutableRetentionRoot> }`. The flag is independent of root presence. For BorrowMut it is true iff the returning-path analysis observes a possible actual mutation or an unavailable call effect; false proves every returning path leaves the place unchanged. Non-BorrowMut rows have false, including Out, whose existing content transition does not use this flag. Nonmutable rows also have empty roots. |
| Summary | `MutableRetentionSummary = Option<Vec<MutableRetentionDestination>>`. None is unavailable; Some has exactly one row per logical parameter. Generic templates carry None. Local concrete bodies infer the record; only producer-certified imports may supply it. |
| Root semantics | Roots are possible retained dependencies on returning paths, including unchanged siblings. A root set never certifies no mutation. Different-argument Contained/Storage substitution keeps the existing frozen whole-value/storage meaning. For an exact BorrowMut Contained(self) source, substitute the completed destination's borrowed contents, including view backings and nested borrowed elements, without making its owned allocations depend on their displaced release identities. Storage(self) still selects the full frozen storage snapshot: a callee can install a new view of its own owned field, absent from prior borrowed contents. Ambiguous self-storage relations remain conservative/rejected until a separate projected-origin summary proves them. Out self-Contained keeps its existing element-only meaning. Unavailable records keep the existing conservative all-input fallback, not exact-self precision. |
| Self contents | Resolve the typed value and its generation contents while excluding only the lifetime introduced by an owned header at that header's occurrence. Recurse into owned contents, and keep lifetimes reached through view/carrier headers, legacy borrowed leaves, fallback/ended/read-only/validation roots and opaque parent dependencies. Do not erase a flattened set of matching roots globally: the same allocation can occur as an owner and as a borrowed dependency. Unknown header kinds cannot justify exclusion. Use this occurrence-based resolver at the output producer as well as the caller. An initial CallerStorage generation whose directory descriptor is View denotes pre-call borrowed contents and maps its self lifetime to Contained(self); a newly formed view into a directory-owned generation remains Storage(self). Unknown or ended identities never authorize that mapping. Header-managed legacy parameter-storage seeds must not reintroduce an omitted owner lifetime; preserve legacy provenance outside those managed paths and reconstruct managed paths from authoritative generation contents. The exact completed fact is frozen before later eager arguments. Do not copy self-owned headers into replacement-generation contents; retained view dependencies remain represented. |
| Mutation producer | Replace may-unmodified state with may-mutated state: empty on entry, add on actual writes/replacement/native source mutations or potentially mutating call actions, union at reachable joins and returning exits. Exact self-assignment stays unchanged. Audit every existing mark site and source-visible mutation descriptor. An Out action marks every enclosing BorrowMut parameter whose storage it can reach, despite its own false flag. By-value/captured writable slice effects, pipeline callbacks and map_err substitute their existing authenticated access facts into mutation state conservatively where a more precise mutation fact is unavailable; unavailable access marks every possible reachable BorrowMut input. Only this path-sensitive state contributes returning output flags; task/access observations remain independent and cannot introduce effects from a solely non-returning branch. Ordinary exclusive access alone is not a write proof. No-return paths produce no output; initial inference facts are not published before the joint fixed point converges. |
| Caller | Only a validated false may_mutate BorrowMut row suppresses whole-place replacement. Exclusive argument/reservation, readonly, alias, live resource-dependent and pre-call-observer checks still run. A true row preserves all possible borrowed contents while advancing existing owned generations. Existing plan80 view effects remain the authority for rebind/original-backing writes; Out and resource transitions remain distinct. |
| Function values | Forward only one known noncapturing target with complete compatible rows. Unknown/empty/multiple/captured-arity/malformed targets retain unavailable handling. Both MoveCheck and EscapeCheck select the same record. Changes to retention roots or mutation flags requeue indirect callers. No function-value Out admission. |
| Encoding | Interface format20 replaces each format19 retention row's root sequence with one canonical bool byte (0/1), then the root sequence. Summary option: u8 0/1; row count and root counts: u32 LE; root: u8 tag then u32 LE ordinal. Row order is parameter order; roots strictly sorted by (tag, ordinal), unique, in range. Unknown tags/bools/version, truncated records, wrong arity/mode, noncanonical roots, uncertified imports and generic Some records reject. Decode structural errors follow byte order; semantic validation checks outer arity, then each row's source range, strict order, destination-mode roots and flag/mode restriction, then generic prohibition. Interface-header version rejection precedes records; all in-memory consumers use the same semantic validator. No old-format compatibility path. |
| Identity and ownership | The existing interface hash includes the complete record and format version, including private body changes to the public inferred flag. Vectors are compiler-owned data; no runtime ownership, user allocation, environment input or native/wire text changes. Validation precedes use. Existing checked-HIR body replay resets, recomputes and compares the whole record; MIR removes it. |
| Sources and prerequisite | Existing shipped mutable-call and checked-HIR contracts; no future milestone prerequisite. Update Plan17 Request43/49 exact record, Plan08 section15.4, Plan19 retention/call/replay rows and relevant interface-format wording. No language spec decision changes or bilingual public API changes. Record completion once in HANDOFF. |

## Implementation closure matrix and owner checklist

| Axis | Closure and owner |
| --- | --- |
| Formation and malformed records | Shared HIR validator checks complete row shape, flag/mode, roots, arity and certification. Codec independent semantic-to-byte and byte-to-semantic goldens cross None/empty/exact rows/flag states; reject every truncated prefix and bad bool/tag/order/range/mode/generic combination. Reuse interface summary and MIR forged-import owners. |
| Construction, move-in/out, Drop, return and replacement | Preserve borrowed siblings for Copy records, mixed Move records and arrays of borrowed views, nested records and caller-owned formals. Fresh whole replacement clears the old borrowed dependency. Native owner checks no-op, indexed/field/root replacement and normal/error cleanup with a positive live-allocation witness and no dispatch allocation. Existing borrowed_replacement owns cleanup-mode/nulling behavior. |
| Source lifetime | Compile-only negative matrix: retained string owner consumed/replaced/short-region exit after a different-field update; direct and singleton indirect; local/import/generic/forwarder; whole/per-unit. Positive twins keep the owner live; fresh whole replacement permits the former owner to end. Storage/view siblings and nested borrowed collection contents stay live. The new self-view formation (`r.view = r.a[..]`), saved-view extraction, owned-field replacement and subsequent use must reject; ambiguous self-Storage is never treated as old contained provenance. |
| Mutation authority | Unchanged versus indexed/field/root replacement, mixed no-op/write branches and loops, early exits, direct/import/known indirect. Pre-call views reject after any possibly replacing branch. Exact no-op keeps the destination usable but preserves existing exclusive-observer/resource restrictions. Reuse source-visible mutation owners and add discriminating tests for mixed branch and forwarding flags. Cross Out-to-BorrowMut forwarding (direct/imported/forwarded), writes through by-value/captured slice aliases, pipeline callbacks and map_err. Include a write only on a non-returning branch, proving it does not contaminate returning-output flags. |
| Self-content freeze | Owner unit test crosses owned/view/unknown descriptors, same generation reached through owned and borrowed occurrences, nested content, cycles, fallback and ended roots. Caller source tests cross later eager source replacement, alias writes, multiple mutable outputs and saved old views; all updates read completed snapshots. No global owner-root subtraction. |
| Control paths | if/match/else/?/map_err, zero/repeated loops, early return, no-return alternatives, mutually recursive forwarding and declaration order. Only reachable returning edges contribute output flags/roots. Existing control owners cover unaffected joins; new targeted flags/retained-sibling cases detect the changed algebra. |
| Function targets | One helper-level parameterized owner validates exact target selection, flag preservation and unavailable/empty/multiple/missing/captured/malformed fallback. Source matrix covers aliases and same-target if selection. Multiple-target no-op/replacement keeps fallback and cannot preserve an old allocation. |
| Whole/per-unit and generic/cache | Driver owner exercises known imported function values and concrete generic forwarding, checked-HIR replay, MIR producer validation and cold/hit/edited/restored package cache. Public interface hash changes when a private helper changes roots or may_mutate; forged stale local flags and roots reject replay. |
| Unchanged boundaries | No new mutable place/type/lifetime syntax, FFI/runtime/codegen strategy, ownership mode or parallel behavior. Reuse dynamic_array_fields, imported_mutable_retention, return_provenance and borrowed_replacement owner targets. No performance/resource promise and no benchmark. |

## Review gate

The author-side matrix identifies a changed safety strategy and exact interface
record. Obtain one fresh inspection-only adversarial review of this ledger and
capability boundary before implementing the safety repair. The existing
uncommitted singleton-consumer prototype is not deliverable on its own. Resolve
plan findings coherently, then implement and perform one matrix-to-diff pass,
owner checks, self-review, independent preflight code review and normal gates.

## Independent plan review resolution

The fresh inspection found two P1 strategy gaps before implementation. Both are
closed in the ledger above: Storage(self) retains full frozen storage semantics,
including newly created self views; and mutation authority follows Out and
captured/by-value writable aliases into enclosing BorrowMut parameters. Neither
fix admits a new mutable place or projected-origin summary. The implementation includes both closures and the complete singleton consumer.

Independent field golden vectors (hex; bool precedes each root sequence):

```text
None: 00
Some([]): 01 00000000
Some([unchanged empty, ordinary empty]): 01 02000000 00 00000000 00 00000000
Some([mutating Contained(1), ordinary empty]): 01 02000000 01 01000000 00 01000000 00 00000000
Some([mutating Storage(1), ordinary empty]): 01 02000000 01 01000000 01 01000000 00 00000000
```
