# Align Design Notes

## Why Align exists

Align is not an attempt to invent new syntax.

Align exists because modern software development has changed.

The old model:

```text
Human -> Code -> Compiler
```

The new model:

```text
Human -> AI -> Code -> Compiler
```

Language design must reflect this reality.

---

## The four-way alignment

Align seeks to align the following four parties.

```text
Human
AI
Compiler
Hardware
```

Most languages optimize for humans alone.

Align treats all four as first-class citizens.

---

## The central observation

Modern CPUs are extremely fast.

Modern compilers are extremely sophisticated.

Modern AI can write code.

Yet developers still hand-optimize the following.

```text
allocation
cache locality
SIMD
branch prediction
parallelism
```

Align seeks to make the optimal path the default path.

---

## Less code

One of the founding beliefs.

> Less code means fewer bugs.

A language should remove boilerplate wherever possible.

But it does not hide the following.

```text
allocation
errors
parallelism
unsafe operations
```

These stay visible.

---

## Convergence over expressiveness

Many modern languages maximize expressiveness.

Align maximizes convergence.

The goal:

```text
different developers
different AI models
different codebases
```

naturally arrive at similar solutions.

---

## One way

Align strongly prefers the following.

```text
one error model
one optional model
one ownership model
one parallel model
```

Converge on one rather than several competing approaches.

This also applies to empty option lists. `[]` is not a second polymorphic "empty collection" type:
when a function or binding expects `slice<T>`, that context supplies `T` and the literal follows the
ordinary fixed-array-to-slice borrow path. With no expected element type it is rejected. The empty
case allocates nothing and needs no database- or package-specific compiler exception.

This is why **multi-value return is just returning a tuple**, not a separate mechanism. A
Go-style "multiple return values" feature produces several values that are not themselves a
value (you can't store, nest, or array them) — a second, special-cased way to hand back more
than one thing. Align instead has one first-class anonymous product type, the tuple `(T, U)`,
the positional companion of the keyword-less named struct: a named struct for a domain type, a
tuple for an ad-hoc result. A tuple's ownership falls out of its elements (the same rule as a
struct), so it adds no new ownership concept either.

It is also why there is **one filtering operation, `where`** — not both `where` and `filter`.
The two would be exact synonyms (keep the elements a predicate selects), and two names for one
operation is precisely the divergence Align rejects: different code would arrive at different
spellings of the same thing. `where` wins because it reads naturally with field selectors
(`users.where(.active)`) and fits the data-oriented, query-like core. (`filter` was dropped from
the function list for this reason.)

---

## Compiler-friendly first

Align is intentionally restrictive.

Restriction is not a weakness.

Restriction becomes information for the compiler.

The compiler should be able to infer the following.

```text
contiguous memory
no alias
cold error path
arena lifetime
non-null values
```

without requiring complex annotations.

---

## Hardware-friendly first

Performance begins with the cache.

Before SIMD.

Before GPU.

Before parallelization.

Key concepts:

```text
contiguous memory
SoA
hot/cold split
arena
chunk processing
```

### Why the default struct layout is unspecified (field reordering)

A normal struct's field order is the compiler's business, not the source's: fields are laid out in
descending alignment so padding disappears (`{ a: i8, b: i64, c: i8 }` is 16 bytes, not the 24 that
declaration order would waste). This is the cache-density lever applied to the *element* level — a
tighter struct means more elements per cache line, fewer bytes streamed, better use of every load —
and it is exactly the reasoning behind `soa<T>` and the arena, one layer down. It costs nothing: safe
Align has no field-address-taking, so the physical order is semantically unobservable (access is by
name), and there is a well-worn precedent — Rust reorders struct fields by default for the same
reason. The one place a fixed byte layout matters — crossing to C, `raw` memory, JSON's byte
contract, by-value register passing — already has its marker, `layout(C)`, which pins declaration
order. So the default optimizes for the machine, and the escape hatch is explicit and visible where a
human or an ABI actually needs the bytes nailed down: hardware-friendly by default, "nothing hidden"
where it counts.

---

## Memory model v2: one region lattice, explicit copies

(Design: `impl/08-memory-model-v2.md`. Decided as a whole before M6.)

Two principles drove the load-bearing choices.

**One region lattice, not three point solutions.** Escape safety started as three unrelated
mechanisms (arena depth for `box`/`str`, a "local-backed" flag for slices, a region-0
restriction for struct `str` fields). They are unified into a single total order
`Static ⊐ Frame ⊐ Arena(1) ⊐ … ⊐ Arena(d)` with one rule — a value may only be stored or
returned where it outlives the destination. Regions stay **inferred** (no lifetime syntax,
ever); they are an analysis result, not a surface type. Restriction-as-information: one
lattice keeps the checker simple and preserves the optimizer's no-alias / contiguous /
arena-lifetime facts.

**Escape joins and cleanup joins are different facts.** A region join deliberately keeps the
shortest lifetime any path may produce; using that conservative result to choose `free` would lose
which path actually allocated the value. Every resource-owning slot therefore has a path-local
individual-vs-arena bit. Value-carrying `block` / `if` / `match` / `else` / `?` lower that bit beside
the value and select both on the same CFG edge. This keeps the one region lattice conservative for
safety while making cleanup exact, without visible lifetime or ownership-mode syntax.

**A Move call argument crosses an ownership boundary.** The callee has no caller-arena provenance,
so only a free-standing owned value can be transferred by value and dropped there. An arena-owned
value stays in its visible arena; code that only needs access passes a slice/view. Rejecting that
transfer keeps allocation and cleanup visible and avoids a hidden per-member ownership ABI.
Argument evaluation still belongs to the caller until the call is emitted, which requires
component-wise cleanup provenance for temporary aggregates when a later argument exits early.
The rule also covers synthesized calls. `Result.map_err` checks its Move error payload, while fused
pipeline functions reject Move source and result elements until MIR has explicit element
move-out/null and per-iteration cleanup. `map_err` carries the selected branch's runtime ownership
bit through its rebuilt result, retains a fresh receiver during mapper evaluation, and joins mapper
capture provenance into the result region. A materializing `scan` also rejects a Move accumulator
because it retains every intermediate value; `reduce` rejects one until its per-iteration transfer
and scanner error cleanup are explicit. Value-carrying scopes preserve the same exact transfer: a
Move value leaving a `task_group` forwards its tail local's runtime cleanup bit and clears the inner
source.

**One aggregate, one allocation mode.** A bound aggregate has one path-local cleanup bit, so all of
its owned members are free-standing or all are arena-owned. A mixed aggregate is rejected instead
of leaking heap members or individually freeing arena storage. Borrowed members remain governed by
the region lattice and do not affect this allocation choice. Owned field and element assignments
must preserve the aggregate's mode. A one-owner aggregate can carry a path-dependent runtime mode,
but mutation is rejected when the joined mode is not definite. Move-struct construction retains
completed field owners until the complete value exists, including direct `let` initialization.

**Explicit `.clone()` over a hidden copy-on-escape.** A zero-copy decoded view that needs to
outlive its input is cloned *explicitly*; the compiler never inserts the copy silently. The
cache-friendly fast path — borrow the input bytes, process, discard — is identical either
way; the difference is only the rare escape, where a copy is physically unavoidable. Making
it explicit honors **Nothing hidden** (allocation is visible) and **Predictable performance**
(a small edit that starts escaping a value does not silently jump its cost class). This is the
hardware-aligned choice: predictable allocation beats convenience, and an in-arena clone is a
bump allocation, not a malloc cliff. (Convenience-first auto-copy was rejected for the same
reason exceptions and GC were — it hides cost.)
JSON follows the same boundary for escaped text: a clean token remains an input view, while a
selected escaped token is decoded once into the caller's visible arena. The result therefore
retains both input and arena provenance; a selected escape without an enclosing arena is an
explicit decode error. Ignored tokens are still validated without a hidden proportional buffer.

**Native state belongs behind one package-defined resource boundary.** A database connection,
compiled regular expression, socket, process, and compression context are the same language
problem: a native owner is Move, its safe operations borrow it, and its destructor must run
exactly once. Encoding each one as a new compiler-known type makes `std` privileged forever and
prevents ordinary `pkg` code from providing equally safe wrappers; exposing `raw` plus `close`
instead makes the safety invariant a caller convention. The common answer is a
package-defined opaque `resource` whose representation is accessible only to the declaring
module's unsafe descendant subtree. Its raw-only source hook is `pub` only inside the package's
`internal` boundary; the resource producer supplies a hidden linkable Drop thunk, so consumers
neither import the hook nor lose separate-compilation cleanup. The hook module need not import the
declaring root, so a driver submodule can construct the public root resource without a cycle or a
public raw constructor. `resource.borrow` is public and safe wherever the opaque type is visible
because it reveals only owner-tied provenance; construction/extraction/transfer of raw
representation remains subtree-privileged and unsafe. Shared `borrow` preserves Move ownership;
invalidating `borrow mut` also updates a writable Copy state aggregate in place. Parameter modes
remain part of function values,
and their inferred return provenance includes target-relative capture roots, so indirect calls
cannot erase the ownership ABI or lifetime roots. A recursively Move return also forwards its
path-selected cleanup bit; lifetime summaries cannot reconstruct ownership after a branch.
Inferred owner generations prevent a returned view from surviving
replacement, mutation, or Drop. Exclusivity is checked across every call argument, not only
parameters spelled `borrow`: a by-value Copy view/resource reference or view-bearing aggregate
rooted in the generation invalidated by a peer `borrow mut` is rejected before the callee can
receive a dangling view. This structural rule generalizes the existing Move/Drop and
borrow-liveness machinery without adding lifetime syntax, traits, reference types, or a second
ownership model. Replacement through `borrow mut` uses the ordinary old-value Drop plan before
storing a new value, while an unchanged pointee remains caller-owned. Raw ownership transfer is
limited to a standalone resource root, avoiding hidden per-field cleanup state.

Shared borrow also accepts stable Copy storage. Copy does not need borrow to preserve ownership,
but it needs the same explicit pointer-to-caller-storage mode when a large structural value or a
producer-generated typed callback must avoid a hidden by-value copy. This is one general ABI rule,
not a package exemption, and it still rejects temporaries.

**Borrowed sum matching is a projection, not a copy.** The same distinction matters when a library
must inspect an owned `Option`, `Result`, or user sum without taking it from its caller. A match over
a stable place whose exact root/path pair is directly shared- or exclusively-borrowed reads the tag
and active payload in place; a descendant field fact never promotes an owning parent. The new path
admits Copy scalars/views, `string`, ordinary dynamic scalar/AoS-record arrays, and finite acyclic
structs and tagged values built recursively from those forms; an array element obeys the same
closed grammar. Fixed and specialized arrays, other collections, resources, opaque handles, and
other unsupported Move shapes remain on the existing borrowed-place diagnostic. The arm binding is
a read-only projection with the original static payload type, source generation, and no independent cleanup bit. Copy fields and
borrowed text leaves remain cheap reads, while an owned text leaf can use the existing explicit
`.clone()` operation. Aggregate and nested-sum clone operations are outside this capability. A
borrowed arm binding is not a stable place for a nested borrowed `match`; that use is rejected
without an owning fallback. Returning or retaining the whole non-Copy/Move payload is still an ownership error, while existing
Copy/view matching keeps its current result behavior. Keeping this as a
compiler projection over the existing flattened tagged layout preserves **Nothing hidden**, avoids
a shallow Move aggregate copy, and leaves owning-place match, `else`, and `?` semantics unchanged.
A package-specific wrapper or alternate error/result API would create a second ownership model, so
the language capability is the correct boundary.

**A Copy view read through a borrowed array projection keeps the projection roots.** Widening the
payload grammar to `array<str>` and AoS arrays of Copy records is unsafe if ordinary `Index` or
field projection silently treats the projected binding as static. Direct, field, and projected
bases therefore feed the same source generation and contained input/arena roots into return and
`borrow mut` retention summaries for every admitted region-bearing Copy leaf, including direct or
nested `str` and `slice<T>`. The implementation follows the canonical Copy/borrow classifiers, not
a `str` special case. A terminating index produces no bounds action or result. This is the existing
inferred-region model applied to a new reachable path, not a reference type or array special case.

**An owned string array reads through its existing view type.** `string` already has exactly one
non-owning form, `str`, and both share the two-word text layout. Ordinary `texts[i]` therefore
returns `str`: the array remains sole owner, existing root/region analysis ties the view to that
generation, and `.clone()` is still the visible owned-copy operation. This closes the real
tokenizer consumer without inventing a general reference type or hidden aggregate binding.

**A borrowed Move record array element is a call place, not a value.** Decoded artifact graphs store Move
records in ordinary AoS dynamic arrays. Loading `rows[i]` by value would either copy one owner or
require partial element transfer and cleanup, neither of which a read-only verifier intends. The
general completion is narrower and explicit: `inspect(rows[i])` is accepted when the selected
direct, imported, or function-value target declares that parameter `borrow`, the base is a stable
local/field/projection place, and its root is reserved from once-only index evaluation through all
later arguments and the call action. A same-root move, Drop, replacement, transfer, or mutable
borrow during that interval rejects; unrelated mutation remains valid. After index fallthrough, MIR
emits the bounds-failure branch at the indexed argument position, carries only a guarded descriptor
through later argument evaluation, and revalidates the root before LLVM forms and passes the
existing address at the call. A terminating index forms no guard, descriptor, later argument,
pointer, or call; a terminating later argument forms no pointer or call. No header/element owner,
cleanup bit, or allocation is manufactured. Returned views and views retained through existing `borrow mut`
summaries retain the array generation and contained region roots. Keeping by-value and `borrow mut`
element forms rejected preserves visible ownership and avoids a second partial-element state model.

**Structured owned errors complete the existing tagged-value model.** A native library error
needs owned message/detail fields because the foreign buffer dies at the call boundary, while a
compound operation may return a Move output through the same `Result`. Replacing that with numeric
codes, empty-string sentinels, or an opaque boxed error would create a second weaker error model.
The proper completion is recursive tagged Move payloads: `Option`, `Result`, and user sums derive
the same Drop plan as structs, drop only the active payload, and move/null it through owning-place
`match`, `else`, and `?`; an admitted borrowed-place match reads the payload without clearing its
source. The success path allocates nothing for an unused Move error. This is a general
language completeness fix with the database as consumer, not database error magic.

**Minimal generics must compose through ordinary package signatures.** Rejecting `array<R>` or a
top-level `query<P,R>` application inside a generic function leaves a package unable to implement
its own typed API and invites compiler-known DB entry points. L7 therefore permits nested symbolic
applications of the existing generic/container types and adds one closed structural
`RegionPlain` bound. Full substitution still precedes ownership/escape/MIR; there are no runtime
dictionaries, user traits, reflection, or newly legal concrete element categories.

**`SoaPlain` is a narrow layout proof, not a second generic paradigm.** D13 needs an ordinary
package function to return `soa<R>` while its public template is still abstract. Reusing
`RegionPlain` would be unsoundly broad because nullable and byte-view Rows are valid region values
but not valid concrete SoA element shapes. A separate closed `SoaPlain` bound states exactly the
existing nonempty primitive/`str` struct rule and grants only symbolic SoA formation; the template
interface carries that symbol for separate compilation, then concrete substitution precedes emitted
HIR/MIR. The batch remains the one Move owner and `soa<R>` remains the same borrowed view rooted in
that resource generation. This adds neither an owned SoA value nor a DB-specific compiler API.

**PostgreSQL delivery mode is an execution cost choice, not a Query identity.** A static Query's
SQL, Params/Row contract, binder, decoder, batch plan, and semantic fingerprint do not change when
one execution selects `SingleRow` or `PortalBatch(n)`. The public `Delivery` addition and the
rows/statement ABI implementation still change their owning interface/implementation hashes once,
so affected per-unit dependency keys and object caches invalidate when those changes land; runtime
mode selection creates no further cache identity. The choice therefore stays in the explicit
driver-qualified execution-option slice. Both direct and prepared execution enter one libpq result
state machine, retain Params until protocol completion, and expose partial-server-failure timing
instead of pretending bounded delivery is atomic. A direct `one_native` multiplicity remains
pending until clean protocol completion: normal drain preserves the SQL effects of DML `RETURNING`,
while a late server failure or explicit timeout wins. Absence of Delivery never enters this live
state machine and retains the shipped caller-synchronous BufferedFull timing, including its existing
nonblocking deadline completion. Prepared parity is a separate
formation rail because its statement state must retain the producer parameter-name resolver.
Binary wire formats remain a separate rail because they change bind/decode representation rather
than result delivery lifetime.

**A caller-supplied arena is monotonic across a late error.** `one_native` must clone a validated
first Row before mutating the rows generation to probe multiplicity. That copy is visible through
the required `out` argument and happens exactly once. If Cardinality 2 or a later protocol,
deadline, COPY, or cleanup error wins, no Row is returned, but those exact first-Row clone bytes stay
allocated until the caller's arena scope ends. The package neither hides a scratch arena nor
pretends it can rewind caller storage; zero rows and an invalid first Row allocate nothing in `out`.

**A row error does not silently choose transaction effects.** A streamed validation, decode, or
batch-storage error is already the primary caller-visible failure, but libpq may still own an
effectful `RETURNING` protocol. Align destroys unpublished values and drains without further decode
under the original absolute deadline, preserving normal completion effects when time remains. Only
deadline expiry cancels; the first row/storage error remains primary, while the connection or
transaction state exposes whether completion or cancellation won. Rows therefore retain both the
absolute deadline and the original duration needed by recovery.

**A deferred or unknown native subprotocol fails closed at every result consumer.** A COPY result is
not an ordinary invalid rows result: libpq cannot reach the terminal result until the COPY exchange
itself is consumed or terminated. Shipped synchronous and timeout executors can already observe it;
the rule is therefore a pre-stream PostgreSQL result-status closure, not only a streamed-rows arm.
Pipeline sync/aborted results likewise leave connection-global pipeline mode until an explicit
pipeline-exit operation, which this rail does not own. Every package-owned PGresult consumer clears
COPY, pipeline, or an unknown numeric status once, immediately poisons/closes, preserves an earlier
owned error or silent Drop, and then releases package owners. No later result drain, COPY operation,
pipeline exit, cancel, transaction probe, or blocking restoration runs on that connection.
Supporting libpq 17 or newer does not mean assuming a future status is drainable.
The separate Rust prepare and migration executors are part of the same consumer audit. One private
Rust classifier exhaustively identifies the complete libpq 17 numeric status set before a tool
reads result rows or issues follow-up SQL; this pure classification loads no new symbol and does not
raise the current client floor. A null result, COPY status, partial single/chunk row result, pipeline status,
or unknown numeric status copies the available diagnostic, clears the current result when present,
immediately finishes and nulls the connection owner, and permits no rollback, deallocation, row
access, or later libpq call. Known complete results retain their existing tool error mapping.
Because migration sends complete user SQL through synchronous `PQexec` but owns no COPY exchange,
canonical PostgreSQL migration screening also rejects a top-level first-token `COPY` before URL
access, target open, or native work. The screen classifies each complete statement once in source
order, so `COPY; BEGIN` reports COPY while `BEGIN; COPY` reports transaction control; only after
that pass can Forbidden-count validation win. Preparation uses `PQprepare`/`PQdescribePrepared` and
does not execute COPY; other tool SQL is fixed, but neither fact substitutes for fail-closed status
handling.

**Stream protocol state precedes status-to-error mapping.** The zero-row `PGRES_TUPLES_OK` terminal
changes the expected next event to null. Any later non-null result is therefore the streamed-sequence
error even when its ordinary standalone status would map to a native execution error. Its status
still chooses cleanup: ordinary known statuses clear and drain, while COPY, pipeline, and unknown
statuses clear and close immediately. This separates deterministic error precedence from the
safety action required by the native subprotocol.

**Context-backed static validation stays in the settled execution phase.** The generated
static-option validator needs an execution context, while overlap is deliberately checked only
after static validity. Direct PostgreSQL delivery therefore preserves the shipped order: validate
descriptor/options/restriction and live state, allocate the context, run generated static
validation, acquire the lease, then bind and call libpq. Moving static validation before live state
or moving the lease before it would change observable error precedence and allocation behavior.

**A deadline is checked at the last reversible pre-send point.** Enabling libpq nonblocking mode may
consume the remaining operation budget. Direct and prepared explicit delivery therefore re-read the
monotonic clock immediately afterward and before send. Expiry restores blocking mode and returns
Timeout with zero send, selector, and cancel calls; failed restoration poisons/closes. This preserves
the shipped D9 rule that effectful SQL never starts after its deadline has already expired.

**A live native protocol requires a complete consumer lease inventory.** PostgreSQL catalog and
EXPLAIN originally used synchronous full-result libpq calls, so D12 checked their live connection
but did not give them the typed-execution lease. That was harmless only while every PostgreSQL rows
constructor also completed its libpq protocol synchronously. Single-row/chunked delivery keeps the
connection protocol-busy after return, exposing the omission. The lease fix is one smaller prerequisite,
not a delivery special case: every catalog and common/native EXPLAIN call acquires the same lease,
holds it through result/context cleanup, and rejects overlap before libpq. Both subsequent direct
and prepared streamed-delivery PRs depend on that general closure. The separate shipped-result
status prerequisite follows it because that safety repair is independently useful without a public
surface, ABI, or libpq-version change.

**A named `region` is a destination capability, not an allocator abstraction.** Compound
database reads and streaming decoders need ordinary library functions to construct caller-owned
arrays and strings without falling back to hidden heap allocation. `arena out {}` exposes only
the existing arena's allocation destination as a scope-limited capability. Passing `out:
region` neither transfers the arena nor makes allocation implicit: the destination remains
visible at the call site and the result remains bounded by the same inferred region lattice.
There is no allocator trait, lifetime parameter, cross-arena sharing, or automatic copy. An
explicit `clone_in(out)` marks the unavoidable transition from a short-lived input view to
owned output.

**Static SQL is one statement-artifact mechanism, not a Query-only exception.** A row-returning
Query and a rowless command differ only in Row/result/decode data. Both use the same item identity,
source/wire SQL split, generated Params binder, retention plan, per-driver checked state, producer
implementation hash, and exported typed contract. This keeps the minimal SQLite insert vertical
from inventing a second compiler path. A portable CheckedRequired descriptor means checked on every
permitted driver; mixed SQLite/PostgreSQL evidence remains visible instead of collapsing to a
misleading boolean.
The artifact fingerprints the complete reachable structural Params/Row definitions, not only their
names. A Query additionally owns a static metadata plan and generated materialization thunk, so
runtime inspection remains possible across separate compilation without reflection or reading
source/metadata files. Checked metadata and migration/schema identities have versioned exact codecs
and independent goldens; “canonical” is never an agreement between two copies of the same encoder.

**A bounded encoder is a destination policy, not a second format.** A consumer that must reject an
oversized persisted artifact cannot safely call an unbounded encoder and discard the result. The
bounded operation therefore shares the exact typed encode plan and every scalar/escape/descriptor
writer with the ordinary operation, while its destination enforces an inclusive byte ceiling
before growth. Success bytes match by construction, the owned result makes allocation visible, and
limit failure is an ordinary `Result`; no estimator pass, dynamic JSON tree, alternate canonical
rules, or partial prefix becomes a second way.

**Database configuration is closed, scoped data.** Connection, static statement, prepare, execute,
transaction, metadata, and EXPLAIN options have distinct finite sums and separate common/native
slices. Their first-release variants, defaults, and conflicts are part of the API contract, not
driver-selected examples. A requested variant is applied or rejected before SQL send; it is never
stored in a reflection bag or ignored for portability. Each milestone lands the option type its
operation consumes, while the later cancellation milestone completes shared machinery rather than
replacing a provisional API.
Connection-global native state also has an explicit owner: SQLite v1 permits one active execution
lease per physical connection. A second Copy execution view cannot overlap timeout/statement state;
it fails before mutation or native access, and the owning stream alone restores state on
exhaustion, error, or Drop.

**Catalog output owns its destination, and exceptional migrations fail dirty.** Metadata and plans
copy flat records and strings into a caller-named region before native buffers die; multi-term
keys/indexes are repeated rows rather than nested hidden allocations. Migration SQL is atomic by
default. Every live migration command names its entry graph, catalog, driver, and matching target;
ambient configuration cannot redirect it. A visibly transaction-forbidden one-statement file
records Applying before native execution and blocks on ambiguous failure until checksum-bound
operator repair. Query nullability is similarly fail-closed: engine-reported query evidence is
retained, ambiguous evidence remains `Unknown`, and catalog `NOT NULL` alone cannot remove runtime
NULL checks after joins or expressions. Metadata detail is a finite projection matrix, not a
best-effort property bag; identifiers are validated before native access. These choices expose the
costs that cannot be wished away: result retention, live-schema identity, and non-transactional
side effects.

**An aggregate constant is a `slice<T>`, not an `array<T>` — ownership is a property of the type.**
A top-level array constant (`PRIMES := [2, 3, 5]`) could have been an owned `array<T>`, but that would
contradict the model: ownership is decided by the *type*, and a compile-time table owns nothing. It is
the exact analogue of a `str` literal — `GREETING := "hello"` is a `str` view of static bytes, not an
owned `string`; so `PRIMES` is a `slice<i64>` view of a static table, not an owned array. This falls
out of the region lattice for free: the elements are one **per-unit read-only data** table and the
constant is a `Static` `{ptr,len}` view of it, so it is shared (never copied), returnable from any
function, and never dropped — with no new mechanism. It also keeps **one way**: indexing, `.len()`,
slicing, and pipelines reach it through the existing borrowed-`slice<T>` paths, so there is no
array-constant-as-value seam and no allocation. Per-unit (not whole-program) rodata is the settled
storage: each importing unit rematerializes the constant from its exported initializer source, which
is exactly what makes cross-unit edits invalidate dependents through the interface hash for free. An
`array<T>` annotation is therefore rejected, not accepted-and-coerced — the type would be a lie about
ownership.

**A heap record builder reuses nominal type identity and compile-time Drop, not a runtime record
descriptor.** Once a consumer needed runtime-sized record arrays, three possible designs existed:
serialize records through JSON, add a self-describing structural record-builder wire, or extend the
existing typed builder. JSON would turn an in-memory ownership operation into an encoding round
trip. A second structural wire would disagree with Align's nominal record identity and duplicate
interface versioning, layout, cache invalidation, and Drop facts. The typed extension is the one-way
answer: the compiler already knows the exact record definition and recursively generated Drop plan,
so the runtime header still needs only storage state. The safe heap predicate is deliberately
narrow—Copy scalars, free-standing `string`, and nested records of the same class. Views belong to
the explicit-region builder; other owned collections wait for their own recursive cleanup proof.
That split keeps allocation visible and lets realloc move raw record bytes without inventing hidden
arenas, reflection, or callbacks.

---

## The lambda philosophy

Lambdas (`fn x { ... }`) are not a separate paradigm bolted onto the data-processing core — they
**are** how you pass behavior to `map` / `where` / `reduce` / `par_map`. A lambda and a named
function are the same thing; the lambda just spares you a top-level declaration for a one-off.

The load-bearing decision is **how capture works**. A lambda that captures an enclosing variable
does *not* allocate a hidden closure environment: it is lifted to an ordinary function whose
captured values become extra parameters, passed at the call site. So a captured pipeline lambda
fuses into the same counted loop as a named function and carries zero allocation — the capture is
just a loop-invariant argument. The compiler snapshots each stage capture once when that written
stage is formed, evaluates later terminal arguments, then snapshots terminal/reducer captures; the
fused loop reuses those SSA operands. It never delegates capture timing to an optimizer by emitting
an enclosing-local reload per iteration. A view snapshot keeps its owner dependency across the
intervening arguments, so an ownership-changing `init` is rejected rather than leaving a dangling
callback argument. This keeps lambdas inside the existing
guarantees rather than introducing a new cost class: **Nothing hidden** (no silent heap
environment), **Predictable performance** (a lambda is never secretly slower than the named
equivalent), and **Compiler-friendly** (the optimizer sees a direct call, not an indirect one
through a closure object).

**Escape decides the representation.** A lambda that *escapes* — stored in a variable, returned,
or handed to `task_group`'s `spawn` to run later — outlives the locals it captures, so it cannot
borrow them; it needs a **closure environment** holding the captured values. The compiler's escape
analysis (the same one that governs views and arenas) chooses: a non-escaping lambda (every
pipeline `map`/`where`/`reduce`/`par_map`) is inlined with captures-as-parameters — zero
allocation, SIMD/GPU-friendly; an escaping lambda gets an environment. That environment is not a
new hidden cost class: it is **owned by the enclosing region** — the `task_group {}` / `arena {}`
scope it escapes into — and freed with that region, exactly like every other region allocation.
The task-group region is reserved for spawned environments and result slots; it does not silently
turn unrelated allocations in the block into arena allocations. So
it stays inside the one region-based allocation model, and the *visible* act of escaping (and the
*visible* enclosing scope) is the allocation boundary — consistent with **Nothing hidden** (no
silent free-floating `malloc`). This is the load-bearing design point: it lets first-class function
values and `task_group` exist *without* eroding the inlined, offload-ready pipeline path, and the
two paths are distinguished by escape, not by two different lambda syntaxes. (The allocation model
for a closure that escapes *every* region — e.g. one returned to an unbounded caller — is part of
the deferred first-class-closure design; the `task_group` consumer is scope-bounded and clean.)

The **Side Effect Rule** completes the picture: a `par_map` lambda must be Pure (it may read
captured values but not mutate external state), which is what makes data-parallel execution safe
without locks. A `task_group` task, by contrast, *may* be impure — it performs I/O — and its
safety comes from capture being by value (no shared mutable state) rather than from purity.
Successful-Wait evidence follows the Result value only through transparent local/control
operations. It is not inferred through a call, return, capture, import, or aggregate: those
boundaries would require a second hidden effect/provenance summary. Each fallible Wait also has a
generation-local identity. All earlier Waits for one drained task generation must resolve Ok before
that generation is complete; a later empty Wait cannot erase an unresolved or failed result.
Failure invalidates the covered generation. Every Spawn advances the current task generation and
stales prior Wait proofs. With an unresolved Wait it also invalidates old Tasks; otherwise a later
successful Wait can cover old and new tasks.
Once completion exists, a later no-task Wait cannot make an initialized result slot unsafe, so its
unhandled Result does not revoke the earlier fact. Loops use stable syntax-site proof tokens and a
header fixed point before joining breaks; this both converges and prevents traversal order from
erasing an earlier iteration's unresolved or failed Wait.
This lets stored Result handling remain ordinary source code while keeping `get()` safety visible
and mechanically local.
The Task handle carries a separate compiler-only origin for the group and generation that created it. Because the
handle is Move, transparent local/control moves preserve that origin; opaque boundaries do not.
Nested groups push a new identity without hiding outer facts. Therefore an inner Wait cannot
authorize an outer Task, but an outer Wait Result handled inside the inner group updates the outer
fact. `get()` consults that origin rather than the current nesting position. Current primitive
results are copied out without consuming the handle. Group exit filters proofs by group identity:
an inner Wait Result carried out as the block value loses its inner proof and cannot authorize an
outer Task, while proofs naming still-active outer groups remain. Owned task results are a later
extension.
Ordinary sequential pipeline callables may also be Impure: deterministic input/stage order and
`where` guards preserve their observable behavior. Their inferred effect is optimization evidence,
not a rejection rule. Purity is inferred, never annotated, and is still weaker than
non-trapping/total execution.

---

## The loop philosophy

Align has exactly one loop construct, and it is deliberately narrow: `loop { ... break value }`.
**The pipeline owns the data path; `loop` owns the control path.** Traversing a collection is
`map` / `where` / `reduce` — that is not a style preference but what lets the compiler see
*which* data-parallel operation the code is (SIMD, fusion, offload). So `for x in xs` does not
exist: it would compete with the pipeline for the same territory, split the culture, and hide the
map/filter/reduce structure the compiler needs. What the pipeline cannot express is iteration
whose trip count is decided by the iteration itself — read until EOF, retry with backoff, drive a
protocol, pump a state machine to convergence. That category needs exactly one tool, and `loop`
is it.

**Why not recursion?** "No loops, use tail recursion" was considered and rejected — not on taste,
but because guaranteed tail-call optimization structurally conflicts with four load-bearing
decisions:

1. **Drops and regions kill tail position.** Move types drop at scope end and arenas free at
   scope end, so any frame holding one cannot tail-call — the cleanup runs *after* the call. This
   is the same reason Rust rejected implicit TCO. I/O pump loops are exactly the frames that hold
   Move values (`reader`, `buffer`), so TCO fails precisely where loops are most needed.
2. **`?` kills tail position.** The one error model makes sequential loops fallible, and
   `r.read(buf)?` followed by anything is not a tail call. An error model and a recursion-based
   loop model fight each other.
3. **Nothing hidden.** Whether a call is in tail position is invisible in source; a one-line
   refactor silently turns O(1) stack into O(n) and surfaces as a runtime stack overflow. Align
   does not build hidden failure modes into its basic iteration idiom.
4. **Compiler- and AI-hostile.** A loop back-edge is the friendliest CFG LLVM can get;
   reconstructing loops from recursion is the fragile inverse. And accumulator-threaded tail
   recursion is a known bug source for both humans and models, while `loop` + `mut` state is not.
   Recursion-as-iteration loses on all four alignment axes at once.

Recursion itself remains legal — a parser or a tree walk is genuinely recursive — but it is for
recursive *problems*, never a substitute for iteration, and Align guarantees no TCO.

**Why not `while`?** `while cond` is a second loop form that cannot yield a value; `loop` with
`break value` subsumes it and stays an expression like `if` / `match` / `arena`. **Why no
`continue` or labels?** Minimality with an exit: skip-to-next is an `if` around the rest of the
body, and a nested loop needing a two-level exit is a function waiting to be extracted. Both can
be revisited on real-code evidence; starting without them is the smaller regret.

The boundary is enforced, not hoped for: walking an array by index inside a `loop` draws a
"write it as a pipeline" lint — the same pattern as the unnecessary-heap and unhandled-`Result`
lints. `loop` also finally gives the deferred frequency-dependent lints
(allocation-in-loop, branch-in-hot-loop, `prefer-pipeline-over-vecN`) their firing surface.

---

## The SIMD philosophy

Align does not try to make developers write SIMD.

Align makes ordinary code naturally SIMD-friendly.

Examples:

```text
map
reduce
scan
where
mask
```

These should lower naturally to vectorized code. The point is *structural*: contiguous arrays mean a
pipeline walks memory sequentially (no random jumps), and safe primitive conditional reductions can
lower to a mask + `select` — so the predictable shape, not hand-tuning, is what keeps hot loops
vectorizable. A callable after `where` **is control-flow guarded** unless it is separately proven safe
on an inactive lane; safe field operations and builtin reducers retain the mask/identity-select
shape. Pure alone is insufficient because a Pure function may trap (audit: `impl/12` §3.1).

**Branchless is for vectorization, not because branches are slow (recorded 2026-07-04, external
design-note review adoption).** Modern branch predictors (TAGE-class) make well-predicted branches
near-free, and scalar CMOV chains create data dependencies that can be *slower* than branching.
Align's masked `where` form exists because select/predication enables SIMD for operations that are safe
on inactive lanes, not as a scalar-branch-avoidance dogma — don't cargo-cult branchless into scalar
std code or speculate trapping callables. The one
exception where branchless is mandatory is `std.crypto`'s constant-time requirement (see
`open-questions.md`).

SIMD lives in two layers, and the split is deliberate. **`vecN<T>` / `maskN<T>` are an escape hatch**
for hand-tuned fixed-width register kernels (a dot product, an FMA loop, a FIR filter) — they are
*always* a fixed size, so they can be a `Copy` register value with a constant `sizeof` and constant
lane indices. **The pipeline (`map` / `where` / `reduce`) is the main road**, and it never names a
width — which is exactly why a future scalable ISA (SVE/RVV) lives here invisibly: the same source
lowers to a fixed-width loop on NEON/AVX or scalable predicated codegen on SVE/RVV, chosen in the
backend. That a width is *not* in the source is consistent with "nothing hidden": a vector length,
like the AVX-vs-NEON choice itself, is a hardware detail, not a semantic effect — so hiding it (unlike
allocation, errors, or parallelism, which are real effects) is correct, not a leak.

For the layout itself, Align takes the **explicit `soa<T>` over automatic inference** road. The safe
core has no raw pointers or field-address-taking, so a struct array's physical layout is
semantically unobservable — the compiler *could* silently turn `array<User>` into struct-of-arrays.
We deliberately don't: a silent layout switch hides performance (against "predictably fast") and
needs an opaque heuristic. Instead the choice is one visible token — `array<User>` (rows) vs
`soa<User>` (columns) — and the compiler does the field-wise column lowering *under* that type. So
the decision is explicit and predictable; the mechanism is automatic. It is the principled,
first-class form of the "split it into parallel arrays by hand" trick every data-oriented programmer
already reaches for.

### Where the SIMD actually comes from (and why the default build is conservative)

Align targets the real deployment world — **cloud and containers, where you build once and run on an
unknown, varied fleet** (Intel/AMD/Graviton, feature-masked or live-migrated hosts). A binary baked
for the build host's CPU (`native`), or for a high fixed baseline like AVX2, would crash (`SIGILL`)
on some hosts. So the philosophy splits SIMD by layer:

- **Generated code** is fixed at build time, so it targets a **safe, portable per-arch baseline by
  default** (`x86-64-v2` / `armv8-a`). `native` and higher baselines are **opt-in** — one good
  default, visible opt-in, never hidden. The right frame is a *portable per-arch vectorization
  strategy*, not one fixed width: on fixed-width ISAs (AVX/NEON) the baseline is 128-bit + a scalar
  remainder, but on a scalable ISA (SVE/RVV) it is scalable *predicated* codegen — one binary that
  adapts its vector length at run time, not a 128-bit cap. MIR stays width-agnostic precisely so the
  backend can make that per-arch choice (`impl/04 §4`, `impl/05 §5`).
- **Wide SIMD on a varied fleet comes from the library**, via *runtime* CPU-feature dispatch (the
  binary detects AVX2/NEON at run time and falls back safely). This is why the library leans on
  portable dispatching crates rather than hand-written intrinsics: it adapts per-host *and* stays
  multi-arch (x86-64 + aarch64) from one source. The heavy SIMD work (JSON, string scan, bulk copy)
  lives here precisely because this is the only layer that can adapt at run time.

The lesson: for an AOT language aimed at the cloud, "automatic SIMD" is not a single fixed target —
it is a conservative portable floor in the codegen plus runtime-adaptive SIMD in the library.

---

## The GPU philosophy

Align is not a GPU language.

Align only seeks to keep future GPU execution possible.

It prefers data-oriented operations because they map naturally to the following.

```text
CPU
SIMD
GPU
```

---

## The string philosophy

Strings are not magic objects.

Strings are data.

The goal:

```text
scan once
zero copy
builder based output
string pools
```

Repeated scanning should be avoided.

**Owned `string` stays `{ ptr, len }` — no Small-String Optimization.** SSO (inline
`{ ptr, len, cap }` with a tag bit) was considered and rejected: it adds a branch to every
access and breaks FFI pointer stability, while Align's arena model already avoids the
small-`malloc` churn SSO targets — so it trades "predictable performance" + "nothing hidden"
for a marginal win. (Settled in `open-questions.md`.)

**Output writes into a `builder` sink, not a returned string.** The library convention is
`write_json(out: mut builder, …)` over `to_json() -> string`: serialization/formatting append
into a caller-provided buffer (often arena-backed), so complex output costs zero heap
allocations. Paired with read-oriented `std` APIs returning views (`str`/`slice`/`bytes`)
rather than owned copies, this makes zero-allocation pipelines the default. (A std design
rule — `open-questions.md` Future "Library architecture principle".)

---

## The JSON philosophy

JSON is the de facto assembly language of modern APIs.

Align treats JSON as a first-class concern.

The goal:

```text
SIMD scanning
typed decode
zero-copy strings
field tables
arena allocation
```

"Typed decode" is written `u: User := json.decode(d)?`, not `json.decode<User>(d)`.
A decode's target type is return-position-only — it cannot come from the arguments —
so it is recovered from the expected type propagated from context (the binding
annotation, flowing back through `?`). Align deliberately has **no
expression-position type-argument syntax** (no turbofish): the binding annotation is
the single place a type is written ("one way"), and refusing `f<T>(x)` removes the
`<`-vs-comparison parse ambiguity at expression position outright — the same
ambiguity that pushed Go to `f[T](x)` and Rust to `::<>`. When context supplies no
type, that is a hard error asking for an annotation, never a silently-defaulted type.
(Settled 2026-06-22; see `open-questions.md`.)

Decode is **strict and exactly-once**: every declared field must appear once; a missing *or duplicated*
declared field is an `Err`, never a serde-style silent last-wins (undeclared keys are the only thing
skipped). The reason is the one error model: decoding into a fixed struct, a duplicate key is a data
error, and surfacing it as a value beats a silent partial decode — "nothing hidden". This is the
intended contract; the current decoder's speculative fast-path has one known narrow deviation, tracked
as a pre-freeze gap in `open-questions.md`. (Settled 2026-06-29.)

**Completeness has three tiers, not one type (settled 2026-07-18).** "Complete JSON" is typed
decode/encode over the full type matrix (schema known), a lazy `json.doc` view (schema unknown),
and a `json.scan` streaming source (larger than memory) — each the ideal form for its regime, none
competing for the same job ("one way" per job). Three deliberate rejections define the shape:

- **No serde-style value tree.** A recursive `JsonValue` heap tree is per-node allocation and
  pointer-chasing — the exact cost model Align exists to avoid ("nothing hidden", data-oriented) —
  and would drag recursive enums and a map type into the language. The simdjson-style on-demand
  view gets the same capability from the machinery Align already has: the SIMD structural index,
  arenas, and region-tied borrowed views. Objects-as-data (dynamic keys) are covered by ordered
  member iteration on the view, so **no map type enters the language** for JSON's sake.
- **Unions discriminate by shape, restricted until deterministic.** A JSON `oneOf` maps to a sum
  type whose variant payloads occupy pairwise-distinct shape classes (Str/Number/Bool/Object/
  Array), checked at compile time — so decode is a single-byte O(1) dispatch, no backtracking, no
  ordering sensitivity ("compiler-friendly by restriction"). `null` is not a class; absence is
  `Option`'s job everywhere (one absence representation).
- **Streaming rows are Copy-only.** `json.scan` reuses one row slot without a per-row arena or
  `Drop`, so semantic checking admits only rows whose complete reachable definition graph is
  recursively Copy under the canonical `DropPlan`. This is a scanner-only restriction; ordinary
  JSON decode and the declaration's other uses retain their own explicit ownership contracts.
- **Owned decode is ownership-directed, not escape-directed.** A declared record whose closed graph
  contains owned text selects a free-standing materializer at `json.decode`; a borrowed graph
  keeps the zero-copy input/arena path, and a mixed graph rejects. The target's `string` types expose
  the allocation choice before execution, so the result can outlive both input and an enclosing
  arena without a hidden copy-on-escape. `encode` and `encode_bounded` consume the same graph and
  ordered plan. The shipped flat graph is deliberately narrow. Its accepted recursive successor is
  still a closed, acyclic, view-free grammar with a 128-level bound; it replaces the flat descriptor
  and runtime route rather than creating a second JSON ownership model. This lets C6 persist nested
  records/options/arrays while keeping every allocation mode visible in the declared types.
- **A target-local JSON descriptor is never ambiently authenticated.** Per-unit serialization wraps
  it in the canonical target triple, object format, and complete relevant ABI tuple and validates
  that envelope before reading an offset. The frontend target cache key remains redundant
  partitioning, not evidence. Public non-generic records participate in interface identity;
  private records and concrete consumer monomorphs participate only in implementation identity.
- **Integer display follows declared signedness through the runtime call.** A template `IntHole`
  retains its exact `Ty::Int`; codegen chooses the signed or unsigned decimal builder ABI after
  sign- or zero-extension. This keeps full-range `u64` JSON canonical without a JSON-only formatter
  or signed reinterpretation.
- **Request 6's scanner generic boundary is concrete-row-only.** Concrete generic monomorphs such
  as `Wrap<i64>` remain eligible after row resolution, and ordinary generic calls use expected-return
  propagation owned by `align_sema::Checker::check_generic_call`; numeric `IntVar`/`FloatVar` retain
  deterministic `i64`/`f64` defaults. An unresolved `Wrap<T>` / `json.scanner<Wrap<T>>` type argument
  inside a generic function keeps the exact resolver diagnostic `instantiating a generic struct
  with a type parameter ('Row<…>' inside a generic function) is not supported yet`; that capability
  is a separate compiler prerequisite, not an implicit extension of the scanner surface.
- **The catalog carries no dangling entries.** `validate<T>` (decode-and-discard is validation),
  `token` (no consumer; doc + scan cover it), and `field_table<T>` (compiler-internal) were
  deleted rather than left "spec'd but unimplemented" — a catalog entry is a promise, and unkept
  promises are exactly the "this works, that doesn't" fragmentation the completeness push removes.

---

## The entry-point philosophy

The source entry has one C-compatible boundary, not an arbitrary Align function exported under
the special name `main`. No-argument `main` is Unit, exact i32, or
`Result<(), Error>`; the argv form is the Result form. Exact i32 already is the C ABI. Unit and
Result go through one generated i32 wrapper, so exit behavior is defined without teaching the
language about additional platform entry ABIs. Rejecting every other return type is preferable to
silently exposing a bool, float, or aggregate under C's `main` symbol.

## The safety stance

Align is intentionally positioned between the following.

`unsafe` authorizes an invocation site, not a value that can carry invisible permission elsewhere.
An extern may therefore be called directly or used by an immediate non-escaping callback consumer
inside `unsafe`, but it cannot become a first-class function value until the language has an
explicit unsafe-callable type. This keeps foreign execution lexically visible.

**Generated native callbacks are producer-selected, not a second FFI surface.** Some native
libraries call application behavior synchronously, but exposing raw callback pointers or a general
export annotation would let ABI, lifetime, effect, and unwind obligations drift into application
convention. A trusted compile-time package producer may instead select one exact noncapturing
target and cause the compiler to emit a nominal descriptor and package-specific C trampoline. The
package contract owns the complete ABI and malformed-input policy; source can neither construct nor
reinterpret the descriptor. This reuses ordinary effect/provenance/cleanup facts while keeping
closure allocation, unsafe permission, native pointers, and callback-frame views out of the public
language. Invocation-scoped callback views are non-Send: direct, imported, and concrete indirect
helper facts preserve that provenance to every `spawn`/`par_map` sink, while an unresolved target
fails closed. Persisted native semantics remain a separate package concern: for example, SQLite
collations still need versioned ordering identity and migration/`REINDEX`, even if their callback
body is statically proved Pure.

Raw memory stores flat values, including `raw` pointers themselves. This is the honest representation
for a package-owned native handle slot: the address remains an address, its load/store stays visibly
`unsafe`, and no database or other FFI wrapper needs an integer cast or a compiler/runtime-owned
handle registry. Safe public resource types still hide that representation and own exactly-once
cleanup.

Native ABIs also need an actual null pointer. `raw.null()` forms it explicitly inside `unsafe`; it
does not introduce a second optional-value model into ordinary code. `Option<T>` remains the only
language-level absence, while raw ABI sentinels remain visible and grep-able at the boundary.

A declared non-Unit return is also a control-flow obligation. The compiler accepts a path only when
it produces the declared value or provably does not continue; it never repairs reachable
fallthrough with an ABI-dependent implicit value.

```text
Rust
Zig
```

The position:

```text
safer than Zig
simpler than Rust
```

Normal code should be safe.

Dangerous code should be isolated.

---

## The performance philosophy

Stated by the owner (2026-07-11, the optimization consultation): the ideal is that

> normally-written Align compiles to what an expert would have hand-tuned in Rust.

Three consequences shape every performance decision:

- **Constraints buy information.** The reason a pipeline can fuse, vectorize, and skip bounds
  checks is that `map/where/sum` LEAVES the intent standing — a hand-written loop destroys it and
  forces rediscovery. Every "one way" restriction is a promise kept to the optimizer (and, dually,
  to the adversarial reviewer: the same legibility that enables optimization makes verification
  converge).
- **Data movement before instruction execution.** Align optimizes what is read, in what order,
  from how few cache lines — before it optimizes how it is computed. The measured wins bear this
  out: the soa column scan beats Rust 8–10× as a *cache* win, not a SIMD win. Contiguous by
  default; indirection visible in the type; only needed fields loaded; hot and cold data apart;
  memory traffic weighed alongside asymptotic complexity.
- **The benchmark target is a triple.** Align-normal vs Rust-normal vs Rust-expert. Winning every
  case against Rust-expert is not the bar (both end in the same LLVM); the bar is
  **Align-normal ≈ Rust-expert at a fraction of the effort** — with the receipts (benches, and
  eventually the per-build optimization report) checked in.

Speed alone is not the moat — expert Rust catches up. The moat is speed that is **explainable**
(the compiler says why a loop did or didn't vectorize), **verifiable** (shape tests pin the fast
form), and **non-regressable** (CI gates on allocation/fusion counts). Fast, and provably so.

---

## The AI philosophy

AI-friendliness is not a feature.

It is a design constraint.

What it avoids:

```text
complex lifetime systems
macro systems
multiple paradigms
excessive abstraction
```

What it prefers:

```text
predictability
clarity
consistency
```

---

## The resource-oriented north star

A sharpening of the AI philosophy, not a new direction (recorded 2026-06-28 from the `work/`
research sweep; benchmarks in `open-questions.md`).

AI can now write a lot of code. That shifts the language's job. The question is no longer only
"can a skilled human express this?" but:

```text
when AI writes ordinary code, does it land on a fast, predictable, resource-aware shape by default?
when the user's CPU / GPU / RAM / VRAM / SSD is limited, does the language help use what they have?
when code is slow, can the toolchain explain why before days are wasted?
```

This is the Rust contrast, stated as a different bet:

```text
Rust:  a skilled human can write very fast, very safe systems code.
Align: AI-written ordinary code should fall into fast, safe-enough, resource-aware rails.
```

Rust rewards expertise. Align reduces the expertise required to *avoid the obvious resource mistake*.
The win is not "a stronger optimizer than Rust" — flat scalar loops hit parity, same LLVM. The win
is that **the slow shape is hard to write**: SoA over `Vec<Struct>`, fused pipelines over intermediate
arrays, arena over per-object alloc, zero-copy views over `read_file` copies, sink-first buffered I/O
over flush-per-write, dictionary ids over hot-loop string hashing. The benchmarks bear this out (SoA
column scan ~11×, mmap view ~12×, buffered stdout ~355×, dictionary-id reuse ~21× — all measured).

Consequences already in the design:

```text
- fast data layout is the default rail (soa<T>, fusion, columnar group_by)
- cost is visible (no hidden alloc / copy / async / thread)
- memory layout is a first-class, explicit choice (type- and scope-driven, never whole-program inferred)
- I/O is sink-first, buffered, region-scoped (mmap views, writev, io.copy)
- native operations that fill a `buffer` take only a bare `mut` local: the mutation and stable
  address are visible, while temporaries and immutable aliases fail before any syscall or entropy
  operation is formed
- the std library encodes performance rails, not only convenience APIs
- diagnostics explain resource mistakes in plain terms (the perf-rail lints)
```

The north star, plainly: *a constrained person, on constrained hardware, should be able to ask AI to
write systems code that lands on the fast path by default.* This is not a claim that weak hardware
beats expensive hardware — it is a claim that the floor rises. Local LLM inference is the headline
long-term instance of this pressure (recorded as a Future direction in `open-questions.md`); it is a
direction, not a v1 commitment, and it must not distort the language into a GPU-only ML framework.

---

## Why `core.hash` is one dependency-free mixer over bytes

`core.hash` exposes a single canonical non-crypto hash (`wyhash`) over a byte view, not a generic
`Hash` trait over arbitrary values. Three forces converge on that shape:

- **One way.** A public `hash64` forces a decision the `group_by` perf work kept deferring — *which*
  hash is Align's non-crypto hash (FxHash vs `ahash` vs hand-rolled AES). Picking one canonical mixer
  and pointing every internal path (group_by, dict-encode, the JSON PHF) at it is the convergent
  answer; two "non-crypto hashes" would be the thing to avoid. Realized 2026-07-03: all three now
  route through the one `align_hash::wyhash` (see `open-questions.md`), replacing FxHash / FNV-1a.
- **Minimal-runtime identity over peak speed.** `ahash` (AES-NI) benched faster but adds a dependency
  and a cross-arch fallback to a runtime whose whole identity is small/zero-dep/predictable. `wyhash`
  is ~40 lines, dependency-free, strong-avalanche, and proven — the ideal fit. Speed that costs the
  identity is the wrong trade here (it can still be revisited as a perf lever, isolated).
- **No trait complexity.** Hashing arbitrary values needs a `Hash` derivation mechanism — a trait
  system Align deliberately doesn't have. Hashing a *byte view* (`str`/`slice<u8>`) needs none: the
  data-oriented core already hands you bytes. `hash128` returns a tuple, not a `u128`, for the same
  reason group_by returns columns — the small, explicit, data-shaped value, no new scalar width.

---

## Why recursive heap record builders stop at record/string array elements

The heap `array_builder` accepts Options and dynamic arrays inside its view-free record graph, but
does not introduce arrays whose elements are Options or other arrays. The named evaluator consumer
needs optional fields, arrays of owned strings, and arrays of records that themselves contain those
fields. All use existing compact type representations and the canonical recursive Drop plan.
Making direct `array<string>` a valid record field also makes that record available to the existing
Option, Result, and user-sum payload grammar. This is not a new tagged surface, but it is a newly
reachable Drop composition and therefore belongs in the same implementation proof.

An `array<Option<T>>` or `array<array<T>>` would instead need a new composite-element array type,
element move-out/indexing rules, interface and ABI representation, and generic/pipeline coverage.
Admitting that larger family merely because the words “recursive array” suggest it would violate
compiler-friendly restriction and add an unconsumed second failure domain. The smaller closed
grammar therefore preserves one typed builder and one Drop mechanism while leaving the genuinely
new array representation for a consumer that needs it.

---

## Why process capture bounds each stream and keeps bytes explicit

A child has two independent backpressure channels. One combined stdout-plus-stderr counter would
make a diagnostic budget depend on which pipe the scheduler drained first, and could let a valid
response consume the space needed to explain its failure. `max_capture_bytes(L)` therefore applies
the same explicit `L` to each stream: at most `2L` retained bytes, with exact-limit success and no
partial result after overflow. Unset remains the only unbounded state, while explicit zero means
empty-only; hidden defaults and magic sentinel values stay out of source.

Text and bytes remain separate operations for the same reason `read_file` and byte views are
separate. `run()` promises valid `str` views and rejects malformed UTF-8. `run_bytes()` promises the
same process, timeout, cap, kill, direct-child reap, and ownership behavior but exposes arbitrary
bytes through `slice<u8>`. A flag that changes an accessor's type, silent lossy conversion, or
truncation would create hidden mode or partial success. Two named terminals over one capture engine
keep the type and allocation contract visible.

Overflow uses the existing `Error.Invalid` category because the requested operation cannot produce
a complete value under its declared bound; it adds no process-specific error model. Timeout remains
distinct and is checked before each poll/read checkpoint and while waiting for the direct child after
pipe EOF. Overflow, timeout, and hard pipe/wait errors signal the owned process group when present,
kill/reap only the direct child, close the capture fds, and discard partial bytes, so failure has one
cleanup shape and never masquerades as a nonzero successful run.

---

## Why HTTP receive limits use two scopes and a reserved code

A reusable client needs a safe default, while one provider operation often knows a smaller response
budget. Keeping both `client.max_response_body_bytes` and
`request.max_response_body_bytes` makes that policy visible at the transport boundary: the request
may narrow but never widen its client. Zero has one meaning at each scope—restore the fixed default
or inherit—so there is no hidden process setting or second client implementation.

The limit must be distinguishable from malformed framing and from an HTTP status, but adding a new
builtin `Error` variant would widen every exhaustive match for one library-specific resource
condition. `Error.Code(-1)` is therefore reserved for this receive limit. Standard errno mapping
publishes non-negative raw codes and HTTP statuses remain response data, so the negative code is
stable without creating a second error type. The HTTP lowering owns its private native sentinel and
converts it before the common errno-status decoder.

The explicit path allocates fixed header and body regions from the protocol allowance and caller's
cap, never from peer-declared framing, and uses one fixed scratch. The bodyless path allocates no body
region. This makes the resource promise mechanically testable: body cap plus cumulative head
allowance plus scratch. The default path keeps its existing one-buffer layout, while a caller asking
for a hard bound accepts the explicit two-region response allocation needed to avoid transient growth.

## Why HTTP streaming is a dependent reader with a consuming SSE transition

A response stream has two owners that cannot be separated: the checked-out connection and the
client pool to which an exactly completed connection may return. Making the stream retain an
inferred shared borrow of its client expresses that relationship without a hidden reference-counted
pool or visible lifetime syntax. Other shared client requests remain legal; moving or dropping the
pool before the stream does not. Drop before exact completion closes immediately and never drains in
the background, so both side effects and latency stay visible. Builtin `Option`/`Result` already have
active-tag Move/Drop and provenance machinery, so the positive carrier grammar contains only bare
streams and finite nesting through those tags. One exhaustive no-wildcard storage-graph classifier
rejects every other edge by default, including user records/sums, anonymous tuples, collections,
boxes, builders, tasks, and captures. This fail-closed rule keeps the client dependency from becoming
an unchecked reachable-field property and makes a future `Ty`/`Scalar` constructor reopen a compiler
tripwire instead of relying on a maintained blacklist.

Raw receive follows the settled I/O rule: a read fills a caller-owned fixed-capacity `buffer`.
De-chunking is protocol mechanism and stays inside `std.http`; body allocation is policy and stays at
the call site. An unset streaming request has no cumulative total cap because it never materializes
the complete body, while a positive `max_response_body_bytes` remains enforceable and is never
silently ignored. This is what allows both large downloads and indefinite event streams without a
second client or a magic larger default. Long life does not mean unbounded work in one operation:
the whole-message chunk-framing counter becomes a replenished per-`read`/`next` allowance, so a
stream can continue indefinitely only by returning bounded progress to its caller.

SSE is a consuming type transition rather than a boolean mode. After `sse()`, raw reads are absent
from the type, so framing bytes and event state cannot be interleaved accidentally. `next` also fills
caller storage and returns three string views into its fresh generation plus one inline Copy retry
value; it does not allocate one owned string per token. ID/retry changes are block-transactional:
control-only blocks commit at their blank line, while a data-bearing block commits only with event
publication. Rolling back the current block on failure preserves an earlier control commit without
advancing the reconnect cursor past an event the caller never received. WHATWG decoding and dispatch
are mechanism. Redirects, media-type/status policy,
reconnect timing, sleep, and `Last-Event-ID` request construction remain caller policy, because an
automatic `EventSource` loop would hide network work and control flow; explicit stream accessors
still expose the latest id/retry state when a control-only block precedes EOF. The same explicit-bound
outcome, `Error.Code(-1)`, covers either a configured cumulative body bound or the selected event
buffer capacity, always without partial publication. A separate per-`next` source-work guard counts
comments, unknown fields, invalid retry lines, and control-only blocks as well as event input; its
fixed slack beyond caller capacity keeps protocol syntax bounded without turning ignored input into
hidden allocation.

---

## Why trusted filesystem access returns a file handle, not a path or directory handle

A canonical pathname is only a string observation. Reopening it would repeat ambient pathname
resolution and lose the directory identities that made the first observation trustworthy.
`fs.open_beneath` therefore performs the no-follow walk and returns the existing owned `reader`
bound to the final descriptor. `fs.create_exclusive_beneath` follows the same rule for the existing
owned `writer`. The proof and the usable capability are one value, with the language's normal Move
and Drop behavior.

The public surface remains deliberately narrower than an `openat` wrapper. A root plus strict
relative path makes containment visible in source, while a public directory-handle type, metadata
reflection, canonicalization, or mode flags would introduce new ownership and policy surfaces before
a consumer establishes them. Two named regular-file constructors also keep input validation and
exclusive output creation explicit; a mode bit that changed the final type or mutation behavior
would hide a material effect.

Descriptor-relative traversal closes ancestor replacement without changing cwd or installing a
global root. The constructor validates both complete strings first, walks with no-follow semantics,
revalidates object identity, and releases all temporary owners on failure. Native failures still use
the one settled `Error` mapping, so the safety boundary does not invent a filesystem-specific error
model.

---

## The package philosophy

A dependency should be *ordinary source in your tree*, not a resolved artifact. Align's package layer
adds **two path rules and zero new compiler concepts**: a "package" is the module subtree a tool (or a
human) vendors under `pkg/`, and the compiler never learns what one is — resolution, visibility,
effects, escape, and capabilities all carry over from the module system unchanged. Three forces
converge on this:

- **Namespaces must remain real at package boundaries.** A compiler-provided bare type alias cannot
  reserve the same word across every vendored module: that would make ordinary qualified APIs such
  as `pkg.db.Error` impossible even though user types already have canonical module identities.
  Non-entry modules therefore resolve a same-module declaration before a builtin alias. The closed
  explicit table is `core.Error` (always in scope), `crypto.argon2_params` plus the six settled
  `crypto.{rs256,es256,ed25519}_{private,public}_key` spellings (`std.crypto` import), and
  `regex.regex_match` (`std.regex` import). The signature-key entries shipped atomically with the
  asymmetric implementation. The entry namespace
  remains unmangled and rejects a true canonical collision. This preserves one lookup rule and does
  not weaken the no-shadowing rule for values.

- **Nothing hidden, extended to provenance.** The first import segment is a trust tier
  (`core`/`std`/`pkg`/project), so a file's header shows not just *what* it reaches but *whose* code
  it trusts. Import aliases are refused (`import x as y` would hide provenance at the call site), so a
  call stays fully qualified — `pkg.web.get(...)` — and the trust tier is visible at every use, not
  only in the header. Vendoring is literally copying the subtree; there is no source rewriting on
  vendor (hidden magic), no develop-layout vs installed-layout split.
- **Hermetic by construction, no manifest.** The package graph, like M15's unit graph, is discovered
  from imports + the filesystem — `grep 'import pkg\.'` *is* the dependency list, with no manifest to
  drift. One version of a package exists per tree because `pkg/<name>/` can exist once, so the diamond
  problem is resolved by whoever populates the tree, not by a version solver in the compiler; an
  incompatible major version is a new name (`pkg.web2`). Version *selection* is a fetch-tool concern
  that ends before the compiler starts.
- **AI-friendliness as the payoff.** Because dependencies are ordinary, greppable source in the tree,
  the whole dependency closure is in-context and auditable — the maximally AI-friendly shape. The two
  rules that make this safe are pure path checks: the **`internal`** rule lets a package keep
  implementation modules private (without it every module is permanent public API), and **layering**
  (`core → std → pkg → project`) keeps a vendored package from reaching back into the consuming
  project — which would compile in exactly one tree and invert the dependency arrow. One visibility
  model (`pub` + the `internal` path rule) is deliberately the whole story; a second granularity
  (`pub(pkg)`, export lists, re-exports) is the complexity budget Align refuses.

## Why regex is `std.regex`, not language syntax

Regex is useful application machinery, but it does not change Align's grammar, type system, or
optimization model. Making patterns ordinary `str` values and compilation an explicit fallible
operation preserves the language's existing rules: allocation is visible through an owned Move
handle, malformed input is a `Result`, repeated work is avoided by binding and reusing the handle,
and there is no hidden global cache. A regex literal would add lexer/parser/constant-evaluation
surface before any demonstrated need and would create a second way to compile the same pattern.

The engine choice follows the resource-oriented north star: an automata implementation with
predictable worst-case search behavior is preferable to a more expressive backtracking engine whose
runtime may explode on adversarial input. Therefore v1 deliberately excludes backreferences and
look-around, returns byte spans rather than allocating matched strings, and starts with only
compile/is_match/find/find_at. Captures/replacement/split can be added at the library boundary when a
real consumer establishes their ownership and allocation shapes; none requires a language change.

## Why asymmetric signature keys are algorithm-specific

The post-`pkg.db` asymmetric crypto extension uses six compiler-provided Move key types rather
than one generic provider handle: private and public types for RS256, ES256, and Ed25519. This
makes algorithm and signing-versus-verification confusion a type error, keeps ownership visible,
and avoids a runtime algorithm selector that would create a second cryptographic paradigm.

Construction is deliberately narrower than a general key-file API. Private keys accept one
bounded canonical unencrypted PKCS#8 v1 `PrivateKeyInfo` version zero in `PRIVATE KEY` PEM, public
keys accept canonical SPKI `PUBLIC KEY` PEM or
already-base64url-decoded JWK components, and sign/verify borrow an exact typed key. Password
callbacks, certificates, OpenSSH/traditional PEM, private JWK, key generation/export, and
provider selection remain outside the surface. `OneAsymmetricKey` and relabeled PKCS#1/SEC1 DER
cannot enter through format auto-detection: the one private path is PKCS#8-specific. Its exact
decoded and canonical-reencoding buffers are explicit secret owners cleansed before free. This keeps file, terminal, environment, and
network access out of crypto parsing and leaves encoding to the existing one
`encoding.base64url_decode` path.

Wire signatures follow the ecosystem formats that consume them: RS256 is PKCS#1 v1.5 with
SHA-256 at modulus width, ES256 is P-256/SHA-256 with exact JOSE `r || s`, and Ed25519 is pure
Ed25519. Separate public functions retain the one-way surface while one payloaded compiler key
kind and one checked runtime shell share the implementation proof. The authoritative exact
surface, validation/error precedence, private-secret cleanup, ABI, resource bounds, and implementation closure matrix are in
`docs/impl/std-design/crypto.md` “Asymmetric signature suite.”

The runtime shell owns one private OpenSSL library context and explicitly loaded built-in default
provider. Exact `provider=default` fetches plus key/operation provider-pointer checks make ambient
configuration unable to substitute the implementation. Ed25519 admission independently performs
canonical RFC 8032 point recovery and small-order rejection because provider `public_check` does not
own that invariant.

OpenSSL's error queue is thread-local ambient state, so every fallible call clears, immediately
drains/classifies, and clears it again. Only a closed input-rejection set becomes `Error.Invalid`;
an empty, unknown, mixed resource, internal, fetch, or unsupported failure becomes opaque
`Error.Code(0)`. This makes stale errors irrelevant and prevents allocation failure from looking
like malformed key data.

The timing boundary is deliberately honest about the borrowed engine. Key parsing, public-point
validation, and provider validation are trusted setup and make no timing promise; exposing them as
a repeated remote oracle is outside the contract. After admission, the signing wrapper never
extracts private components or branches/indexes on their contents, uses only the high-level EVP
signature operation, and relies on the pointer-verified built-in default provider's constant-time
primitive implementation with RSA blinding retained. Signature verification uses public material.
Functional vectors establish cryptographic semantics, while wrapper/API/provider-provenance
inspection — not noisy timing statistics — owns the constant-time boundary.

## Why logging is an explicit Move sink

Logging is application output, so Align makes its policy an ordinary owned value rather than a
process-global singleton. `log.new(writer, minimum)` visibly selects the destination, buffering,
threshold, lifetime, and cleanup owner. Moving the writer into a nominal logger prevents unrelated
code from bypassing the level and first-error state, while the existing handle rules provide
exactly-once Drop without a logging-specific ownership model.

The transfer owns the writer handle, not every descriptor behind it. File writers retain owned-fd
cleanup, standard streams retain their static process-fd borrow, and a logger made from a connection
writer retains the connection-derived region. That distinction prevents the wrapper from laundering
a borrowed descriptor into an owned or static one.

The first release has one operation, `line`, instead of one method per level or a reflection-based
structured-record API. Its explicit `log.level` argument keeps filtering data-driven without adding
dynamic global configuration. `Off` is both the complete disabling threshold and a suppressed
record value. The severity tags have a fixed order, so `enabled` and `line` share one comparison
instead of parallel per-level behavior.

Best effort is compatible with the one `Result` model because failure is deferred, not erased.
`line` retains the first writer status and returns Unit so logging does not add a branch to every
work path. `flush()` is the explicit observation point and maps that same status through std's
existing error table. Retaining the first failure both identifies the original cause and prevents
later writes from turning one broken sink into repeated I/O. A caller that omits the checkpoint
chooses the ordinary writer-Drop behavior, whose cleanup error is already unobservable.

Argument evaluation stays ordinary and eager. A lazy logging closure would introduce a logging-only
evaluation rule and a new capture/effect/ownership surface. `enabled(level)` instead gives callers
one explicit guard when template or builder construction is material. Formatting itself remains the
shipped template/builder path; the logger adds no variadic formatter, reflection, or formatter
registry.

The record transform escapes only backslash, LF, and CR, then appends one LF. This is the smallest
allocation-free rule that makes every completed record one physical line and distinguishes an
escaped line break from literal escape text. It is deliberately not terminal sanitization: tabs,
NUL, bidi controls, and other valid UTF-8 remain data. Security-sensitive output uses an explicit
encoder above the logger instead of silently changing text here.

Time, source location, process/thread identity, fields, JSON, rotation, file opening, dynamic
threshold mutation, asynchronous queues, and fatal behavior are absent for the same reason: each
would add an effect, allocation, ambient input, or policy that the caller did not spell. They can be
ordinary packages or explicit values if real consumers establish their contracts. The complete
surface and closure matrix are in `impl/std-design/log.md`.

## Why `core.codec` validates one canonical envelope before exposing views

The columnar format has two jobs that must not be confused. Its metadata makes one batch
self-describing enough to inspect; its child buffers keep ordinary numeric work on an
optimizer-visible typed-column path without per-element runtime calls. `ALNCOL01` therefore owns a
small fixed envelope while deliberately reusing Arrow's non-null physical layouts for `i64`, `f64`,
bit-packed bool, and 32-bit-offset UTF-8. Reusing the buffers allows later explicit adapters without
importing Arrow IPC's FlatBuffers schema, stream framing, nullable/nested type system, or
compatibility policy into the language core.

Validation is a one-time capability transition. Before `codec.open` returns, it proves every width,
offset, order, padding byte, unique name, bool tail bit, string offset, and UTF-8 range. The returned
batch is a Copy view tied to the input's region and storage generation, so the borrow checker—not a
second checksum or reparse—keeps those facts true. Every kind projects to one symmetric typed column
view whose `at` lowers to visible alignment-1 byte/bit/offset operations. This keeps validity
independent of `buffer`'s byte-aligned storage instead of overstating typed-pointer alignment. A
permissive decoder would
destroy the one-semantic-value/one-byte-value property and move uncertainty into every accessor, so
v1 rejects unknown flags, tags, gaps, nonzero padding, and trailing bytes.

The explicit Move encoder is the allocation home. Its `put_*` calls copy because retaining an
unbounded sequence of borrowed column regions would require runtime-changing lifetime facts on the
receiver. Each put first proves its complete prospective change, then commits atomically; a caller
can handle `Error.Invalid` and keep using the same encoder without hidden partial state. `finish`
consumes that one owner into `buffer`, preserving the existing ownership and hard-OOM models. A
compile-time reflected `soa<T>` codec, variadic columns, and a generic dynamic value were rejected:
each adds a second schema or generic mechanism before `pkg.frame` establishes a real need.

Allocation-free duplicate checking is deliberately capped at 1024 columns. The decoder uses two
fixed `[u16; 1024]` stack arrays and ten stable merge passes, bounding the full-size case at 9,217
lexicographic comparisons; the encoder keeps a sorted name index and rejects the next column before
mutation. Nullability is deferred as one whole decision.
Arrow validity bitmaps, source `Option` columns,
typed accessors, and encoder inputs must arrive together; accepting a bitmap flag now without that
Cartesian contract would create unreadable states. The same rule keeps nested types, dictionary
encoding, IPC, compression, streaming, and RPC out of v1. Exact bytes and the closure matrix are in
`impl/core-design/codec.md`.

## Why `pkg.frame` returns row ordinals instead of owning another frame

The codec batch already owns canonical dynamic metadata and typed zero-copy projections. Wrapping
it in a second `Frame` would duplicate schema identity, kind validation, ownership, and lifetime
rules before a consumer demonstrates any additional capability. Selecting columns also stays an
explicit caller operation: `find` followed by one typed projection makes missing-column and
wrong-kind policy visible instead of moving it behind a dynamic join API.

The first relational primitive therefore consumes two exact typed columns and returns an ordinary
`array<RowPair>`. Ordinals are sufficient for explicit gather operations, retain no input region,
and flow through the existing array pipeline without a query-plan value or materialized-column
copy. The right side is fixed as the build side and output is left-major/right-ascending, so
duplicate order does not depend on cardinality heuristics, allocator state, or hash iteration.

The explicit inclusive `max_pairs` makes duplicate fanout and output allocation visible. Counting
before one exact output allocation preserves all-or-nothing publication; a negative limit is
distinguished from an otherwise valid join that exceeds its result or right-index representability
bound. I64 and byte-exact str
reuse the settled equality/hash families. Bool's two buckets have no demonstrated consumer, while
f64 requires a separate `-0.0`/NaN hash-canonicalization decision, so neither is admitted by
analogy. The complete contract and implementation closure matrix are in
`impl/pkg-design/frame.md`.

## Why `pkg.auth` is protocol assembly, not another crypto or identity system

The sensitive primitives already have one owner in `std.crypto`: OS entropy, HMAC-SHA256,
Argon2id, and constant-time comparison. JWT and password storage still invite repeated
protocol-level mistakes—algorithm confusion, padded or alternate encodings, unauthenticated JSON
parsing, unbounded stored work factors, and ad hoc PHC spellings—so `pkg.auth` owns exactly that
assembly. Adding native JWT/PHC entry points would duplicate validation and cleanup across the
package/compiler/runtime boundary without improving the cryptographic trust surface.

Time and resource policy stay visible at the call site. JWT verification takes `now_ns` instead of
reading a clock, and password verification takes an `Argon2Policy` interpreted as three inclusive
maximum costs before it performs a KDF. Hash creation uses the same record as exact parameters;
there is deliberately no recommended default hidden in library code. The session-token operation is
fixed at 256 random bits because allowing a caller to select a weaker token length adds no useful
authentication policy.

The shared `json.doc` parser currently has two documented RFC 8259 leniencies: unescaped C0 bytes
inside strings and leading-zero numbers. Auth cannot sign or return those spellings as valid JSON.
One allocation-free package lexical pass rejects exactly those forms before the shared parser owns
the remaining grammar; changing only auth does not fork JSON semantics or silently tighten every
existing JSON consumer.

Claims remain bounded JSON text and a verified token returns those exact bytes. The package does
not infer issuer, audience, roles, cookies, storage, rotation, or revocation. Likewise, canonical
Argon2id PHC owns no password rules, pepper, automatic rehash, or user database. These are
application decisions, not alternate modes inside one auth helper. Ordinary borrowed byte inputs
and owned string results also preserve the existing ownership model; V1 states the current lack of
zeroizing string/buffer Drop instead of inventing a package-local secret type. Exact format,
precedence, allocation, and closure rules are in `impl/pkg-design/auth.md`.

Compilation-unit capability collection is module-wide, not call-reachability-based. Consequently a
session-token-only `pkg.auth` consumer still retains the module's HMAC/Argon2 capability and
libcrypto. The design records that existing cost explicitly instead of promising per-function
linking that the current whole/per-unit machinery does not provide.

## Why `pkg.kv` is a typed RESP2 client, not a generic Redis protocol API

One Redis connection carries a sequential byte stream, so a generic command/reply escape hatch
would let an incomplete nested reply silently desynchronize every later operation. The candidate
instead admits only `GET`, `SET`, and one-key `DEL`, each with a closed reply shape. An opaque Move
client and call-bounded `borrow mut` make one request/one reply the only concurrency model. A
transport failure, oversized response, or reply whose framing cannot be proved complete retires the
client before returning; only a complete bounded grammar-valid Simple Error payload (NUL/invalid
UTF-8 admitted, CR/LF excluded, exact CRLF terminator) or complete GET text payload that fails UTF-8
decoding leaves the stream synchronized and reusable. A Simple Error CR/LF violation is `Protocol`
and retires the client. Grammar, cap, framing, and same-read trailing validation finish before UTF-8
selects owned `Server` versus reusable `Decode`.

Endpoint, timeout, memory, condition, and expiry policy stay visible at the call site. `connect`
takes an explicit host, port, per-address connect timeout, socket I/O timeout, and inclusive reply
cap. `SET` takes a closed `Always` / `IfAbsent` / `IfPresent` condition and an optional positive
nanosecond duration converted upward to Redis `PX` milliseconds. This is enough for the first
`pkg.auth` session-store consumer: `IfAbsent` supplies an atomic token-collision check,
`IfPresent` avoids resurrecting a revoked or expired session, explicit expiry gives server-owned
retention, and one-key `DEL` supplies revocation. No client clock, retry, redirect, credential,
database, pooling, transaction, script, pub/sub, or ambient configuration is implied.

That visibility requires the existing timeout substrate to honor what the caller wrote. A positive
connect attempt therefore uses monotonic start-plus-budget arithmetic rather than an overflowable
absolute deadline, checks both nonblocking installation and blocking restoration, closes and
continues on either failure, rounds a remaining wait upward to milliseconds, and rechecks an early
zero from `poll`; an immediate/readiness result wins the logical deadline race. A nonzero resolver
result maps `EAI_NONAME`/`EAI_NODATA` to `Io(Invalid)` and every other EAI through the shipped
magnitude encoding to `Io(Code)`, leaving null output and attempting no socket. Only after successful
resolution is order preserved, with first success and last attempted failure. Failure to install
either I/O timeout after selection retires/closes the unpublished connection rather than trying
another address; send failure may have changed receive before close. Positive socket
I/O timeouts round upward to normalized microseconds. This repair is a prerequisite shared by
existing `std.net`/`std.http`; the same poll conversion and start/budget rule closes the complete
positive-i64 range for `process.command` without changing its timeout-wins checkpoint order. It is
not package-local policy or a claim that kernel scheduling supplies a strict wall-clock return.

RESP framing and validation remain ordinary package source. The only planned runtime addition is
one generic package-internal row for checked receive/send timeout installation. A non-null
compatible caller must hold one live/unfreed connection exclusively with no live reader/writer shell
derived from it and no other value retaining one at entry, and no read/write/configure/
reader-or-writer construction/free/Drop overlap. From entry `{R0,S0}`, receive failure retains both
states, send failure leaves `{T,S0}`, and success leaves `{T,T}`; the row does not roll back or close. Either option failure
requires caller retirement, forbids later read/write/configuration/reader-or-writer construction/
retry, and requires one later free/Drop. Success may construct derived shells, but another timeout
call requires all such shells and retaining values to Drop first; the package calls before shell
construction and closes its fresh unpublished connection on failure. The compiler knows its
physical symbol for typed ABI compatibility, collision, and reachability, but it is not a language
builtin or HIR/MIR operation. Package source explicitly
decodes the shared native-status table because ordinary extern calls do not receive builtin MIR
decoding automatically. Its internal modules import `std.process`; impossible
status/count/view-length/view-pointer/output products reach the existing
`ProcessAbort` capability before parsing or publication. A malformed private resource record is
also an internal invariant violation rather than a `Closed` producer: every operation and Drop
reaches `ProcessAbort` before native I/O or untrusted pointer access. SIGPIPE
safety is instead repaired in the existing connection-derived writer, so every `std.net` consumer
benefits and `pkg.kv` does not create a second byte-write path. Slice and builder writer overloads
converge on it. Its private socket sink selects
`MSG_NOSIGNAL` or checked `SO_NOSIGPIPE`; file and standard-stream writers remain unchanged. This
keeps Redis parsing out of the runtime, adds no language operation or public network surface, and
changes no existing writer ABI identity.

GET values and server errors are ordinary owned strings; keys and SET values are borrowed only for
their synchronous write. No reply or scratch view escapes, and the configured response cap bounds
the only value-sized receive allocation. Empty owned results use canonical `{null, 0}` without a
final buffer; nonempty results allocate one. V1 deliberately stays on plaintext RESP2 with no protocol
negotiation or TLS. The exact candidate contract, byte grammar, error precedence, runtime
reservation, and implementation closure matrix are in `impl/pkg-design/kv.md`. Its first independent
review reopened the timeout/native/cache axes, and the fresh complete review found four remaining
raw-view, source-reachable-lifecycle, resolver, and RESP-grammar gaps. The next complete review found
two P3 consistency gaps in the timeout action lists and malformed-state error partition; the
following review found one remaining P2 in the pre-existing-derived-shell entry state. No public
contract is accepted until a fresh complete review closes this fourth repair.

## Why tests are Result blocks run in separate processes

An Align test reuses the language's one error model. Its body is a compiler-private
`fn() -> Result<(), Error>` with a documented successful tail, so helpers return Result, `?`
propagates, Err fails, and ordinary cleanup runs. Assertions are explicit `core.test` operations;
there is no exception, panic-catching framework, boolean-returning second dialect, or hidden global
registry.

The ordinary parser's final-expression rule remains unchanged. Test-context sema treats an exact
final assertion as a statement only at root completion or when its enclosing block/control is
structurally a statement; every Value edge, including expected Unit, rejects. This preserves the one
statement-only assertion form without requiring a dummy trailing expression or inventing a
parser-only assertion node.

The declaration is not an ordinary named function. It has no parameters, visibility, callable
identity, or interface entry because production code must not acquire a test-only dependency.
Normal semantic formation closes and freezes the complete production program first. Test checking
then appends roots plus every test-generated helper, monomorph, type, descriptor, and capability to
a separate overlay. Normal commands validate the partition and consume only the frozen prefix,
while test mode combines both. This boundary preserves production ids and bytes instead of trying
to discard only visibly tagged root functions after they have already influenced shared tables.
Codegen/cache identity projects that prefix without source spans: diagnostics and located output
retain current offsets, while an earlier test edit cannot perturb production objects. The projection
still encodes each expression's ownership fact in structural order and every semantic descriptor
field, so span erasure cannot merge different cleanup or static-artifact meaning. Test mode has a
separate cache identity and links the explicit import closure once. Database descriptors are not an
overlay exception requiring a second preparation workflow: their constructors remain ordinary
named top-level descriptor functions formed in the prefix, and tests reuse that metadata offline.
The prefix selector is exercised across one-shot/watch, whole/per-unit, ThinLTO, and PGO modes.
Before test artifact work, the combined-view validator rejects every catalog-reachable
`ProcessCommand` through direct, imported, function-value, lifted, and concrete-monomorph edges.
This keeps the first capability honest about descendant containment without adding a hidden
sentinel/status protocol to `std.process`; unreachable production command helpers and all
production artifacts remain unchanged. The same exhaustiveness audit makes `align-repl` reject a
test-bearing submitted entry transactionally, and an implicit entry `main` rejects against an
imported declared `main` before catalog construction.

Each catalog row runs that same immutable artifact in a fresh process group. Process isolation is
the smallest boundary that contains a hard error, abort, exec, exit, or native crash without adding
unwinding to the language. A compiler-owned completion record means an early exit zero cannot
masquerade as a returned Ok. A fixed launch/acknowledgement exchange distinguishes harness setup
from user termination, and one deadline covers both states through cleanup. The parent control and
capture endpoints are nonblocking, so an acknowledgement or short output without completion returns
to poll instead of stalling that deadline. The driver snapshots one native suite cwd after CLI
validation. Every spawn installs it, replaces fd 0/1/2/3, and closes fd 4 and above, making child
cwd and descriptor visibility independent of later embedding-thread mutations. One dedicated runner
state machine owns signals, polling, capture, and wait status: every terminal path keeps the leader
unreaped while it signals the pinned group and then the direct PID, then reaps only its direct child
and continues only after cleanup succeeds. The direct target also closes a leader that left the
verified group. A second control drain after non-reaping terminal observation closes the fast-exit
race between completion send and status observation. Descendants are signalled but not reaped by
this parent. A scoped process-global controller owns SIGHUP, SIGINT, SIGQUIT, and SIGTERM from child
acquisition through summary publication. Returning error paths restore prior handlers; terminal
suite paths retain the controller, block and recheck those signals after the last write, then exit
directly. One lock-free `Idle/Writing/Selected/WritingPending` state prevents any new raw output
syscall after selection while preserving only an already-started syscall's prefix. Each handler
saves and restores the interrupted thread's exact `errno` around arbitration and self-pipe work.
The final guard
uses raw `_exit(128 + signal)`, so SIGHUP/SIGINT/SIGQUIT/SIGTERM are observed as numeric
129/130/131/143 `WIFEXITED` statuses, never as re-raised `WIFSIGNALED` termination. A prior ignored
or custom handler therefore cannot change a published terminal result.

The test artifact also has one entry and one child-control boundary. Its generated harness alone
owns literal `main`; every source-main ABI uses the existing encoded private identity and loses its
ordinary production wrapper. Four exact unkeyed runtime functions own launch receive, fd
close-on-exec, acknowledgement, and completion encoding/send, while the driver implements the
independent peer codecs. Target/profile/runtime LTO reach unit and harness objects; jobs, cache
statistics, timeout, and capture bounds each stop at their named scheduling, diagnostic, or runner
consumer.
After link, every whole/per-unit/harness build-stage owner completes normal cleanup before
signal-controller acquisition. Only the final executable stage enters the runner, because a raw
terminal exit cannot run Rust destructors for any build owner left alive.

Capture moves from a live row to an immutable quiesced row after child cleanup. A pass consumes and
discards it; a failure retains it through the last direct reporting write and only then releases it.
This makes a thousand passing tests produce the same one-line result as one passing test, without
sacrificing the diagnostic bytes at the failure site. Terse success is therefore part of the runner
contract, not an optional CI convention layered over an inherently noisy tool.
Repository owner and CI commands preserve the same property with the existing quiet wrapper:
success is phase/aggregate summaries, while failure or interruption replays the captured diagnostic
log without changing selection or verdict.

---

## In one sentence

Align is a data-oriented language that aligns human intent, AI generation, compiler optimization, and modern hardware.
