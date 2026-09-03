# History of Align

## 2026-09-03: pkg.csv settles on one typed direct-to-SoA materializer

The first `pkg.csv` design accepts one in-memory UTF-8 decoder whose expected `soa<R>` result is the
schema. Header presence, CRLF/LF selection, destination arena, and inclusive row bound are explicit;
there is no dialect inference, parser object, dynamic row model, or row-major result. Present
headers provide a bounded byte-exact named projection, absent headers require exact declaration
order, and RFC 4180 quoting is extended only to UTF-8 text and explicit LF separators.

A raw-ABI UTF-8 prevalidation followed by a complete allocation-free first decode pass validates
grammar, selected scalar conversions, bounds, and layout. Zero rows return canonical `{null, 0}` without allocation; a nonempty second decode pass allocates
an exact arena block and fills columns directly. Clean strings borrow the input; only doubled-quote
fields are normalized into the arena. Primitive records retain
only the output region, while string-bearing records retain both input and output. Errors publish
no partial result and do not advance the arena.

Implementation will activate canonical package admission, one checked `CsvDecode` HIR/MIR
operation, and reserved keyed runtime shape A123 atomically. Abstract generic checking carries a
discarded parameter-shaped record, while only concrete monomorph rechecking emits the operation;
the schema accepts the complete existing `SoaPlain` domain without a CSV field-count cap. The design itself changes neither the
five shipped package subtrees nor the 330-keyed/348-base runtime inventory. Exact surface, grammar,
conversions, precedence, ABI, ownership, and implementation matrix:
`docs/impl/pkg-design/csv.md`.

## 2026-09-02: pkg.kv is one typed synchronous plaintext RESP2 client

The accepted `pkg.kv` design fixes one opaque Move `client`, explicit connect and I/O timeouts plus
an inclusive response cap, owned-string GET, conditional/expiring SET, and one-key DEL. It is one
synchronous plaintext RESP2 text-value surface with no generic command value, default endpoint,
credential, database selector, retry, redirect, pool, transaction, script, pub/sub, protocol
negotiation, TLS, client clock, or ambient configuration.

Implementation begins with two independently useful shared prerequisites: checked full-range
timeout/deadline handling across TCP, HTTP, and process capture, then SIGPIPE-safe writes through
the existing connection-derived writer. The package activates one checked fixed-symbol socket-
timeout row only with its source consumer. Its connection lifecycle is closed over the complete
runtime-provenance reader/writer/logger retention graph, including recursive struct/tagged/sum and
fixed Move-struct-array carriers. Native impossibilities abort before parsing, publication, or
ownership change; malformed private state aborts before native I/O or untrusted pointer access.

A fresh full review accepted the fifth repaired ledger with no P0–P3 finding. Implementation then
shipped root `pkg.kv`, `pkg.kv.internal.resource`, and the checked ABI row together, bringing the
implemented first-party inventory to five vendorable subtrees. The current runtime inventory is 330
keyed plus 18 unkeyed records, 13 of the unkeyed records source-reachable: 348 base exports, 352
with either four-row probe feature alone, and 356 at the maximum combined probe surface. A123
remains the next unreserved ABI shape. Exact surface, ownership, wire grammar, error precedence,
prerequisites, and implementation matrix: `docs/impl/pkg-design/kv.md`.

## 2026-09-01: auth composes fixed protocols over the one crypto substrate

The first `pkg.auth` implementation shipped HS256 JWT, Argon2id PHC password records, and 256-bit opaque
session tokens as ordinary Align package code over shipped JSON, encoding, CSPRNG, HMAC, Argon2id,
and constant-time comparison. It adds no crypto primitive, native ABI, algorithm registry, clock
read, key fetch, session store, or identity policy.

JWT verification takes explicit Unix `now_ns`, authenticates compact bytes before JSON parsing,
pins HS256, and checks optional integer-form `exp`/`nbf`. A package lexical pass closes the shipped
JSON parser's raw-C0 and leading-zero leniencies. Password hashing takes exact work policy;
verification parses one canonical v19 PHC form and enforces caller-supplied cost ceilings before
the KDF. Session tokens have one fixed 32-byte/43-character shape. The package borrows inputs and
returns ordinary owned strings without claiming zeroization. Capability retention remains
module-wide, so every auth import retains libcrypto. Exact surface, formats, precedence,
and implementation matrix: `docs/impl/pkg-design/auth.md`.

## 2026-09-01: frame joins are bounded ordinal products, not a query language

The first `pkg.frame` capability settled one stable inner equi-join over exact typed codec columns.
It returns an ordinary owned `array<RowPair>` of left/right source ordinals, leaving column lookup,
typed projection, gathering, and later array work explicit. There is no Frame wrapper, second
schema system, materialized joined batch, expression tree, or query DSL.

I64 and byte-exact string keys reuse the existing hash/equality substrate. The right side is always
the build side, while output remains left-row-major and right-ordinal-ascending, including the
stable Cartesian product for duplicate keys. One required inclusive `max_pairs` bounds fanout and
output allocation: negative is `InvalidLimit`; an unrepresentable right index or the first pair
beyond the caller, i64-length, or target-byte range is `LimitExceeded`; OOM remains a hard abort. Inputs are borrowed only during the
call and the ordinal result retains neither codec batch. The exact public ledger, inactive ABI
rows, and implementation matrix are `docs/impl/pkg-design/frame.md`.

## 2026-09-01: columnar interchange settles on one validated canonical batch

The self-hosting library wave fixed `core.codec` as a data-only `ALNCOL01` envelope rather than
Arrow IPC or an RPC substrate. V1 carries ordered unique names and non-null `i64`, `f64`, `bool`,
and `str` columns. Its child buffers deliberately match Arrow physical layouts, while Align owns
the small canonical metadata, exact zero padding, strict validation order, and version boundary.

`codec.open` validates once without allocation and returns Copy views tied to the input region and
storage generation. All four kinds use total typed column views with alignment-1 little-endian
access, so `buffer`'s byte alignment cannot invalidate an envelope. The format caps columns at 1024
and uses fixed-stack merge sorting to bound allocation-free name uniqueness work. Encoding chose
an explicit Move accumulator with four transactional typed puts and a
consuming `buffer` finish. This rejected schema reflection, generic dynamic values, a permissive
future-tag reader, hidden borrowed-column retention, nullability without its complete Option/
validity contract, and premature Arrow IPC/stream/RPC scope. The decision and implementation
closure matrix are `docs/impl/core-design/codec.md`.

## 2026-08-31: owned string arrays index to the canonical text view

The R7 tokenizer consumer made the remaining Move-array gap concrete: an `array<string>` could be
built and dropped but its text could not be read as an ordinary expression. A general hidden
reference value for every Move element was rejected. Move-record arrays already support direct
bounds-checked field reads, including owned `string` fields as `str`, and explicit shared-borrow
calls already receive a checked element place without copying the record.

The one consumer-complete extension is therefore `texts[i] -> str`. The array remains the sole
owner, the view keeps the source generation and contained region roots, and existing index order,
termination, bounds, temporary-owner, and invalidation rules apply. There is no clone, allocation,
cleanup bit, source null, ABI change, or new indexing spelling. Whole Move records remain
unavailable as ordinary values.

## 2026-08-11: PostgreSQL streamed delivery is one explicit native rail

The first D13 rail shipped common bounded batches and direct SoA projection while retaining
PostgreSQL `BufferedFull`. The next rail fixes `SingleRow` and `PortalBatch(n)` as explicit runtime
delivery options shared by direct and prepared Query execution. It uses one result/cancel/Drop
state machine, retains parameter copies until protocol synchronization, and leaves static Query
artifacts unchanged. libpq 17 is the client floor for chunked rows and nonblocking cancellation;
the required server compatibility floor remains PostgreSQL 16.4. Binary formats, COPY, pipeline
mode, and LISTEN/NOTIFY remain independent rails rather than one oversized native PR.

The second design review exposed a pre-existing asymmetry: D12 gave SQLite catalog and EXPLAIN the
connection execution lease but allowed their PostgreSQL siblings to call libpq after only a live
state check. Streamed delivery therefore cannot land directly. One public/ABI-neutral prerequisite
first makes every PostgreSQL catalog and common/native EXPLAIN call share the typed-execution lease;
direct delivery follows after that independently testable safety closure merges. Prepared parity is
a third PR because exact `ParameterFormat(name, ...)` validation must retain the producer resolver
in statement v3. The direct rail preserves absent Delivery on both shipped BufferedFull timeout
subpaths and drains a multiplicity result to clean completion instead of canceling an effectful DML
`RETURNING`.

The next fresh review reopened the stream matrix a third time: validation, decode, and batch-storage
errors had no effect-aware pending-protocol cleanup, and rows state retained the absolute deadline
but not the original recovery duration. Those errors now normal-drain without decoding under the
original deadline, cancel only on expiry, preserve the first error, and pin Conn/Tx effect races.
Rows v3 uses offset 112 for the original duration. Chunking bounds rows per result and peak result
buffering, never total query transport.

The following review reopened the matrix a fourth time. COPY results cannot be completed by generic
`PQgetResult` drain, so deferred COPY support now fails closed: clear the current result, immediately
poison/close, make no later COPY/drain/cancel/state/restore call, and preserve any earlier primary
error. The same pass restored direct execution's settled phase order of live state, context-backed
generated static validation, lease, and then bind/native work. Both changes belong to the direct
stream state machine, so the three independently mergeable PR boundaries remain unchanged.

The clean-up review of that fourth redesign found three P2 consistency gaps but no new P1. The
ledger now exposes the monotonic caller-region cost of `one_native`: once a valid first Row is cloned,
the same bytes remain allocated on Cardinality or any later error. Delivery validation follows the
canonical §13.4 payload-before-duplicate order, superseding the earlier local choice, and the option
is inventoried as post-release D13 rather than part of initial D1--D12. None changes the three PR
boundaries.

The base-refreshed review reopened the matrix a fifth time. The earlier COPY closure was too narrow:
shipped synchronous and timeout PostgreSQL consumers can already receive COPY results and release a
connection that libpq still considers protocol-busy. A second public/ABI-neutral prerequisite now
audits every package-owned PGresult consumer and makes COPY plus unknown numeric statuses clear once
and close with no later protocol call. Direct and prepared explicit delivery also preserve D9's
clock recheck after enabling nonblocking mode and before send. The implementation sequence is now
four mergeable PRs: lease repair, result-status safety, direct delivery, and prepared parity.

The final preflight review found two P2 consistency gaps. `PGRES_PIPELINE_SYNC` and
`PGRES_PIPELINE_ABORTED` had been classified as ordinary invalid streamed sequences even though a
connection remains in pipeline mode until `PQexitPipelineMode`; because pipeline exit is outside
this rail, both statuses now share the immediate-clear-and-close branch at every PGresult consumer.
The section introduction now also names both prerequisites and the exact four-PR sequence.

The following preflight review found one further P2 in the same root-cause class. PostgreSQL
migration files could contain top-level COPY; the synchronous migration executor would receive a
COPY status and then attempt rollback on the protocol-busy connection. The status prerequisite now
audits both Rust PGresult modules and extends the existing driver-aware migration screen to reject
first-token COPY before URL access, target open, lock, history publication, or libpq. Preparation
cannot execute COPY through `PQprepare`; all remaining tool SQL is fixed producer-owned text. This
closes the tooling path without changing the four implementation boundaries.

The next review found one P2 in the same tool closure. Merely inventorying prepare/migration SQL
origins left their PGresult decoders free to clear an unknown or deferred status and then issue
`ROLLBACK`, `DEALLOCATE`, or another query. The status prerequisite now adds one private exhaustive
Rust tool classifier and routes every prepare/migration result consumer through it. Null results,
COPY, partial single/chunk rows, pipeline statuses, and unknown numeric statuses close and null the
connection owner before return; later row access and every follow-up libpq call are forbidden. One
parameterized synthetic-result owner fixes current-clear, close, first-error, and no-follow-up-call
behavior. This P2 closes inside the same prerequisite without reopening the matrix or adding a PR.

The final base-bound review found three P2 contract gaps and no new P1. Stream protocol state now
precedes ordinary status-to-error mapping, so every result after the zero-row terminal reports the
invalid sequence while its status still chooses drain or immediate close. PostgreSQL migration
screening is explicitly one statement-ordered classification pass and owns both `BEGIN; COPY` and
`COPY; BEGIN` precedence. Runtime Delivery remains outside static Query semantics, but landing its
public enum and the rows/statement ABI deterministically invalidates affected dependency-interface
and implementation cache keys once. These closures do not change the four implementation PRs.

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

---

## Asymmetric signature suite settled

On 2026-08-30, the post-`pkg.db` crypto convergence item was settled as an implementation-pending
RS256, ES256, and Ed25519 suite. It uses six distinct private/public Move key types and per-algorithm
construct, sign, and verify functions. Private construction accepts only bounded canonical
PKCS#8 v1 `PrivateKeyInfo` version-zero PEM through a PKCS#8-specific path; public construction
accepts canonical SPKI PEM or decoded JWK components. `OneAsymmetricKey` and relabeled PKCS#1/SEC1
DER reject, and wrapper-owned private DER storage is cleansed before free. RS256 fixes PKCS#1
v1.5/SHA-256, ES256 fixes P-256/SHA-256 and raw 64-byte JOSE signatures, and Ed25519 fixes pure
Ed25519.

The decision rejected a generic key handle, runtime algorithm strings, encrypted/traditional PEM,
private JWK, key generation/export, and ambient password or provider selection. Sign and verify
borrow their typed keys; constructor/key-format or malformed internal-ABI input is `Error.Invalid`,
only a closed per-call OpenSSL input-rejection queue maps to Invalid; empty/unknown/resource/internal/
fetch failure remains opaque `Error.Code(0)`, and every post-view signature mismatch
is `Ok(false)`. Each key shell owns an isolated OpenSSL context and explicitly loaded built-in
default provider; exact property fetches and provider-pointer checks prevent ambient substitution.
Ed25519 admission independently validates canonical RFC 8032 point recovery and rejects small-order
points. The settled timing boundary treats construction as trusted setup without a timing promise
and signing as constant-time for secret contents at fixed public lengths under that pointer-verified
built-in default-provider dependency. The public-contract and implementation-closure ledger is
`docs/impl/std-design/crypto.md` “Asymmetric signature suite.”
