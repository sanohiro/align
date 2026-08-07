# History of Align

## 2026-08-07: shared borrow accepts stable Copy storage

Shared `borrow` now accepts a stable bound Copy or Move place. Copy values still pass by value by
default; explicit borrow is the visible no-copy ABI for large structural values and generated typed
callbacks. Temporaries remain rejected. This removed the need for a `pkg.db`-specific exception in
the Q2 binder ABI.

## General library boundaries and Query-centric databases

Database design exposed several gaps that were already recurring in HTTP, networking, process, and
other native libraries. Adding another list of compiler-known handle types was rejected
(2026-07-27). Database-name-specific ownership rules, runtime reflection, an ambient allocator, and
online normal builds were rejected for the same reason: they hide semantics or duplicate a general
language rule.

The chosen direction is:

```text
finite recursive DropPlan for tagged Move payloads
borrow / borrow mut parameter modes with interface summaries
package-defined opaque and dependent resources
owner-tied native views
named arena region capabilities
deterministic static source inputs and Query/command artifacts
region-backed plain-struct builders
nested generic package APIs with a closed RegionPlain bound
```

The same review fixed the boundary details before implementation: `borrow mut` may update a writable
Copy aggregate; parameter modes survive in function-value types; contextual `borrow`/`out`/
`resource` words keep intrinsic paths and `out: region` parseable; a resource producer emits a
hidden linkable Drop thunk for its `pub` internal raw hook; and each static Query constructor is the
single whole body of one uniquely named descriptor item. Arena-owned builder output is finalized
inline rather than passed through a forbidden by-value call. A follow-up soundness pass required
function values to retain and join return provenance, gave inline SQL a tagged item-based source
identity, and fixed migration replay to one canonical filename/version order.

The final database-contract review then closed the remaining implementation choices. Query and
command now share one statement-artifact/binder/cache mechanism; checked state is recorded per
permitted driver; the first-release option sums and milestone ownership are finite and explicit;
metadata/EXPLAIN copy exact flat records into a named region; migration SQL is atomic by default with
a one-statement dirty-state path for transaction-forbidden statements; and PostgreSQL is a
non-skippable merge/release CI gate. The compound examples also reject both partial-NULL child
shapes and keep many-parent output segmented.

A subsequent language-foundation audit found seven remaining prerequisites and made them explicit
rather than database-private work: generic package functions may compose `array<R>` and named
generic resources; Move returns carry a dynamic cleanup bit; mutable-borrow alias scans cover every
peer parameter mode; closure return provenance includes capture roots; replacement through
`borrow mut` drops the old pointee; raw resource transfer is root-only; and static-input manifests
key exact per-driver checked-metadata missing/present state.

The final consistency pass also closed native-boundary scope gaps. SQL and libpq Text/control
strings reject embedded NUL before native calls; the first PostgreSQL release maps only the fixed
integer/float/bool/text/bytea/Option set; and D9 provides enforced deadlines plus native
cancellation cleanup without inventing a public non-Send cancel handle.
The follow-up closes the recursive selector/root case, specifies Text `bytea` hex versus raw Binary
binding, requires post-cancel resynchronization or close, permits truthful Query-less contract
errors, and removes deferred logical types from first-release examples.
A final artifact/metadata closeout fixes the complete Query/command binary schema and independent
byte/digest goldens, and gives duplicate or unnamed constraints a canonical ordinal derived from
all Full-detail common fields rather than from the reported name or catalog order.
The final whole-diff gate additionally makes type fingerprints structural, gives runtime
`QueryMeta` a producer-owned plan/thunk, fixes the checked-metadata and migration/schema binary
identities with independent goldens, serializes SQLite connection-global execution through one
explicit lease, and checks exact metadata signature notation against the owning API table while
syntax-checking positional calls. Mutable
prepare targets carry an explicit non-secret schema ID; normal builds remain offline.

These are mandatory library-boundary prerequisites, not private database builtins and not optional
cleanup. `pkg.db` remains ordinary first-party package code above them. Its design stays SQL-native:
one named Query owns one visible statement, typed Params and exact flat Row, and ordinary Pure Align
code may shape that one row stream without receiving a database handle.

---

## The first idea

The project began with a simple observation.

> The same thing should not have many ways to be written.

This led to the following.

```text
one error model
one ownership model
one optional model
```

---

## The performance discussion

The focus shifted to the following.

```text
cache locality
allocation cost
memory layout
```

over raw instruction performance.

Observation:

The cache is often more important than SIMD.

---

## The turn toward data orientation

The discussion moved away from OOP.

Where it headed:

```text
array processing
SoA
hot/cold split
chunk processing
```

---

## The AI-era discussion

The big realization:

Programming is now this.

```text
Human -> AI -> Compiler
```

This changed the priorities.

What the language should optimize for:

```text
convergence
predictability
consistency
```

over maximal freedom.

---

## Error handling

The exception-based approach was rejected.

Go-style explicit error handling was judged too verbose.

The direction chosen:

```text
Result<T,E>
?
```

---

## Memory model

The GC-first approach was rejected.

Rust-style visible lifetimes were judged too heavy.

The direction chosen:

```text
value types
arena
explicit heap
unsafe isolation
```

---

## The SIMD direction

The goal:

Not to make developers write SIMD.

But to make them write code that naturally becomes SIMD.

This led to the following.

```text
map
reduce
scan
mask
vec
```

These became core concepts.

---

## The string and JSON direction

Repeated scanning was identified as a major cost.

The direction chosen:

```text
scan once
reuse metadata
builder output
zero copy
field tables
```

Later (2026-07-18), when JSON was pushed to completeness, a **serde-style
recursive value tree** (`JsonValue { Null, Bool, Num, Str, Array, Object }`)
was considered for schema-unknown input and **rejected**: per-node heap
allocation and pointer-chasing are the cost model Align exists to avoid, and it
would have pulled recursive enums and a map type into the language. The chosen
form is the simdjson-style lazy document view (`json.doc`) — one SIMD scan into
an arena-backed tape, borrowed zero-copy views for navigation. Two other
catalog entries were rejected at the same time rather than left pending:
`validate<T>` (decoding and discarding is validation) and the SAX `token` tier
(no consumer; the view + streaming scan cover it).

---

## The compiler-friendly direction

Restrictions were added intentionally.

The goal:

To enable compiler inference.

Rather than requiring programmer annotations.

---

## Library structure

The final direction:

```text
core
std
pkg
```

core contains data-processing primitives.

std contains OS integration.

pkg contains frameworks and the ecosystem.

---

## Sequential control

For a long time the language had no loop construct at all.

Collection iteration was the pipeline; the rest was said to be recursion.

Recursion-as-iteration was rejected (2026-07-09).

The reasons:

```text
scope-end drops kill tail position
? kills tail position
TCO is invisible in source
loop back-edges are what compilers want
```

`for` and `while` were also rejected — `for` competes with the pipeline,
`while` is a second loop form that cannot yield a value.

The direction chosen:

```text
loop { ... break value }
```

One narrow expression. The pipeline owns the data path; `loop` owns the control path.

---

## Sequential pipeline effects

The implementation accepted Impure sequential callables while early implementation notes described
all data-processing callables as Pure. The conflict became observable when branchless `where`
speculated a later callable on a rejected element.

The direction chosen (2026-07-13):

```text
sequential pipeline  -> Impure allowed, exact guarded input/stage order
par_map              -> Pure required
```

Effect inference controls optimization legality. It does not reject ordinary sequential effects,
and Pure alone does not make a trapping or nonterminating call safe to speculate. `sort_by_key` key
evaluation remains separate because comparison sorting has a data-dependent call count.

---

## Naming

Several names were considered.

For example:

```text
Opt
Air
Bound
Fuse
Grain
```

The final front-runner:

```text
Align
```

The reason is that it expresses the alignment of the following.

```text
Human
AI
Compiler
Hardware
```

while also pointing to the following.

```text
memory alignment
cache alignment
SIMD alignment
```

---

## General library-boundary prerequisites for query-centric database packages

The query-centric database review settled a general language/library boundary direction
(2026-07-27). `pkg.db` does not import `std.http`; both packages use the same language-level
resource, borrow-provenance, owner-tied native-view, named-region, and deterministic-static-input
facilities.

The implementation sequence is deliberately prerequisite-first. L1a establishes the recursive
`DropPlan` framework but admits only `Option<string>` field leaves; L1b owns
`Option<MoveStruct>` and other finite Move tagged payloads. Indirect-call provenance includes roots
embedded in compatible by-value Move inputs. Resource Drop hooks resolve through ordinary fully
qualified module paths, and dependent construction plus checked raw views are explicit typed MIR
operations rather than LLVM or package-name exceptions.

The initial PostgreSQL Query vertical is explicitly `BufferedFull`: `one`/`maybe_one` decode at most
two delivered rows, but transport and native buffering may contain the complete result. Physical
delivery is measured and labelled separately; later single-row/portal modes are selected
capabilities, never silent substitutions.

The same review closed a call-site aliasing hole: `borrow mut` rejects not only spelled borrows but
every peer argument recursively carrying the invalidated generation, including by-value Copy views
and Rows. DB primitives have one mandatory option-slice form, mutable-borrow examples use mutable
bindings with unchanged call syntax, and both language mirrors agree. The first public database
release gate includes D11 migrations and D12 category metadata/EXPLAIN; D13/D14 remain committed
additive work.

Streaming execution also settled parameter retention: the common API releases source Params
provenance when the call returns, SQLite v1 uses measured transient text/blob bind copies, and
future asynchronous paths retain execution-owned bytes. Dynamic SQL carries an exact visible
`db.Driver`; metadata category calls carry their `MetaOption` slice. Core ledgers list L4/L6 forms
as required-but-unimplemented instead of calling them verified shipped signatures.

The final contract-ledger pass made several underspecified surfaces exact. `resource.borrow` is
public and safe while raw resource operations remain package-subtree privileged.
`db.exec_result` is the allocation-free Copy affected-row record. Migration tooling requires an
explicit entry, migration catalog, driver, and matching target on every live command. Metadata uses
exact `SchemaRef`/`TableRef` inputs and complete flat Column/Key/Index/Query records. Query
nullability is fail-closed: D0 records actual engine evidence, D3/D5 own the driver matrices,
ambiguous evidence is `Unknown`, and catalog `NOT NULL` alone never removes runtime NULL checking.
Metadata `Names`/`Summary`/`Full` and Query entry rows have one exact presence/order/ordinal/digest
matrix, including every Unknown state and Summary→Parameter→Column group order. The canonical
artifact codec serializes the fingerprints/ABI versions named by its digest, duplicate constraint
names use a canonical `key_ordinal`, and schema/table reference components reject U+0000 before
native access in declaration order.
The repository's design process now requires one public-contract ledger and an author-side
ledger-to-prose pass before independent review.
