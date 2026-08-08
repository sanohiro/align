This directory holds the authoritative per-package design docs for **first-party `pkg` libraries**,
at the same depth as `../std-design/` (signatures, Move/effect classification, error policy, slice
plan, pitfalls, test anchors). First-party packages are developed in this repo and distributed as
ordinary vendorable `pkg` subtrees.

# pkg — db

> **Call convention:** public call sites are fully qualified — `pkg.db.*`, `pkg.db.sqlite.*`, and
> `pkg.db.postgres.*` — so package provenance stays visible. For readability, the shorter `db.*`,
> `sqlite.*`, and `postgres.*` spellings in examples are shorthand for those fully qualified names.

## Status

**DESIGN OF RECORD — query-centric SQL/database package.**

Initial required drivers: **SQLite and PostgreSQL**.

The semantic decisions and API shapes in this document are the contract. Implementation begins only
after the general library-boundary prerequisites in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md) ship. Those
features are required Align language/compiler work, not database-private builtins and not optional
future cleanup.

Normative words (`MUST`, `MUST NOT`, `SHOULD`, `MAY`) are intentional requirements.

---

## 1. Executive decision

Align database support is **SQL-native and Query-centric**, not model-centric and not ORM-first.

> SQL defines the relational work. A named Query defines the bound parameters and the exact physical
> rows returned by the database. Ordinary Align code may turn that one row stream into an application
> output in one visible pass.

The primary unit is a named Query module:

```text
Query module
  SQL source          exactly one executable application SQL statement
  Params              values bound to the statement
  Row                 exact flat columns returned by the database
  Output              optional logical application result
  query()             static typed Query descriptor
  run()/shape()       optional one-pass result assembly; no additional SQL
```

The database layer is intentionally thin:

- SQL is the source of truth for reads and writes.
- SQL migration files are the source of truth for schema evolution.
- SQLite and PostgreSQL are both initial drivers, not later compatibility work.
- Common mechanisms share one API when only common behavior and types are used.
- Database-specific connection, Query, prepare, execution, transaction, metadata, and plan controls
  remain available and visible.
- A normal `alignc build` is offline and never contacts a database.
- Optional database-generated Query metadata strengthens checking and may be mandatory in CI.
- Plain Align structs may be reused, but there is no database-level `Model` or `Entity` concept.
- JOINs, CTEs, grouping, window functions, native operators, and database-specific optimization stay
  visible as SQL.
- One named Query executes one visible SQL statement once. Result shaping never issues relationship
  queries or other hidden SQL.

This design treats a transaction/master projection, aggregate report, or one-to-many read as normal.
Loading one table-shaped value is the simple special case, not the conceptual center.

---

## 2. Design principles

### 2.1 SQL stays SQL

Align MUST NOT replace SQL with a LINQ-style DSL, Active Record surface, ORM relationship language,
or a general SQL AST builder.

Complex database work remains visible:

```sql
SELECT
    c.id AS customer_id,
    c.name AS customer_name,
    COUNT(o.id) AS order_count,
    SUM(o.amount) AS total_amount
FROM customers AS c
JOIN orders AS o
    ON o.customer_id = c.id
WHERE o.created_at >= :from_date
GROUP BY c.id, c.name
HAVING COUNT(o.id) >= :minimum_orders
ORDER BY total_amount DESC
```

The package adds typed parameters, typed rows, predictable ownership, explicit execution, migrations,
and metadata. It does not add a second query language in front of SQL.

### 2.2 Queries are primary; structs are data shapes

There is no special `model` declaration.

A struct may describe:

- Query parameters;
- one database row;
- a reusable domain value;
- a Query-specific projection;
- a logical compound output assembled from rows.

It does not imply a table, primary key, relationship, INSERT, UPDATE, migration, or lazy load.

```align
pub User {
  id: i64,
  name: str,
  email: Option<str>,
}
```

The same `User` may be returned by several Queries. A JOIN or report may instead use its own `Row`
and `Output` types. Type proliferation for materially different projections is acceptable; pretending
that every database answer is one canonical entity is not.

### 2.3 Nothing hidden

The following must remain visible in source or in the declared Query contract:

```text
SQL statement
SQL execution count
parameter binding
parameter retention/copy class
allocation destination
error boundary
transaction boundary
result materialization mode
native database dependency
result-shaping pass
```

A field access can never execute SQL. A shaper can never execute SQL. No driver may silently retry,
paginate, add a SELECT, split one Query into several round trips, or ignore a native option.

### 2.4 Mechanism may be hidden; cost class may not

The implementation may choose libsqlite3 calls, libpq calls, binary decoding, statement caching,
buffer reuse, or another internal mechanism. The public contract must still make the cost class clear:

- `one`/`maybe_one` decode at most two delivered rows to decide cardinality; this is not a universal
  bound on database transport or driver buffering;
- SQLite steps at most twice for `one`/`maybe_one`;
- the initial PostgreSQL path uses libpq full-result delivery, so the server result is transported
  and buffered in full before the cardinality decoder inspects at most two rows;
- `all` materializes the full result into the supplied region;
- `rows` is one-pass consumption, not by itself a promise of network streaming;
- `next_batch` materializes one bounded batch;
- shaping is one pass unless its code visibly asks for another data structure or sort.

Driver tests/bench observations label physical delivery as `Step`, `BufferedFull`, `SingleRow`, or
`PortalBatch` and pin transported/buffered/decoded row counts where the native API exposes them.
The baseline D4/D8 PostgreSQL path is `BufferedFull`; D13 adds explicitly selected single-row/portal
paths. A requested delivery option is applied or rejected as unsupported, never silently downgraded.

### 2.5 Thin common layer, explicit native extensions

The common layer covers stable mechanisms:

- connection and execution handles;
- typed Query and command descriptors;
- parameter binding;
- prepared statements;
- execution modes;
- transactions;
- errors;
- common metadata.

It MUST NOT reduce SQLite and PostgreSQL to a least-common-denominator feature set.

Driver packages expose visible native controls at every layer where they matter:

```text
connection
Query definition
prepare
execution
transaction
metadata
EXPLAIN / query plan
```

Code using only common SQL/types/options should keep the same Align interface across SQLite and
PostgreSQL. Code using a PostgreSQL or SQLite feature is deliberately pinned to that driver.

### 2.6 Offline normal builds

`alignc build`, `alignc check`, and ordinary module checking MUST NOT connect to a live database or
the network.

Authoritative database parsing/type resolution may run in an explicit preparation command and its
result may be stored as checked metadata. The compiler need not reimplement PostgreSQL or SQLite SQL
semantics to make ordinary builds offline.

---

## 3. Non-goals

Version 1 does not include:

- Active Record;
- Hibernate-style persistence;
- relationship declarations on structs or fields;
- lazy loading;
- identity maps;
- change tracking;
- automatic joins;
- implicit N+1 queries;
- schema generation from structs;
- migration generation from structs;
- runtime reflection-based row mapping;
- a general Query-builder DSL;
- automatic SQL dialect rewriting;
- stored-procedure abstraction;
- distributed transactions;
- transparent retries;
- transparent pooling;
- automatic selection of a “best” relational fetch strategy.

A pool may later be a separate explicit `pkg.db.pool` package. Its presence must not change Query
semantics or hide acquisition/wait costs.

---

## 4. Package layout

### 4.1 Initial package and public modules

```text
pkg.db
pkg.db.sqlite
pkg.db.postgres
```

Under Align's settled package rule, these are **three public module boundaries in one vendorable
`pkg/db` package subtree**, not three independently versioned packages. `pkg.db` is the root common
module; `pkg.db.sqlite` and `pkg.db.postgres` are driver submodules. This preserves the requested
qualified API names while respecting one package version per build and the acyclic import graph.
The root/internal layer never imports a public driver module; driver modules call downward into the
common/internal layer.

Possible future public submodules or separately named package roots:

```text
pkg.db.pool
pkg.db.odbc
pkg.db.mysql
pkg.db.duckdb
```

The common module owns semantic contracts and closed internal resource dispatch. Driver submodules
own public connection construction, native types/options, authoritative prepare/describe
integration, and driver metadata. A future independently distributed third-party driver uses a
distinct package root unless Align first settles an external driver-registration mechanism; v1 does
not pretend `pkg.db.thirdparty` can be independently versioned.

### 4.2 Required general language foundation

Version 1 is implemented as first-party package modules over the general library-boundary foundation
defined in [`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md).
The following must ship before a database driver:

- recursive tagged Move payloads for structured owned errors and Move outputs through `Result`;
- `borrow` and `borrow mut` function parameters with interface-visible return-borrow summaries;
- package-defined opaque/dependent Move resources with exactly-once Drop, `resource_ref<R>`, and
  owner-tied native views;
- `arena name {}` and its scope-bound `region` capability;
- deterministic compiler-registered static source inputs and Query/command artifacts;
- region-backed `array_builder<PlainStruct>`.

The same machinery is available to `std.http`, `std.net`, `std.process`, and other native libraries.
`pkg.db` does not depend on `std.http`, and the compiler must not recognize database package names to
implement ownership or borrow safety.

The design does not require:

- a `database` keyword;
- annotations or decorators;
- reflection;
- user-defined traits;
- row polymorphism;
- structural-record types;
- operator overloading;
- macros;
- a second compile-time language.

The compiler recognizes static Query constructors such as `db.query_file([])` because they need the
expected `Params` and `Row` types, a tagged file/inline source identity, a SQL-content hash, and
build-dependency tracking. This is a narrow builtin-driven static-data feature, analogous to other
compiler-known formats, not general metaprogramming.

### 4.3 Common concrete handles

The semantic type set is:

```text
db.conn
db.tx
db.exec
db.query<P, R>
db.command<P>
db.stmt<P, R>
db.rows<R>
db.exec_result
db.Driver
db.row
db.value
```

Required meaning:

- `db.conn` is an opaque Move resource owning one physical SQLite or PostgreSQL connection.
- `db.tx` is an opaque Move resource that owns the connection moved into a transaction. Dropping an
  active transaction performs fail-safe rollback and then closes the connection, never commit.
- `db.exec` is a short-lived borrowed execution view produced from either a connection or a
  transaction. This lets a Query be reused inside and outside a transaction without a public trait.
- `db.query<P, R>` is a Copy static descriptor containing SQL identity, parameter contract, row
  contract, driver restriction, exact source hash, selected wire hash, and static options.
- `db.command<P>` is the non-row descriptor. A statement with `RETURNING` is a Query.
- `db.stmt<P, R>` is a prepared Move statement carrying an inferred dependency on the physical
  connection that prepared it.
- `db.rows<R>` is a Move, one-pass typed row stream for exactly one execution, carrying a dependency
  on its statement/connection while native buffers need it.
- `db.exec_result` is the allocation-free Copy affected-row record from §6.1.
- `db.Driver { SQLite, PostgreSQL }` is the exact public driver identity used in errors, metadata,
  delivery observations, and D14's explicit dynamic-SQL restriction; it has no `Any` variant.
- `db.row` and `db.value` are explicit dynamic escape hatches.

The common root declares the resource types and names `pub`, raw-only Drop hooks in
`pkg.db.internal.resource`. Each hook is an ordinary function with an `unsafe {}` body; the resource
declaration generates the root producer's hidden support thunk used by separate-compilation cleanup.
Resource representation privilege applies to the declaring module's descendant subtree. A public
driver descendant imports `pkg.db`, obtains tagged raw native state from raw-only internal FFI
helpers, checks failure, and calls `resource.from_raw` with expected type `db.conn`; no public raw
constructor exists. The internal Drop-hook module accepts only `raw` and does not import `pkg.db`.
The dependency direction is therefore:

```text
pkg.db                    -> pkg.db.internal.resource   (raw Drop/dispatch)
pkg.db.sqlite/postgres    -> pkg.db + pkg.db.internal  (construct root resource)
pkg.db.internal.*         -X-> pkg.db                   (no reverse import)
```

The root/common module never imports its public driver submodules. Adding a first-party driver
deliberately extends the closed tagged internal dispatch. There is no public driver trait, trait
object, or driver-provided resource ABI.

Execution views use the required borrowed-parameter surface:

```align
pub fn exec_conn(borrow c: db.conn) -> db.exec
pub fn exec_tx(borrow t: db.tx) -> db.exec
```

`db.exec` is a Copy sum over `resource_ref<db.conn>` and `resource_ref<db.tx>`. Its provenance is
therefore the source resource generation. Moving/dropping a connection, or committing/rolling back a
transaction, invalidates every derived execution view.

### 4.4 Implementation boundary

After L1a–L7, the following are ordinary first-party Align package code:

- public handles/descriptors/options/errors/metadata data shapes;
- the closed common/SQLite/PostgreSQL module API;
- safe wrappers around private `extern "C" link("sqlite3")` and `link("pq")` declarations, with
  the supported libpq OpenSSL closure declared explicitly through `link("ssl")` and
  `link("crypto")`;
- connection/transaction/statement/rows lifecycle code using general resources;
- driver-specific bind/step/result/metadata calls;
- Query-local `run`, Pure shaping steps, builders, and output types;
- migrations as SQL and their explicit package/tool orchestration.

The compiler/frontend owns only work that cannot be expressed as runtime package code:

- L1a–L7 language, ownership, region, generics, interface, and MIR support;
- recognized static Query/command constructors and exact input tracking;
- descriptor/command type checking from expected `Params`/`Row`;
- dialect-aware placeholder occurrence/source maps and statement screening;
- versioned static artifacts and interface/implementation hashing;
- generated direct field-offset binder and ordinal decoder thunks;
- Query diagnostics mapped to `.sql` spans.

`align_runtime` needs general helpers only: region-builder chunk/compact allocation, checked
owner-tied native view construction/UTF-8 validation, and existing arena/allocation primitives. It
MUST NOT contain SQLite/PostgreSQL Query semantics, SQL parsing, field reflection, or DB handle
types. Basic driver calls go directly from package-generated code to libsqlite3/libpq; the common
PostgreSQL dispatch also names libssl/libcrypto explicitly so static ELF links retain libpq's TLS
dependency closure. When `pq` is present, the driver preserves the discovered link list and appends
one deterministic closure tail (`pq`, `ssl`, `crypto`, then the supported `zstd`/`z` closure) so
suffix libraries that introduce their own native references are resolved too.

`align_driver` owns explicit tooling (`alignc db prepare`, later migrate/status/check), deterministic
artifact I/O, and tool-only database/schema setup. The external SQLite/libpq libraries remain the
authoritative SQL engines. This division keeps normal package behavior ordinary, confines unsafe
native boundaries, and prevents either `std.http` or a database-specific compiler ownership path.

---

## 5. Named Query modules

### 5.1 File convention

The normal layout pairs one Align Query module with one sibling SQL file of the same basename:

```text
db/
  queries/
    user_by_id.align
    user_by_id.sql
    user_with_groups.align
    user_with_groups.sql
    transaction_list.align
    transaction_list.sql
```

`user_with_groups.align`:

```align
module db.queries.user_with_groups

import pkg.db

pub Params {
  user_id: i64,
}

pub Row {
  user_id: i64,
  user_name: str,
  group_id: Option<i64>,
  group_name: Option<str>,
}

pub fn query() -> db.query<Params, Row> = db.query_file([])
```

The path-free `query_file([])` resolves the same-basename sibling SQL file:

```text
user_with_groups.align -> user_with_groups.sql
```

The Query module, not the SQL pathname, is the application-facing identity. Callers import the module
and call `user_with_groups.query()` or its `run()` helper. They do not pass filesystem paths around.

### 5.2 Explicit path override

A Query may explicitly link another static SQL file:

```align
pub fn query() -> db.query<Params, Row> =
  db.query_file("legacy/user_lookup.sql", [])
```

Rules:

- the path is a compile-time string literal;
- it is relative to the defining `.align` module's directory;
- absolute paths are rejected;
- lexical `..` or symlink escape outside the project/package root is rejected;
- the file must be UTF-8 and is registered in `SourceMap`;
- the exact source SQL bytes are hashed without newline normalization; the selected deterministic
  driver wire entry is sent at runtime;
- the file content is a build input;
- a changed file changes the Query hash and invalidates checked metadata.

This path is a definition-time linkage mechanism, not an execution argument.

### 5.3 Inline SQL

Short SQL may be inline:

```align
pub fn query() -> db.query<Params, User> =
  db.query("SELECT id, name, email FROM users WHERE id = :id", [])
```

The expression must be static. A runtime `string` cannot become a typed Query.

Inline SQL has `SqlSourceIdentity::Inline { query_id }`; it does not receive a fake `.sql` path.
`source_sql` is the exact UTF-8 value after Align string-escape decoding. The artifact retains a
decoded-byte-to-defining-literal span map, so scanner/prepare/runtime diagnostics point back into the
`.align` literal. The defining `.align` source is already a unit input; the decoded bytes/hash and
tagged inline identity enter the Query artifact and producer implementation identity.

Complex SQL SHOULD use a `.sql` file because it is easier to review, format, run in database tools,
EXPLAIN, and diff as SQL.

### 5.4 Descriptor, artifact, and incremental identity

`db.query<P, R>` is the compiler-known Copy descriptor specified in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md). The Query module
owns:

```text
public interface
  descriptor item name
  Params and Row types
  driver restriction
  static semantic options

producer implementation / StaticQueryArtifact
  structural Params/Row contracts and fingerprints
  SQL source identity: File(logical path) | Inline(query_id)
  exact source bytes/hash
  deterministic per-driver wire bytes/hash and source map
  parameter occurrence/source-span table
  per-driver binding plan and parameter retention classes
  per-driver checked-metadata policy/state/digest
  declared QueryMeta plan and per-driver checked evidence
  generated bind and decode thunks
  generated QueryMeta materialization plan
```

A static constructor is legal only as the complete single-expression body of one named,
zero-argument, non-generic descriptor function. The body contains exactly one resolved constructor
call. Conditional, repeated, block-bodied, nested, helper-wrapped, and ordinary expression uses are
compile errors. A private descriptor is valid within its module; `pub` exports it. The Query ID is
the fully-qualified module path plus descriptor function name, never an absolute filesystem path or
an ambiguous call span. Two descriptor functions in one module therefore receive distinct
artifact/thunk slots. A SQL-only edit recompiles/relinks the producer and invalidates checked
metadata, but it does not recompile consumers when `Params`, `Row`, driver restriction, and static
semantic options are unchanged. A public-contract change updates `interface_hash` and invalidates
consumers.

The runtime descriptor points to immutable producer-owned static data, per-driver wire/binding/
checked entries, and generated binder/decoder thunks. `P` and `R` are compile-time contracts; there
is no runtime reflection, per-row field-name lookup, or consumer-side Query-body instantiation.
The producer-owned QueryMeta plan is inert descriptor data before D12. D12 introduces its exact
materialization thunk ABI and execution-header version together with the first native metadata
consumer; Q2 does not reserve or populate a dormant function-pointer slot. The eventual thunk
materializes inspection rows into the caller's region without inspecting decoder code or opening
`.align-db` at runtime.

### 5.5 One descriptor, one statement

A static Query source contains exactly one executable application statement.

Allowed around it:

- comments;
- whitespace;
- one statement containing CTEs, subqueries, `UNION ALL`, window functions, native syntax, or a
  `RETURNING` clause.

Not allowed as one Query:

- two semicolon-separated application statements;
- an implicit setup statement followed by a SELECT;
- an implicit relationship SELECT;
- driver-generated pagination statements.

The contract is:

```text
one named Query
  = one static descriptor
  = one visible SQL statement
  = one execution
  = one Row stream
  = zero hidden follow-up statements
```

Migration files are a separate tool format and may contain multiple statements according to the
migration runner’s documented rules.

### 5.6 Params

`Params` is a named struct. Every distinct SQL value-placeholder name maps to exactly one field. The
same name may occur more than once and always reuses that field.

Portable SQL uses named parameters in source:

```sql
SELECT id, name
FROM users
WHERE active = :active
  AND id >= :min_id
```

```align
pub Params {
  active: bool,
  min_id: i64,
}
```

The prepare step lowers named parameters to the driver protocol:

- SQLite resolves and binds each native `:name`;
- PostgreSQL assigns `$1`, `$2`, ... by first lexical occurrence and reuses the same ordinal for
  later occurrences of the same name;
- both retain original byte spans and names in diagnostics and metadata.

Source SQL and wire SQL are distinct identities. `source_sql` is the exact reviewed file/inline
bytes and owns the static-input hash. SQLite's `wire_sql` is byte-identical to it. PostgreSQL's
`wire_sql` replaces only recognized placeholder token spans with the assigned `$n` bytes and
preserves every other source byte; it has its own hash and rewrite-format version. The artifact
stores both plus a source-to-wire span map. Prepare/runtime metadata is keyed by driver, source
hash, wire hash, rewrite version, and static options. Runtime sends the selected wire bytes exactly,
while diagnostics map engine positions back to source spans where possible.
For a NUL-terminated native entry point, generated storage appends one sentinel byte outside the
recorded pointer/length/hash domain; it is transport storage, not part of source or wire SQL.

The scanner is dialect-aware lexical analysis, not a regular expression and not a SQL resolver. It
recognizes quoted strings/identifiers, line/block comments, PostgreSQL dollar-quoted strings, and
the PostgreSQL `::` cast token. Portable Query source accepts only `:identifier` placeholders outside
those forms. Mixing `?`, `$1`, `@name`, or another native placeholder form into a portable Query is
an error; a driver-pinned Query may define an explicit native mapping.

The compiler/preparer reports:

- missing fields;
- unused fields;
- mixed or ambiguous placeholder forms;
- incompatible types;
- a parameter requiring an explicit native type declaration.

Values are always bound parameters. Value interpolation into SQL text is not a typed-Query feature.

SQL source bytes may not contain U+0000. Static Query/command screening reports the exact source
span before artifact generation; dynamic SQL and migration tooling reject it before the first
native call. SQLite and libpq expose different C string/length boundaries, so accepting an embedded
NUL would make statement count, source identity, and sent SQL disagree across drivers.

#### 5.6.1 Binding retention

The common execution contract borrows parameter storage only until the operation returns. This
includes `rows` and `rows_stmt`: before exposing a stream resource, the driver must finish binding
or retain every value in execution-owned storage. The returned `db.rows<R>` carries no borrow
provenance from `Params`; it remains dependent only on its statement/connection and native result
owners. The caller may therefore drop, replace, or mutate the original text/blob owner immediately
after `rows`/`rows_stmt` returns and before the first `next`.

Version 1 SQLite binding is exact:

- scalar fields are passed by value;
- `str`/`slice<u8>` and owned `string`/`array<u8>` payload bytes are bound with
  `SQLITE_TRANSIENT` semantics, so SQLite owns its required copy before the call returns;
- a prepared statement retains that native copy until reset/rebind/finalize, and the rows dependency
  prevents reset while the execution is live;
- partial-bind and native errors release every temporary and consume/drop each moved Params owner
  exactly once.

The baseline PostgreSQL `BufferedFull` path completes parameter transmission through the synchronous
libpq call before returning its owned result. Any later single-row, portal, pipeline, or asynchronous
path must retain its own parameter bytes until the native protocol no longer uses them.
The v1 PostgreSQL Text binder materializes a NUL-terminated execution-owned copy and rejects an
embedded U+0000 in `text`/`varchar`/`name` with `db.Error.Encode` before SQL send. A `bytea`
parameter in Text format is encoded as the PostgreSQL `\x` form with two lowercase hex digits per
input byte; raw bytes are never passed as a libpq Text parameter. Binary-format `bytea` passes the
raw bytes with their explicit length.

These are per-execution bind copies, never per-row copies. Tests and benchmarks report text/blob
bytes copied and allocations separately from row decode. The generated driver binder plan records
the retention class for every field, and Query/execution inspection reports the applicable
`BindValue`/`BindCopy` class; this copy is documented Query cost, not an invisible optimizer choice.
A future zero-copy bind mode requires an explicit driver-qualified surface, return provenance tying
the execution to every source owner, and its own measurements; a driver may not silently substitute
borrowed binding.

### 5.7 Row

`Row` is the exact, flat result-column contract.

```sql
SELECT
    t.id AS transaction_id,
    t.amount_cents,
    s.code AS status_code,
    s.name AS status_name
FROM transactions AS t
JOIN transaction_status AS s
    ON s.id = t.status_id
WHERE t.account_id = :account_id
```

```align
pub Row {
  transaction_id: i64,
  amount_cents: i64,
  status_code: str,
  status_name: str,
}
```

Rules:

- result aliases match field names exactly;
- there is no implicit snake_case/camelCase conversion;
- missing fields are errors;
- unexpected extra columns are errors in strict typed mode;
- duplicate result names are errors;
- nullable results map to `Option<T>`;
- a `LEFT JOIN` may make an otherwise non-null column nullable;
- casts are explicit in SQL when the database result type is ambiguous;
- decoding is generated/statically described, not reflection-based.

Version 1 static `Row` is structurally `RegionPlain`: scalar fields, `str`/`slice<u8>` views, and
one-column logical/`Option`/fixed-array forms recursively composed from them, with no independently owned
`string`, dynamic `array`, resource, raw, function, or builder field. This gives one representation
that can be streamed as current-row views or explicitly cloned into a caller region. Owned
`string`/`array<u8>` remain valid Params/Output data, but are not alternate generated Row storage
forms in v1. A later owned-Row collection requires a separate measured materializer contract; a
driver must not silently choose it.

The generated decoder writes directly to known `Row` field offsets. At prepare/execution setup, the
driver builds one column-ordinal plan and checks count, names, and available native types. The hot
row loop uses that plan and performs no name lookup.

`Declared` does not prove database types or nullability. SQLite storage classes are checked for every
decoded value. PostgreSQL native type identities are checked when the statement is described.
Unexpected NULL for a non-`Option` field and any incompatible runtime value return `db.Error.Decode`.

A reusable struct may be used directly as `Row` when its exact fields match. A Query-specific `Row` is
normal for joins and projections.

### 5.8 Output

`Output` is optional. It describes the logical value the application wants after reading the flat row
stream.

```align
pub Output {
  user: User,
  groups: array<Group>,
}
```

`Output` is not part of SQL type checking. SQL is checked against `Params` and `Row`. Query-local
Align code is checked normally while shaping `db.rows<Row>` into `Output`.

A simple Query often has:

```text
Output = Row
```

A compound Query may have:

```text
Row    = repeated flat JOIN rows
Output = nested or segmented application value
```

---

## 6. Execution surface

The common operation names and parameter ownership modes below are normative. Driver packages add
only the qualified native variants defined by their option scopes.

### 6.1 Commands

A non-row statement is a `db.command<P>`:

```align
pub Params {
  id: i64,
  name: str,
}

pub fn command() -> db.command<Params> = db.command_file([])
```

The command constructor has the same whole-body item restriction, static source identity,
source/wire hashes, parameter occurrence/source maps, per-driver binding/retention plan,
checked-policy map, interface/implementation split, and cache invalidation rules as a Query.
`StaticCommandArtifact` and `CommandStatic` omit only the Row contract, result-column metadata, and
decode thunk. The generated binder is mandatory; command execution never falls back to reflection or
runtime field-name lookup.

Execution returns:

```text
db.exec_result
  rows_affected: Option<i64>
```

`db.exec_result` is Copy and allocation-free. `rows_affected` is `Some(n)` only when the engine
reports a non-negative affected-row count that fits `i64`; commands without such a count return
`None`. The driver reads/converts it before releasing the native result. A native textual command
tag/status is not part of the first-release common result because it would require owned or
region-destined storage. Any later driver-qualified status API must expose that destination/owner
and its allocation explicitly.

A DML statement with `RETURNING` is `db.query<P, R>`.

### 6.2 Result modes

The common operations are:

```text
execute(exec, command, params, options: slice<db.ExecuteOption>)
  -> Result<db.exec_result, db.Error>
one(exec, query, params, out, options: slice<db.ExecuteOption>)
  -> Result<R, db.Error>
maybe_one(exec, query, params, out, options: slice<db.ExecuteOption>)
  -> Result<Option<R>, db.Error>
all(exec, query, params, out, options: slice<db.ExecuteOption>)
  -> Result<array<R>, db.Error>
rows(exec, query, params, options: slice<db.ExecuteOption>)
  -> Result<db.rows<R>, db.Error>
prepare(exec, query, options: slice<db.PrepareOption>)
  -> Result<db.stmt<P,R>, db.Error>
```

Semantics:

- `one`: zero rows -> `NotFound`; more than one -> `Cardinality`.
- `maybe_one`: zero -> `None`; one -> `Some`; more than one -> `Cardinality`.
- both stop decoding after a second delivered row, but retain §2.4's driver-specific transport and
  buffering cost;
- `one`/`maybe_one`/`all`: clone view-bearing fields into the supplied `region`; their results are
  tied to that region.
- `all`: its exact package definition is generic over `P, R: RegionPlain`, grows region chunks, and
  performs the region builder's one documented compacting pass. `RegionPlain` is the L7 closed
  builtin structural bound, not a public/user-defined trait hierarchy; every v1 static Query Row
  already satisfies it.
- `rows`: one-pass stream; no implicit materialization.
- `execute`: never decodes rows.

The package MUST NOT provide a convenience call whose name hides whether it materializes or streams.

### 6.3 Query-local run helpers

A Query module may expose its preferred operation:

```align
pub fn run(
  exec: db.exec,
  params: Params,
  out: region,
) -> Result<Option<User>, db.Error> {
  return db.maybe_one(exec, query(), params, out, [])
}
```

For a compound result:

```align
State {
  parent: Option<User>,
}

fn step(
  borrow mut state: State,
  borrow mut groups: array_builder<Group>,
  row: Row,
  out: region,
) -> Result<(), db.Error> {
  // Pure row-to-state code. Copy row views with `.clone_in(out)` before retaining them.
  return Ok(())
}

pub fn run(
  exec: db.exec,
  params: Params,
  out: region,
) -> Result<Option<Output>, db.Error> {
  mut state := State {
    parent: None,
  }
  mut groups := array_builder(out)
  mut rows := db.rows(exec, query(), params, [])?
  loop {
    row := db.next(rows)? else { break }
    step(state, groups, row, out)?
  }
  parent := state.parent else return Ok(None)
  groups_out := groups.build()
  return Ok(Some(Output {
    parent,
    groups: groups_out,
  }))
}
```

This keeps the SQL path and result mode inside the named Query module while preserving visible
execution and allocation. The normal `loop` is intentional: it avoids adding a database-special
callback ABI or higher-order database execution model merely to hide a loop. L7 supplies only the
nested generic types required by these ordinary package functions.
`borrow mut` gives the Pure step exclusive access without transferring the arena-owned Move state
or builder across a by-value call. The builder remains one separate mutable local; L6 does not put
builders in aggregate fields or add aggregate builder movement. Final validation that needs only
Copy/view state may run before `build()`, but the arena-owned array is not passed by value to a
helper. The Query-local `run` consumes the builder and constructs the returned Output/Result inline,
so the existing arena-owned call-boundary rule remains intact.

### 6.4 Prepared statements

Preparation is explicit:

```align
mut stmt := db.prepare(exec, query(), [])?
rows := db.rows_stmt(stmt, params, [])?
```

A prepared statement:

- is tied to one physical connection;
- owns or references the database statement handle;
- retains the checked parameter/row mapping;
- is Move;
- finalizes/deallocates on Drop;
- does not silently migrate between pooled connections;
- does not imply a global cache.

`prepare` returns a dependent resource tied to `exec`'s underlying connection.
`fn rows_stmt(borrow mut stmt: db.stmt<P,R>, params: P,
options: slice<db.ExecuteOption>) -> Result<db.rows<R>,db.Error>` returns a row resource tied to the
statement's fresh generation. The compiler therefore rejects reuse/finalization of the statement
until that row resource drops; after Drop, another execution may borrow the statement again. Direct
`rows(exec, query, ...)` similarly returns a resource dependent on the underlying connection.

A future cache is explicit and must expose its scope and capacity.

### 6.5 Connection and transaction reuse

A Query accepts `db.exec`, not a connection-specific public trait.

Conceptually:

```align
outside := db.exec_conn(conn)
inside := db.exec_tx(tx)

result1 := user_with_groups.run(outside, params, out)?
result2 := user_with_groups.run(inside, params, out)?
```

The two named constructors produce the same borrowed `db.exec` type. The important properties are:

- Query code is identical inside/outside a transaction;
- transaction ownership remains visible at the call site;
- `db.exec` cannot outlive its source;
- a transaction execution view cannot be used after commit/rollback;
- no dynamic trait object is needed.

---

## 7. Compound Query patterns

Compound Queries are first-class design cases, not deferred ORM features.

### 7.1 Many-to-one and master lookup

Transaction rows joined to master values should normally be one SQL projection:

```sql
SELECT
    t.id AS transaction_id,
    t.posted_at,
    t.amount_cents,
    s.id AS status_id,
    s.code AS status_code,
    s.name AS status_name,
    c.id AS customer_id,
    c.name AS customer_name
FROM transactions AS t
JOIN transaction_status AS s
    ON s.id = t.status_id
JOIN customers AS c
    ON c.id = t.customer_id
WHERE t.account_id = :account_id
ORDER BY t.posted_at DESC, t.id DESC
```

The Query may return the flat `Row` directly or map each row into a nested `OutputRow` in memory.
There is no `transaction.status` relationship that performs another read.

### 7.2 One-to-many: one parent

User plus groups uses one SQL statement:

```sql
SELECT
    u.id AS user_id,
    u.name AS user_name,
    g.id AS group_id,
    g.name AS group_name
FROM users AS u
LEFT JOIN user_groups AS ug
    ON ug.user_id = u.id
LEFT JOIN groups AS g
    ON g.id = ug.group_id
WHERE u.id = :user_id
ORDER BY u.id, g.id
```

Physical row:

```align
pub Row {
  user_id: i64,
  user_name: str,
  group_id: Option<i64>,
  group_name: Option<str>,
}
```

Logical output:

```align
pub Output {
  user: User,
  groups: array<Group>,
}
```

The Query-local exclusive state and Pure step perform one pass:

```align
State {
  seen: bool,
  user_id: i64,
  user_name: str,
}

fn step(
  borrow mut state: State,
  borrow mut groups: array_builder<Group>,
  row: Row,
  out: region,
) -> Result<(), db.Error> {
  if state.seen {
    if row.user_id != state.user_id {
      return Err(inconsistent_parent_id())
    }
    if row.user_name != state.user_name {
      return Err(inconsistent_parent_name())
    }
  } else {
    state.seen = true
    state.user_id = row.user_id
    state.user_name = row.user_name.clone_in(out)
  }

  group_id := row.group_id else {
    _ := row.group_name else return Ok(())
    return Err(partial_child())
  }
  group_name := row.group_name else return Err(partial_child())
  groups.push(Group {
    id: group_id,
    name: group_name.clone_in(out),
  })
  return Ok(())
}
```

`run` initializes `user_name` to the static empty view while `seen == false`, binds a separate
`mut groups := array_builder(out)` local, constructs exactly one `db.rows` stream, advances it in a
normal `loop`, and calls `step(state, groups, row, out)` once per row. It returns `None` when
`state.seen` is false; otherwise it validates/finalizes the Copy parent state, binds
`groups_out := groups.build()`, and directly constructs
`Ok(Some(Output { user: User { ... }, groups: groups_out }))` in `run`. It never passes the
arena-owned array through an ordinary by-value function call. The helper error constructors above
return fully populated
`db.Error` values for this Query identity. The recursive tagged-Move, builder, and region support
are mandatory prerequisites, not decisions left to the driver. The semantics are:

- no second SQL statement;
- no hidden lazy load;
- no hash map;
- no hidden sort;
- one row pass;
- child allocation goes to the supplied `region`;
- parent columns repeated by the JOIN are validated consistently;
- `NULL` child columns mean “no child row,” not a partially initialized child.

### 7.3 One-to-many: many parents

For a large parent list, allocating one independent child array per parent may create excessive
handles and fragmentation. The preferred data-oriented output is segmented:

```align
pub Output {
  users: array<User>,
  groups: array<Group>,
  group_offsets: array<i64>,
}
```

```text
users          [u0, u1, u2]
group_offsets  [0, 2, 2, 5]
groups         [g0, g1, g2, g3, g4]

u0 groups = groups[0..2]
u1 groups = groups[2..2]
u2 groups = groups[2..5]
```

SQL MUST order by the parent grouping key and then the desired child order:

```sql
ORDER BY u.id, g.id
```

The shaper uses adjacent groups only. It MUST NOT silently sort or hash the input.

A future general primitive may be named `group_adjacent_by`, but the first release may use a clear
Query-local loop. The primitive, if added, is a general ordered-stream operation, not a database
relationship feature.

### 7.4 Multiple child collections

A naive join of:

```text
User
  groups[]
  roles[]
  permissions[]
```

may produce a Cartesian multiplication. One statement is not automatically one efficient plan.

The SQL author explicitly chooses one of these visible strategies:

1. native aggregate values (`jsonb_agg`, `json_group_array`, arrays, etc.);
2. child-specific pre-aggregation in CTEs/subqueries;
3. PostgreSQL `LATERAL` or another native mechanism;
4. a tagged `UNION ALL` row stream;
5. an intentional flat product when the cardinalities are known and acceptable.

Tagged stream example:

```sql
SELECT
    u.id AS user_id,
    u.name AS user_name,
    'group' AS item_kind,
    g.id AS item_id,
    g.name AS item_name
FROM users AS u
JOIN user_groups AS ug ON ug.user_id = u.id
JOIN groups AS g ON g.id = ug.group_id
WHERE u.id = :user_id

UNION ALL

SELECT
    u.id AS user_id,
    u.name AS user_name,
    'role' AS item_kind,
    r.id AS item_id,
    r.name AS item_name
FROM users AS u
JOIN user_roles AS ur ON ur.user_id = u.id
JOIN roles AS r ON r.id = ur.role_id
WHERE u.id = :user_id

ORDER BY user_id, item_kind, item_id
```

The shaper visibly dispatches `item_kind` into separate builders. The package does not infer or create
this SQL.

### 7.5 Native nested aggregation

For a database-fixed application, native aggregation is encouraged when it is the best measured plan.

PostgreSQL example:

```sql
SELECT
    u.id,
    u.name,
    COALESCE(
      jsonb_agg(
        jsonb_build_object('id', g.id, 'name', g.name)
        ORDER BY g.id
      ) FILTER (WHERE g.id IS NOT NULL),
      '[]'::jsonb
    ) AS groups
FROM users AS u
LEFT JOIN user_groups AS ug ON ug.user_id = u.id
LEFT JOIN groups AS g ON g.id = ug.group_id
WHERE u.id = :user_id
GROUP BY u.id, u.name
```

That Query is visibly PostgreSQL-specific and may decode `groups` through PostgreSQL JSONB support or
an explicit JSON decode. The common package does not pretend the SQL is portable.

### 7.6 Multiple explicit Queries remain possible

The named Query contract guarantees one statement; it does not ban application code from explicitly
calling two named Queries when measurements show that is better.

What is forbidden is hidden additional SQL. If two Queries execute, two calls must appear in source
and tests can count two executions.

---

## 8. Shaping contract

### 8.1 Canonical shaping is a Pure exclusive-state transition

The canonical Query shaper is a Pure row-to-state function:

```text
step(
  borrow mut state: State,
  borrow mut output_builder: array_builder<Item>,  // zero or more separate builders
  row: Row,
  out: region,
) -> Result<(), db.Error>
```

A shaper with no collection omits the builder parameter. Every builder remains a separate caller
local; it is never hidden inside `State`.

It does not receive:

- `db.rows`;
- `db.conn`;
- `db.tx`;
- `db.exec`;
- a prepared statement;
- a pool.

The Query-local `run` function is Impure: it creates exactly one `db.rows` execution, advances it in
one visible Align `loop`, and passes each row to the step before the next advance. The step must have
inferred effect `Pure`. Connection construction, Query execution, metadata calls, and all other
database I/O are Impure, so the existing effect checker structurally rejects them from the step's
transitive call graph. Mutation rooted in the explicit exclusive `borrow mut state`, builder
operations on that state, and explicit region allocation remain Pure. The state is not copied or
moved into the call; it stays owned by `run`, which also avoids the existing ban on passing an
arena-owned Move value by value.

There is deliberately no v1 `db.fold` higher-order helper. A normal `loop` is already Align's one
sequential-control form, keeps row advancement visible, and avoids either a database-special
callback rule or widening the closed minimal-generics/function-value surface. A later general
stream-fold primitive would require an independent non-database consumer and the same visible
cost/ownership contract.

The low-level `db.rows` API is also the mechanism used by this Query-local loop. Arbitrary code may
stream rows, but only the Pure step is called the conventional Query shaper. If the Impure
orchestrator executes another Query, the second call is visible and its execution-count test
expects two; a named Query helper that promises one execution fails its acceptance test.

### 8.2 One-pass by default

A step receives each `Row` once. It may:

- validate repeated parent values;
- map a row;
- fold an aggregate;
- append to explicit builders;
- group adjacent ordered rows;
- decode a native aggregate value;
- construct a segmented output.

It MUST NOT silently:

- rewind a row stream;
- materialize all rows;
- sort;
- create a hash table;
- deduplicate;
- issue SQL.

Any such operation must be visible as ordinary Align code and allocation.

### 8.3 Ordering is a Query contract

Adjacent grouping is correct only when SQL provides a deterministic ordering. The Query module should
record the assumption in a comment/test, and the SQL must include the required `ORDER BY`.

The package does not inspect a query plan and claim an order not expressed by SQL.

### 8.4 Duplicate handling is explicit

A join may intentionally duplicate children. The shaper does not automatically deduplicate by child
key. The SQL author may use `DISTINCT`, pre-aggregate, or the shaper may visibly track/deduplicate when
that cost is desired.

### 8.5 Output is ordinary Align data

Once shaping completes, `Output` has no connection to the database runtime. It is a normal Align
value governed by ordinary ownership, regions, moves, and drops.

---

## 9. Memory and ownership

### 9.1 No GC, no per-row reflection

Typed decoding uses a precomputed field/column plan. The hot path does not:

- look fields up by string per row;
- allocate a map;
- box every value;
- create reflection objects;
- allocate one heap object per row.

### 9.2 Row views

A streamed `Row` may contain borrowed `str`/`bytes` views into the current driver result buffer.
`next(borrow mut rows)` ends the previous resource generation. The generated decoder roots every
view-bearing field in the new generation, so an older row cannot be used after the next call unless
its data was first copied with `clone_in(out)`.

The driver must document its validity window:

```text
row-at-a-time  views valid until the next mutable borrow (`next`/`reset`/finalize)
batch          views valid while the batch lives
materialized   views/owned values valid for the destination region
```

The compiler/runtime must reject or prevent a borrowed row view from escaping its window.

### 9.3 Materialized results

`all`, `one`, `maybe_one`, and shaping that builds arrays receive the explicit `region` capability
bound by `arena name {}`. They do not silently choose the global heap.

```align
arena out {
  rows := db.all(exec, query(), params, out, [])?
  use(rows)
}
```

The returned value is tied to `out` and cannot escape the arena block. Longer-lived output requires a
visible copy into another valid owner/region according to the normal Align memory model.

### 9.4 Builder prerequisite

Compound output uses the general region builder specified in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md):

Required capability:

```text
array_builder<PlainStruct>(out)
  push(value)
  build() -> array<PlainStruct>
```

The builder grows in region chunks and performs one documented compacting element pass into the final
contiguous array. It performs no hidden heap allocation. View-bearing fields must already outlive
`out`; streamed text is copied with `clone_in(out)` before `push`. This is a shared language/core
prerequisite, not an excuse for a private database-only container.

### 9.5 Batch and SoA path

The first correct implementation may decode rows into ordinary structs. The design must preserve a
future direct path:

```text
database protocol rows
  -> typed column buffers
  -> soa<Row>
  -> normal Align pipeline
```

Direct `soa<T>` decode must not build an intermediate `array<T>` and transpose it.

Nullable primitive columns SHOULD use a values buffer plus a validity bitmap internally rather than
one tagged `Option` object per lane, while preserving `Option<T>` semantics at the language surface.

---

## 10. SQL types

### 10.1 Common logical types

The first release uses bool, signed integers, floats, UTF-8 text, bytes, and nullable `Option<T>`.
The common package adds temporal, decimal, UUID, JSON, array, range, or domain types only after an
exact logical representation and both driver mappings are settled in the owning D12–D14 consumer
decision. No lossy implicit conversion or placeholder wrapper is permitted.

### 10.2 SQLite mapping

Initial SQLite mapping:

```text
INTEGER         signed integer widths, checked on decode
REAL            f64 (f32 only by explicit expected conversion policy)
TEXT            Params: str/string; Row: str
BLOB            Params: slice<u8>/array<u8>; Row: slice<u8>
NULL            Option<T>
```

SQLite is dynamically typed. A typed Query therefore requires a stable declared/checked result type.
When SQLite cannot establish one, the SQL author must use an explicit `CAST`, a schema declaration, or
a Query-native type option. The package must not guess from the first row.

SQLite `STRICT` tables and declared column affinities should strengthen checking when available.

### 10.3 PostgreSQL mapping

The exact common first-release PostgreSQL mapping is:

```text
int2/int4/int8          i16/i32/i64
float4/float8           f32/f64
bool                    bool
text/varchar/name       Params: str/string; Row: str
bytea                   Params: slice<u8>/array<u8>; Row: slice<u8>
NULL                    Option<T>
```

`date`/`time`/`timestamp`, `numeric`, UUID, JSON/JSONB, arrays, ranges, domains, and user-defined
types are not silently coerced into this set. Their exact logical/native representations are owned
by D12–D14 consumer decisions and become explicit driver-qualified mappings before use.
Parameter/result OIDs are retained in checked metadata. An unavailable type requires such a visible
native mapping or an explicit dynamic value.

### 10.4 NULL

SQL NULL maps only to `Option<T>`.

A nullable result into non-`Option<T>` is a check/decode error. A non-null result into `Option<T>` may
be allowed only if the policy is explicit and one-way; v1 should prefer exact nullability matching.

---

## 11. SQLite driver

### 11.1 Connection

```align
conn := sqlite.connect("app.db", [
  sqlite.ConnectOption.OpenReadWrite,
  sqlite.ConnectOption.Create,
  sqlite.ConnectOption.BusyTimeoutNs(5_000_000_000),
  sqlite.ConnectOption.Pragma("journal_mode", "WAL"),
  sqlite.ConnectOption.Pragma("foreign_keys", "ON"),
])?
```

The native design must retain explicit extension points for:

- open flags;
- URI mode;
- busy timeout/handler policy;
- arbitrary explicit PRAGMA where safely representable;
- shared/private cache flags where supported;
- thread/open mode;
- extension loading policy (disabled by default; explicit if ever supported).

The first-release finite connection sum is the exact subset in §11.2. Busy-handler callbacks and
extension loading require the later proved callback work and are not constructors in v1. No
requested option may be silently ignored.

### 11.2 Query/prepare/execute options

The first-release SQLite sums are fixed:

```text
sqlite.QueryOption.RequireVersionAtLeast(major, minor, patch)
sqlite.CommandOption.RequireVersionAtLeast(major, minor, patch)

sqlite.PrepareOption.Persistent
sqlite.PrepareOption.Normalize

sqlite.ExecuteOption.BusyTimeoutNs(ns)

sqlite.TxOption.Deferred
sqlite.TxOption.Immediate
sqlite.TxOption.Exclusive

sqlite.MetaOption.IncludeInternalObjects
sqlite.MetaOption.IncludeHiddenColumns

sqlite.ExplainOption.QueryPlan
sqlite.ExplainOption.Bytecode
```

`major`, `minor`, and `patch` are `u32`.

`RequireVersionAtLeast` is static, pins the descriptor to SQLite, and participates in its public
semantic contract and artifact. `Persistent`/`Normalize` apply only to preparation.
`BusyTimeoutNs` temporarily replaces the connection busy timeout for that execution and restores it
when that execution actually ends; it conflicts with another busy-timeout value. For
`execute`/`one`/`maybe_one`/`all`, restoration precedes return. For `rows`/`rows_stmt`, the rows
resource retains the override and the connection's package-tracked prior value, then restores it on
exhaustion, terminal step error, or Drop before releasing the connection/statement dependency.
Restoration failure poisons/closes the connection rather than returning it with unknown policy.
SQLite v1 also gives every native connection one package-tracked active-execution lease.
Synchronous execution holds it until return; `rows`/`rows_stmt` retains it until
exhaustion/error/Drop. Except for `next`/Drop through the resource that owns the lease, every second
operation that would reach that native
connection—whether or not it requests a timeout—is rejected before binding, changing timeout state,
or making a native call. The error is
`db.Error.Unsupported(db.ContractError { query_id, item:
"sqlite.connection.active_execution", message:
"SQLite connection already has an active execution" })`; `query_id` is the attempted
Query/command ID or `None` for a Query-less operation. This deliberately restricted v1 rule prevents
connection-global timeout state and active statements from overlapping through Copy `db.exec`
views. A failed second attempt never reads/restores the first lease's saved value. Tests pin
ordinary/timeout stream overlap in both directions, a failed second override followed by first-stream
Drop, exhaustion/error cleanup, and restore failure poisoning.
Exactly zero or one transaction mode is accepted, with `Deferred` as the `[]` default.
`IncludeInternalObjects` and
`IncludeHiddenColumns` apply only to metadata categories for which SQLite exposes those objects.
Exactly one EXPLAIN mode is accepted; `[]` means `QueryPlan`.

The first-release connection sum is also finite:

```text
sqlite.ConnectOption.OpenReadOnly
sqlite.ConnectOption.OpenReadWrite
sqlite.ConnectOption.Create
sqlite.ConnectOption.Uri
sqlite.ConnectOption.PrivateCache
sqlite.ConnectOption.SharedCache
sqlite.ConnectOption.NoMutex
sqlite.ConnectOption.FullMutex
sqlite.ConnectOption.BusyTimeoutNs(ns)
sqlite.ConnectOption.Pragma(name, value)
```

`[]` means `OpenReadWrite` without create, URI, cache, mutex override, busy override, or PRAGMA.
The database path must contain no U+0000. A `Pragma` name must match ASCII
`[A-Za-z_][A-Za-z0-9_]*`; its UTF-8 value must contain no U+0000 and is passed with deterministic
SQLite string-literal quoting, never pasted as raw SQL. Invalid names/values fail before open/setup,
and an unsupported PRAGMA returns an error rather than being ignored.
Read-only conflicts with read-write/create; private conflicts with shared; no-mutex conflicts with
full-mutex; a duplicate PRAGMA name or busy timeout conflicts. Durations must be positive. A
compile-time or linked-library capability miss returns `Unsupported`; no variant degrades to a
different libsqlite3 flag. Extension loading is not exposed in v1 and remains disabled.

### 11.3 Native features

The design must not block:

- `RETURNING`;
- `STRICT` tables;
- `WITHOUT ROWID`;
- JSON1;
- FTS virtual tables;
- attached databases;
- backup API;
- incremental blob I/O;
- custom functions/collations when a safe callback model exists.

Only connection, typed Query, transaction, migration, and metadata essentials are required in the
first release. Others are roadmap items.

---

## 12. PostgreSQL driver

### 12.1 Connection

```align
conn := postgres.connect(url, [
  postgres.ConnectOption.ApplicationName("align-service"),
  postgres.ConnectOption.ConnectTimeoutNs(5_000_000_000),
  postgres.ConnectOption.Parameter("options", "-c statement_timeout=5000"),
])?
```

The driver must allow native libpq/connection parameters rather than reducing them to a fixed common
config struct.

Required categories:

- host/port/database/user;
- application name;
- connect timeout;
- TLS/SSL mode and related parameters;
- target session attributes;
- arbitrary supported connection key/value parameters;
- notice/error detail access.

Secrets remain runtime values and never enter static Query metadata.

### 12.2 Parameter and result formats

The first-release PostgreSQL sums are fixed:

```text
postgres.QueryOption.ParameterType(name, canonical_type_name)
postgres.CommandOption.ParameterType(name, canonical_type_name)

postgres.PrepareOption.ParameterOid(name, oid)

postgres.ExecuteOption.ParameterFormat(name, Text|Binary)
postgres.ExecuteOption.ResultFormat(Text|Binary)

postgres.TxOption.Isolation(ReadCommitted|RepeatableRead|Serializable)
postgres.TxOption.Access(ReadOnly|ReadWrite)
postgres.TxOption.Deferrable(bool)

postgres.MetaOption.SearchPathOnly
postgres.MetaOption.IncludeSystemCatalogs

postgres.ExplainOption.Analyze
postgres.ExplainOption.Format(Text|Json)
postgres.ExplainOption.Verbose(bool)
postgres.ExplainOption.Costs(bool)
postgres.ExplainOption.Buffers(bool)
postgres.ExplainOption.Timing(bool)
postgres.ExplainOption.Settings(bool)
postgres.ExplainOption.Wal(bool)
```

`name` and `canonical_type_name` are `str` literals.

`ParameterType` is static, pins the descriptor to PostgreSQL, names a Params field, and participates
in its public semantic contract and artifact. `ParameterOid` applies only to preparation.
`ParameterFormat` names a Params field; `ResultFormat` applies to the complete result in libpq v1.
Unknown fields/types/OIDs, duplicate field controls, and conflicting formats are errors, not ignored
hints. The first implementation may support only `Text`; requesting an unavailable binary mapping
returns `Unsupported` before sending SQL. Text-format `bytea` uses the exact hex encoding from
§5.6.1; Binary-format `bytea` alone passes raw bytes with an explicit libpq length.

The first-release connection sum is:

```text
postgres.ConnectOption.ApplicationName(value)
postgres.ConnectOption.ConnectTimeoutNs(ns)
postgres.ConnectOption.SslMode(Disable|Prefer|Require|VerifyCa|VerifyFull)
postgres.ConnectOption.TargetSessionAttrs(Any|ReadWrite|ReadOnly|Primary|Standby)
postgres.ConnectOption.Parameter(name, value)
```

The URL supplies host, port, database, user, and password. `[]` adds no libpq keyword override.
Duplicate semantic keys, including a URL/option conflict, are errors; secrets are runtime values and
never enter Query artifacts. Durations must be positive. The URL, application name, and arbitrary
parameter name/value must be valid UTF-8 without U+0000; the wrapper rejects an embedded NUL as
`db.Error.Encode` with no Query identity before calling libpq, never truncates it.

The package owns the connection's client encoding and fixes it to UTF-8. `client_encoding` is a
reserved semantic key: an explicit URL/keyword occurrence or
`ConnectOption.Parameter("client_encoding", ...)` conflicts before opening. For the `options`
connection parameter, the package tokenizes the documented backslash-escaped, space-separated
startup-option grammar and rejects ASCII-case-insensitive `client_encoding` assignments in
`-c name=value`, `-cname=value`, or `--name=value` form; `-` and `_` are equivalent while comparing
the long name. A trailing escape or a `-c` without its assignment is an Encode error before open.
After the expanded URL entry and every accepted user override, the package appends exact
`client_encoding=UTF8` to `PQconnectdbParams`, so `PGCLIENTENCODING` cannot supply ambient behavior.
For a non-null `PGconn`, the package first checks `PQstatus`. Any status other than `CONNECTION_OK`
copies the native connection error and closes without calling `PQclientEncoding`. Only after
`CONNECTION_OK` does it require `PQclientEncoding(conn) == PG_UTF8`. A mismatch, including `-1`,
always closes the connection and returns
`db.Error.Unsupported(db.ContractError { query_id: None,
item: "postgres.connection.client_encoding",
message: "PostgreSQL client encoding is not UTF-8" })`; this branch does not depend on libpq having
populated an error message. No SQL execution is permitted before this post-connect invariant holds.

`[]` transaction options mean `ReadCommitted`, `ReadWrite`, and non-deferrable. Each transaction
dimension may occur at most once. PostgreSQL-invalid combinations such as deferrable without
serializable read-only are rejected before `BEGIN`. `SearchPathOnly` conflicts with
`IncludeSystemCatalogs`. EXPLAIN defaults to text with PostgreSQL defaults; `Buffers`, `Timing`, or
`Wal` requires native `Analyze` and otherwise conflicts before execution.

### 12.3 Native features

The design must not block:

- PostgreSQL arrays;
- UUID;
- JSONB;
- enums and domains;
- ranges;
- composite types when explicitly mapped;
- `COPY`;
- pipeline mode;
- single-row mode;
- `LISTEN`/`NOTIFY`;
- server-side prepared statements;
- `LATERAL`;
- `DISTINCT ON`;
- custom operators/extensions;
- detailed `EXPLAIN` formats.

The initial public release requires the common Query/transaction path, native options, checked
metadata, and basic plan access. COPY/pipeline/notify are scheduled in D13.

---

## 13. Native options and portability

### 13.1 Option scopes

Options are scoped, not one untyped bag:

```text
db.QueryOption             static common Query semantics
db.CommandOption           static common command semantics
db.PrepareOption           common prepare controls
db.ExecuteOption           common one-execution controls
db.TxOption                common transaction semantics
db.MetaOption              common metadata categories
db.ExplainOption           common plan controls

sqlite.ConnectOption       SQLite connection controls
sqlite.QueryOption         SQLite static Query controls
sqlite.CommandOption       SQLite static command controls
sqlite.PrepareOption       SQLite prepare controls
sqlite.ExecuteOption       SQLite execution controls
sqlite.TxOption            SQLite transaction controls
sqlite.MetaOption          SQLite metadata controls
sqlite.ExplainOption       SQLite plan controls

postgres.*Option           corresponding PostgreSQL-native scopes
```

A driver option passed at the wrong scope is a compile-time type error.

Common operations accept only common options. A native option selects a driver-qualified operation:

```align
db.execute(exec, command, params, common_options)
sqlite.execute_native(exec, command, params, common_options, sqlite_options)
postgres.execute_native(exec, command, params, common_options, postgres_options)
```

This avoids an untyped option bag, a circular dependency from `pkg.db` to every driver, and a public
trait. Driver-qualified static Query options pin the Query descriptor to that driver and participate
in its public semantic artifact. Runtime connection/prepare/execution options never change the
Query's static identity.

### 13.2 Option value representation

Every `*Option` above is a finite public sum type. Operation arguments are
`slice<ThatScopeOption>`; `[]` explicitly means no options. Option payloads are Copy scalars,
`str` views consumed during the call, or static type/Query identities. A driver must copy any
runtime string it retains after the call. An option list is not stored as an untyped map and is
never inspected through reflection.

The common first-release variants and defaults are exact:

```text
db.QueryOption.Check(DeclaredOnly|CheckedOptional|CheckedRequired)
db.CommandOption.Check(DeclaredOnly|CheckedOptional|CheckedRequired)
db.PrepareOption.TimeoutNs(ns)
db.ExecuteOption.TimeoutNs(ns)
db.TxOption.BeginTimeoutNs(ns)
db.MetaOption.TimeoutNs(ns)
db.MetaOption.IncludeSystem
db.ExplainOption.TimeoutNs(ns)
```

`[]` selects `DeclaredOnly` for a static Query/command, no client deadline for a runtime operation,
and excludes system metadata. Common `db.explain` is inspection-only; only the driver-qualified
PostgreSQL native option `Analyze` executes the named Query. A duration must be positive. A scope
accepts at most one timeout and at most one check policy. Duplicate tags, two check policies, or a
driver-native option that conflicts with a common option return `db.Error.Unsupported` or
`db.Error.InvalidQuery` as appropriate before SQL is sent. Native `Analyze` visibly changes EXPLAIN
from an inspection into one execution of the named Query; execution-count instrumentation includes
it. SQLite has no v1 ANALYZE option and rejects any attempt to reinterpret a PostgreSQL option.

These are the mandatory minimums, not examples. Adding a later variant is an ordinary source/API
change to the owning finite sum and its disposition matrix. It cannot be implemented as an unknown
tag or silently ignored extension.

Static Query/command option arguments are more restricted: a recognized constructor accepts only a
fixed literal list of option constructors whose payloads are literals, type identities, or other
compiler-known constants. A runtime local, environment read, FFI result, or arbitrary function call
in a static option is a compile-time error. The canonical option tag/payload encoding participates
in `StaticQueryArtifact`/`StaticCommandArtifact`; driver-pinning options also participate in
`IStaticQuery`/`IStaticCommand`.

Every primitive operation has one option-bearing form:

```text
connection   driver.connect(input, slice<driver.ConnectOption>)
Query        db.query_file(slice<db.QueryOption>)
             driver.query_file(slice<db.QueryOption>, slice<driver.QueryOption>)
command      db.command_file(slice<db.CommandOption>)
             driver.command_file(slice<db.CommandOption>, slice<driver.CommandOption>)
prepare      db.prepare(exec, query, slice<db.PrepareOption>)
execution    db.execute/one/maybe_one/all/rows(..., slice<db.ExecuteOption>)
transaction  db.begin(conn, slice<db.TxOption>)
metadata     db.meta_database/meta_schemas/...(..., out: region, slice<db.MetaOption>)
EXPLAIN      db.explain(exec, query, params, out: region, slice<db.ExplainOption>)
```

The corresponding `driver.*_native` form receives the common option slice and one additional
driver option slice. It does not reinterpret a driver option as a common option. There are no
default arguments, optionless overloads, fluent option builders, string-key option maps, or
process-global option state. Query-local `run` helpers decide visibly whether to accept an option
slice from their caller or pass `[]`.

Connection, Query, prepare, execution, transaction, metadata, and EXPLAIN therefore have distinct
static types even when two variants happen to carry the same scalar. Their implementation ownership
is deliberately earlier than D9:

```text
D1  common + driver Query/Command option sums and artifact encoding
D2  SQLite connection and baseline execution variants
D4  PostgreSQL connection and baseline execution variants
D6  prepare variants
D7  transaction variants
D9  common deadline enforcement/native cancellation cleanup and the cross-scope disposition matrix
D12 metadata and EXPLAIN variants
```

D9 completes deadline enforcement/native cancellation cleanup and the combined precedence matrix; it does not create
preliminary option APIs that D1/D2/D4/D6/D7 already require, and it must not invent another
representation or a v1 public cancel resource.

### 13.3 No silent ignore

Every option has one of these outcomes:

```text
applied
rejected as unsupported
rejected as conflicting
```

“Ignored for portability” is forbidden.

### 13.4 Precedence

Validation order is observable and exact. Connection formation performs these phases:

1. validate and parse the explicit connection input;
2. visit options in source order, validating the current payload and native conversion before
   registering its tag/semantic key; a duplicate or conflict is reported at its second occurrence;
3. validate cross-option/capability constraints after the complete list; and
4. open the native connection and establish post-connect invariants.

The first failing phase wins. Within phase 2 the first failing source-order option wins, regardless
of whether a later option has a different error. A URL supplies its semantic keys before the first
PostgreSQL option, so the first colliding option is the conflict owner.

Query/command execution performs these phases before native work:

1. validate the execution-header pointer, ABI, reserved bytes, kind, mask/slot/thunk agreement,
   `descriptor_id` view, and Q1-plan pointer without acquiring state; only after the complete phase
   succeeds is descriptor identity trusted;
2. validate common options in source order, payload before duplicate detection;
3. validate driver-native options by the same rule;
4. validate driver restriction;
5. validate the `db.exec` discriminator, generation, open/poisoned state;
6. invoke the generated static-option validator; and
7. acquire the driver execution lease.

An invalid phase-1 descriptor returns
`db.Error.InvalidQuery(db.ContractError { query_id: None, item: "db.descriptor.header",
message: "invalid static database descriptor" })`; no untrusted identity bytes enter the error.
Q2 therefore reports malformed descriptor before timeout/mismatch/closed state, unsupported common
timeout before mismatch/closed state, mismatch before closed/static-option/overlap errors, and a
closed state before a static-option failure. Bind/native work begins only after phase 7. The existing
cleanup rule is separate: the first operation error survives later cleanup errors, while a cleanup
error replaces success.

Option override semantics remain:

- static Query options describe Query semantics and cannot be overridden incompatibly;
- prepare options affect statement preparation only;
- execution options affect one execution only;
- connection defaults are fallback values only for explicitly overrideable properties;
- duplicate/conflicting non-overrideable options are errors.

### 13.5 Driver restriction

A Query/command descriptor records the public finite sum `db.DriverRestriction`:

```text
AnySupportedDriver
SQLiteOnly
PostgreSQLOnly
```

`db.query_file([])` starts portable in API terms, though its SQL may still fail driver preparation.
`sqlite.query_file([], [])` and `postgres.query_file([], [])` explicitly pin the descriptor. Native
options also pin it.

Executing a pinned Query with the wrong driver fails before SQL is sent (`DriverMismatch`).

Portability means “same common Align interface and successfully prepared SQL,” not automatic dialect
translation.

---

## 14. Transactions

### 14.1 Explicit lifecycle

```align
tx := db.begin(conn, common_tx_options)?

result := update_account.run(db.exec_tx(tx), params, out)?

conn := db.commit(tx)?
```

Rollback is explicit:

```align
conn := db.rollback(tx)?
```

`begin` consumes `conn`; the caller cannot use the connection while the transaction is active.
`commit` and `rollback` consume `tx` and return the connection on success. A failed explicit end or
Drop performs best-effort rollback and closes the owned connection; it never returns a connection in
an unknown transaction state and MUST NOT commit.

### 14.2 Native transaction options

SQLite must expose relevant begin/locking modes:

```text
DEFERRED
IMMEDIATE
EXCLUSIVE
```

PostgreSQL must expose:

```text
isolation level
read only/read write
deferrable
```

Native transaction options remain driver-qualified. Unsupported combinations fail before starting the
transaction.

### 14.3 Nested transactions

Version 1 need not present a portable nested-transaction abstraction. Savepoints may be explicit
native APIs. The common package must not claim nested transaction semantics that differ between
drivers.

---

## 15. Errors

Database operations return a structured database error rather than aborting for recoverable
failures. The v1 payloads are:

```align
db.NativeError {
  driver: db.Driver,
  code: Option<string>,
  extended_code: Option<i64>,
  sqlstate: Option<string>,
  message: string,
  detail: Option<string>,
  constraint: Option<string>,
  table: Option<string>,
  column: Option<string>,
}

db.CardinalityError {
  expected_min: i64,
  expected_max: i64,
  observed_at_least: i64,
}

db.ContractError {
  query_id: Option<string>,
  item: string,
  message: string,
}

db.Error {
  Connection(db.NativeError),
  Timeout(db.NativeError),
  Cancelled(db.NativeError),
  NotFound,
  Cardinality(db.CardinalityError),
  Constraint(db.NativeError),
  Serialization(db.NativeError),
  Deadlock(db.NativeError),
  SchemaMismatch(db.ContractError),
  DriverMismatch(db.ContractError),
  Decode(db.ContractError),
  Encode(db.ContractError),
  InvalidQuery(db.ContractError),
  Unsupported(db.ContractError),
  Native(db.NativeError),
}
```

Every native string is copied into owned Align storage before the driver result/error buffer is
released. Errors are therefore Move values and never borrow a connection, statement, or row buffer.
Allocating detailed strings on the error path is accepted and measured separately from the success
hot path.

This surface depends on recursive tagged Move payloads: `Option<string>` fields use conditional
Drop, `db.Error` variants recursively own `db.NativeError`/`db.ContractError`, and
`Result<T,db.Error>` moves/drops the active payload through `?` and `match`. The `Ok` path allocates
no error storage. Numeric-only errors, empty strings as absence, or an opaque heap error are not
acceptable substitutes.

The semantics are:

- stable categories for ordinary control flow;
- `ContractError.query_id` is `Some(id)` whenever validation has a Query/command subject, including
  `meta_query` and Query EXPLAIN. It is `None` only when no Query/command subject exists, such as
  connection input, transaction-option, or category-metadata validation, and when a malformed
  descriptor header fails before its identity becomes trusted. `item` names the exact operation and
  input in both cases;
- driver code/message/detail retained where available;
- SQLSTATE retained for PostgreSQL;
- primary/extended result codes retained for SQLite;
- constraint/table/column names retained when the database provides them;
- no string parsing required to detect a stable category;
- programmer contract violations may abort only where ordinary Align APIs would abort (for example,
  invalid slice bounds), not for database-reported failures.

Transparent retry is not part of the package. A caller may visibly match `Serialization`/`Deadlock`
and retry.

---

## 16. Offline checking and database preparation

### 16.1 Three check levels

A Query or command has one observed verification state per permitted driver and one static policy:

```text
driver state
DriverVerification {
  driver
  state: Declared | DatabaseChecked
  metadata_fingerprint: Option<hash>
}

Declared
  static SQL source exists; Params/Row are valid Align types; hash is known

DatabaseChecked
  the selected database engine prepared/described the SQL and emitted metadata

policy
DeclaredOnly
CheckedOptional
CheckedRequired
```

The descriptor's driver restriction determines the required state set:

```text
SQLiteOnly          {SQLite}
PostgreSQLOnly      {PostgreSQL}
AnySupportedDriver  {SQLite, PostgreSQL}
```

`CheckedRequired` makes missing or stale metadata a compile error for **every** member of that set.
Preparing only SQLite metadata for an `AnySupportedDriver` descriptor does not satisfy the policy;
the developer must also prepare PostgreSQL or explicitly pin the descriptor to SQLite.
`CheckedOptional` uses exact current metadata independently per driver and leaves only the missing or
stale driver honestly `Declared`. Query inspection reports the complete map and runtime inspection
reports the selected connection entry. Neither the package nor compiler may collapse a mixed map to
one “checked” boolean or describe a Declared driver as database-type-checked.

### 16.2 Explicit preparation command

```sh
alignc db prepare app.align --driver sqlite --database dev.sqlite --schema-id dev-v1
alignc db prepare app.align --driver sqlite --memory --migrations db/migrations
alignc db prepare app.align --driver postgres --url-env ALIGN_DB_PREPARE_URL --schema-id dev-v1
alignc db prepare app.align --driver postgres --url-env ALIGN_DB_PREPARE_URL --schema-id dev-v1 --check
```

The entry `.align` path and one `--driver` are required in v1. Query discovery uses exactly the
entry's reachable import graph; it never scans the project for Query `.sql` files. The explicit
SQLite `--migrations <dir>` catalog is a separate tool input governed by §16.6. `--query
<fully-qualified-id>` may repeat to restrict the Query set. The output root is the entry build's
project root plus `.align-db/`. A later tool-only config file may shorten these flags, but it is not
a compiler manifest and is not required by the first implementation.

SQLite accepts exactly one schema environment: `--database <path> --schema-id <id>`, or `--memory`
optionally initialized from the canonical migration sequence in `--migrations <dir>` (§16.6).
PostgreSQL accepts `--url-env <name> --schema-id <id>`; the environment variable's value is read only
by this explicit command. Its URL must explicitly contain a non-empty user and password, exactly one
host, a nonzero port, and exactly one database. Target overrides, service expansion,
`client_encoding`, and startup `options` are rejected; the tool appends package-owned UTF-8 through
`PQconnectdbParams` and supplies an explicit empty startup-option sequence, so libpq cannot select a
target, encoding, or SQL setting from ambient `PG*` defaults.
`schema-id` is a non-empty, non-secret UTF-8 release/schema identity chosen
by the caller and must contain no U+0000. It is required for a mutable database target because
normal offline builds cannot rediscover that target's schema. The migration-backed memory form
derives its identity and forbids `--schema-id`. Direct URL flags may exist for local use but must be
warned as shell-history-visible. The command prints the selected driver, schema source/identity,
server/library version, and Query count before writing.

This is the only workflow allowed to contact a database. It:

1. reads explicit command flags and optional tool-only preparation configuration;
2. applies/selects the intended schema environment;
3. asks SQLite or PostgreSQL to prepare/describe every selected static Query;
4. records parameter/result/native type information and nullability where available;
5. records driver/version/schema identity;
6. records the source SQL hash, selected driver wire SQL hash/rewrite version, and relevant option
   hash;
7. writes deterministic repository metadata;
8. supports `--check`, which fails when regeneration would change repository metadata.

One preparation batch observes one schema snapshot. SQLite begins a read transaction and touches
`sqlite_schema` after any migration transaction; PostgreSQL begins a read-only repeatable-read
transaction before capturing environment state. The snapshot remains open through every selected
prepare/describe and closes with the preparation connection.

Preparation compiles the reachable units in an explicit regeneration mode that still enforces every
language/Query contract but temporarily treats missing/stale checked metadata as the artifact to
regenerate. A normal `build`/`check` never enters that mode. `--check` performs the same describe
work, compares canonical bytes, writes nothing, and exits nonzero on missing/stale/different output.

Connection URLs and secrets come from explicit command arguments or environment variables used by
this command only. They are not compiler build inputs and are never written. A normal build reads
only the selected metadata artifact and never contacts the database.

### 16.3 Metadata location

For each descriptor/driver pair, the path is exact:

```text
.align-db/
  .publication.lock
  sqlite/
    <descriptor-id-hash>.json
  postgres/
    <descriptor-id-hash>.json
```

`.publication.lock` is an empty implementation-owned cross-process lock, not a build input or an
artifact identity. Normal compilation holds a shared OS lock across its complete checked-metadata
snapshot; preparation holds the exclusive lock across comparison, staging, replacement, and
rollback. The file remains after first publication so a process exit releases synchronization
without stale-lock recovery.

`descriptor-id-hash` is `Hash128::of(descriptor_id.as_bytes()).to_hex()`. `descriptor_id` is the
Query's `query_id` or command's `command_id`. The driver directory is exactly
`sqlite` or `postgres`; the filename uses the 32 lowercase hexadecimal characters plus `.json`.
The compiler derives this path from the descriptor and driver without scanning `.align-db`. A file
whose internal Query ID or driver disagrees with its derived path is invalid; a hash collision is a
diagnostic, never aliasing.

Version 1 uses this canonical UTF-8 JSON codec:

- the file is one JSON object on one line followed by exactly one LF;
- there is no other whitespace and no byte-order mark;
- object keys appear in the exact orders below; duplicate, unknown, missing, or out-of-order keys
  are invalid;
- strings emit `\"`, `\\`, `\b`, `\t`, `\n`, `\f`, and `\r`; every other U+0000–U+001F scalar
  uses lowercase `\u00xx`; `/` is not escaped and all other Unicode is exact UTF-8, never `\u`;
- integers are shortest ASCII decimal with no leading zero or `+`; negative zero is invalid;
- an `Option` is its payload or JSON `null`; enum tags are the lowercase strings listed below;
- decoding must reproduce the exact bytes on re-encode. Merely accepting semantically equivalent
  noncanonical JSON is forbidden.

The top-level key order and value types are exact:

```text
format_version: 1
descriptor_id: string
module: string
item: string
driver: "sqlite" | "postgres"
driver_restriction: "any_supported_driver" | "sqlite_only" | "postgres_only"
statement_kind: "query" | "command"
statement_class: "select" | "dml" | "ddl" | "native" | "unknown"
source_identity: SourceIdentity
source_sql_hash: Hash128Hex
wire_sql_hash: Hash128Hex
rewrite_format_version: u32
static_options_hash: Hash128Hex
params_fingerprint: Hash128Hex
row_fingerprint: Hash128Hex | null
schema_fingerprint: Hash128Hex
engine_version: string
driver_version: string
search_path: array<string>
extensions: array<Extension>
parameters: array<Parameter>
columns: array<Column>

SourceIdentity File key order:
  kind: "file"
  logical_path: string

SourceIdentity Inline key order:
  kind: "inline"
  descriptor_id: string

Extension key order:
  schema: string
  name: string
  version: string | null

Parameter key order:
  source_name: string
  protocol_ordinal: u32
  logical_type: string
  native_type: string | null
  native_type_id: i64 | null

Column key order:
  ordinal: u32
  source_alias: string
  logical_type: string
  native_type: string | null
  native_type_id: i64 | null
  nullable: "yes" | "no" | "unknown"
  origin_schema: string | null
  origin_table: string | null
  origin_column: string | null
```

`Hash128Hex` is exactly `Hash128::to_hex()` (`lo`, then `hi`), 32 lowercase hexadecimal
characters. `module`, `item`, and `descriptor_id` obey L5's exact identity rule. `row_fingerprint` is
non-null exactly for Query. Parameters use one-based protocol ordinal order; columns use zero-based
decoder order. `search_path` preserves semantic lookup order. `extensions` sorts by the complete
UTF-8-byte tuple `(schema, name, version Option tag/bytes)`. SQLite requires empty `search_path` and
`extensions`; PostgreSQL records both explicitly, including empty arrays.
`logical_type` is the formatter's canonical fully-qualified Align spelling of that field's
substituted `CanonicalType` root; aliases are resolved and no source-layout spelling enters it.

`source_sql_hash`, `wire_sql_hash`, `static_options_hash`, and structural Params/Row fingerprints
and `driver_restriction` must match the L5 artifact inputs; the selected driver must be permitted.
`schema_fingerprint` is the exact preparation schema identity;
it is the §16.6 `ALIGNSID` stream digest, not an engine-dependent JSON/object hash. PostgreSQL binds
the explicit schema ID plus engine-reported search path and extension assumptions. The
engine-provided parameter/result order must match the declared binder/decoder plan. Commands require
`columns = []`.
`static_options_hash` is `Hash128::of` over the complete encoded
`sequence<StaticOption>` field, including its u32 count prefix.

The complete file bytes have `metadata_fingerprint = Hash128::of(bytes)`. The runtime QueryMeta
identities are derived, not self-referential JSON fields:

```text
schema_identity  = schema_fingerprint

server stream:
  magic "ALIGNSRV", u32 version 1, Driver tag,
  engine_version string, driver_version string,
  search_path sequence<string>,
  extensions sequence<{ schema: string, name: string, version: Option<string> }>
server_identity = Hash128::of(server stream).to_hex()

prepare stream:
  magic "ALIGNPRP", u32 version 1, descriptor_id string, Driver tag,
  metadata_fingerprint Hash128, server_identity Hash128
prepare_identity = Hash128::of(prepare stream).to_hex()
```

Binary stream integers, strings, sequences, Options, and Hash128 values use L5 §6.2's codec.
`CheckedMetadata.metadata_digest` is `metadata_fingerprint` and
`metadata_format_version = 1`. The compiler copies the derived identities and ordered native
parameter/column evidence into the producer-owned QueryMeta plan; runtime code never opens this
file.

Native PostgreSQL OIDs may be recorded as environment evidence, but volatile OIDs are not the sole
canonical type or schema identity. Canonical type names and schema-qualified identities are stored
alongside them.

The reader rejects invalid UTF-8/JSON, noncanonical escaping/number/key order, wrong field types,
unknown tags, non-dense ordinals, count/name mismatches, invalid hashes, source/artifact mismatch,
and trailing bytes without panicking. A malformed selected file is a hard diagnostic even under
`CheckedOptional`; a well-formed but stale file follows §16.4. Preparation `--check` constructs the
same semantic record and compares the complete canonical bytes without writing.

D3/D5 check in standalone-reference goldens at
`crates/align_driver/tests/golden/checked_metadata_{sqlite_query,postgres_command}_v1.json` and
sibling `.digest` files. The production writer, production reader, and a test-only reference writer
share no encoding functions and must all match the checked-in bytes/digest. The fixtures exercise
every Option state, escaping class, native ID, origin/nullability state, repeated parameter order,
and both source identities.

Secrets and connection URLs are never stored.

#### 16.3.1 Nullability and origin evidence

Nullability evidence is fail-closed and query-specific:

```text
Yes      the engine describes this exact result expression as nullable
No       the engine describes this exact result expression as non-null
Unknown  the engine supplies no authoritative query-level answer, or evidence is ambiguous
```

Catalog `NOT NULL`, a source column's declaration, or origin lookup alone never produces `No`.
Outer joins, expressions, functions, and rewrites can change result nullability. Align v1 does not
add a SQL nullability analyzer to compensate. Origin is recorded only when the engine reports an
unambiguous schema/table/column identity for that exact result entry; it is never inferred from a
name or search path.

The initial support matrix is deliberately conservative:

| Driver evidence | Origin | Nullability |
|---|---|---|
| SQLite result metadata plus optional origin APIs | record only an exact reported origin | `Unknown` unless a probed engine API gives query-level evidence |
| PostgreSQL RowDescription plus catalog lookup | record only the reported table/attribute origin | `Unknown`; catalog nullability does not describe an arbitrary result expression |

D0 records the exact API/version observations behind this matrix. D3 and D5 must check in their
driver/version matrices and tests before either checked-metadata milestone merges; this is not a
D12–D14 design decision.

`Yes` requires `Option<T>` in the exact `Row` contract. `No` requires non-`Option<T>` in v1.
`Unknown` permits either shape but proves neither: every decoded SQL NULL still becomes `None` for
`Option<T>` or a structured decode error for non-`Option<T>`. The runtime NULL guard is mandatory
for `Declared`, `DatabaseChecked`, and all three evidence states, and optimization may not remove it.
`Unknown` does not prevent `DatabaseChecked` when the exact driver type, ordinal, and other required
metadata agree.

For each static Query-or-command/driver pair, the compiler derives the one metadata pathname from the
descriptor ID hash. `StaticInputManifest` and the frontend action key record the exact logical path
plus `Missing` or `Present(content_hash, format_version)` even under `CheckedOptional`.
Creating, deleting, or editing the file therefore invalidates the producer without a directory
scan. Exact current metadata contributes to the Query artifact/implementation hash; only its public
semantic consequences contribute to the exported Query interface.

### 16.4 Stale metadata

If Query identity, SQL, Params, Row, static options, driver, metadata format, or declared schema
identity changes, existing checked metadata becomes stale.

Modes:

```text
CheckedOptional   use exact current metadata or remain Declared
CheckedRequired   compilation/CI error
```

No stale metadata may be silently treated as current.

`CheckedOptional` with missing/stale metadata emits the same honest Declared per-driver entry as if
no metadata existed; it never embeds stale evidence. `CheckedRequired` checks every permitted driver
and fails before object-cache reuse if any entry is missing or stale.
The explicit preparation regeneration mode in §16.2 is the sole exception needed to create the
missing artifact.

### 16.5 No full custom SQL engine in v1

Version 1 SHOULD NOT implement a complete PostgreSQL/SQLite parser, resolver, function catalog,
implicit-cast engine, or nullability analyzer.

The database engine is authoritative during `alignc db prepare`. Align performs only lexical work for
file validation, placeholder scanning/rewriting, statement-count screening, hashing, source maps,
diagnostics, and obvious contract errors. Driver prepare is still authoritative for statement count
and SQL validity.

Database-checked metadata is evidence for one recorded schema environment, not a proof that runtime
data cannot differ. Execution setup still checks result count/name/native types, and per-row decoding
still rejects NULL or SQLite storage-class mismatches.

This keeps implementation thin and database behavior accurate while preserving offline normal builds.

### 16.6 SQLite preparation environment

SQLite preparation may use:

- a configured database file;
- a temporary database created by applying migrations/schema;
- an in-memory database when the schema is reproducible there.

The chosen source is explicit in command output. `--migrations <dir>` is an explicit tool action and
the only directory enumeration in this workflow. It is not performed by normal build/check or
static Query discovery. Version 1:

- enumerates only immediate entries, never recursively;
- rejects non-UTF-8 entry names and every symlink;
- selects regular files whose names match exactly
  `[0-9]{4}_[a-z][a-z0-9_]*[.]sql`;
- rejects every other regular filename ending in `.sql`; unrelated non-SQL files and directories
  are ignored;
- requires at least one selected migration when `--migrations` is present;
- parses the four digits as version `0001` through `9999`, rejects duplicate versions, and requires
  a contiguous sequence beginning at `0001`;
- sorts by numeric version ascending, independent of filesystem enumeration order;
- requires each selected file to be UTF-8 and reads exact bytes without newline normalization;
- rejects U+0000 in every selected file before applying the first migration;
- applies each whole file as a migration script in that order; the one-statement Query rule does not
  apply to migration scripts;
- encodes and fingerprints the migration catalog exactly as follows:

```text
magic "ALIGNMIG"                    # 8 bytes
format_version u32 = 1
entry_count u32
for each migration in numeric version order:
  version u32
  filename string                   # L5 u32 length + exact UTF-8 bytes
  content_hash Hash128              # Hash128::of(exact file bytes)
catalog_fingerprint = Hash128::of(complete bytes).to_hex()
```

No path outside the exact filename enters the stream. Count overflow is a pre-execution error.
`crates/align_driver/tests/golden/migration_catalog_{empty,nonempty}_v1.hex` and their `.digest`
siblings pin both cases; the non-empty fixture contains non-ASCII filename bytes and SQL without
newline normalization. A standalone reference encoder shares no production codec function.

All name/version/gap/symlink/UTF-8 errors are reported before applying the first migration.
`alignc db migrate` in D11 reuses this exact catalog rule rather than inventing another order.
The exact preparation schema identity stream is:

```text
magic "ALIGNSID", u32 version 1, Driver tag, source tag
source tag 0, SQLite memory:
  catalog_fingerprint: Option<Hash128>   # None means the explicit empty schema
source tag 1, SQLite database:
  schema_id: string
source tag 2, PostgreSQL:
  schema_id: string
  search_path: sequence<string>
  extensions: sequence<Extension>
schema_fingerprint = Hash128::of(complete bytes).to_hex()
```

The `Extension` binary record and sequence order are those in §16.3. V1 memory preparation exposes
no PRAGMA/attachment input; adding either requires fields in this versioned stream. A configured
database uses the explicit `schema-id`; PostgreSQL additionally binds the engine-reported search
path and extensions. Undeclared ambient connection state is forbidden.
Independent reference fixtures
`crates/align_driver/tests/golden/schema_identity_{sqlite_empty,sqlite_migrations,sqlite_database,postgres}_v1.hex`
and sibling `.digest` files pin all three source tags, both catalog Option states, non-ASCII
`schema_id`/search-path strings, and canonical extension ordering. Production and reference encoders
share no codec functions.

### 16.7 PostgreSQL preparation environment

PostgreSQL preparation may use:

- a development database;
- an ephemeral CI/container database;
- a dedicated schema/database created by the tool.

The command must report the server version, database/schema identity, search path, and installed
extension assumptions that affect Queries.

---

## 17. Migrations

### 17.1 SQL migration files

```text
db/
  migrations/
    0001_create_users.sql
    0002_create_groups.sql
    0003_add_user_group_index.sql
```

Migrations remain SQL. There is no schema DSL and no struct-to-DDL generation.
Discovery, filename validity, contiguous version ordering, symlink rejection, exact-byte hashing,
and schema-input fingerprinting use §16.6's one canonical catalog rule in both prepare and D11
migration commands.

### 17.2 Commands and exact inputs

Every D11 live-database command requires an explicit entry graph, migration catalog, driver, and
target:

```text
alignc db migrate --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
alignc db status  --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
alignc db check   --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH

alignc db migrate --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
alignc db status  --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
alignc db check   --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME

alignc db repair  --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
                  --version N (--accept-applied | --clear-dirty) --expect-checksum HASH
alignc db repair  --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
                  --version N (--accept-applied | --clear-dirty) --expect-checksum HASH
```

`ENTRY` is the explicit `.align` entry used to discover the project/package root. `DIR` is a
project-root-relative migration directory that must pass the §16.6 catalog/path/symlink rules.
The target is exactly one of `--sqlite-path PATH` or `--postgres-url-env NAME`, matching `--driver`;
every missing, duplicate, or mismatched selector fails before opening a database. The SQLite target
opens read-write/create for `migrate`, read-only/no-create for `status` and `check`, and
read-write/no-create for `repair`; a missing file therefore fails every operation except `migrate`.
No command applies an implicit PRAGMA. The PostgreSQL form validates `NAME` as an
environment-variable identifier and reads that one URL value only after command parsing; the
environment-variable name is explicit while the secret value never enters argv, artifacts, logs,
or normal builds. There is no default `DATABASE_URL`, config-file discovery, current-directory
migration scan, or inferred driver.

`migrate` applies pending migrations with driver-appropriate locking and a migration history table.
`status` reports applied/pending/checksum/dirty state without applying. `check` compares the exact
catalog with live history/state without applying or repairing. `repair` performs only the exact
checksum-bound action in §17.5. `alignc db prepare` is the separate explicit metadata-generation
workflow in §16.5–§16.7 and does not weaken these D11 target inputs. None of these commands runs
during a normal build.

### 17.3 Checksums and history

Migration identity includes filename/version and content checksum. An already-applied migration whose
content changed is an error unless an explicit repair workflow is invoked.

### 17.4 Transaction behavior

Every migration has exactly one transaction policy. The optional directive is the first physical
line, is ASCII and case-sensitive, and remains an ordinary SQL line comment:

```sql
-- align:migration transaction=required
-- align:migration transaction=forbidden
```

At most one is present. No directive means `required`. The exact directive bytes participate in the
migration checksum; there is no command-line or ambient configuration override.
Migration files may not contain `BEGIN`, `COMMIT`, `ROLLBACK`, `SAVEPOINT`, or equivalent native
transaction-control statements; the runner owns the boundary. The driver-authoritative script
preparation/screening rejects them before any migration mutates the database.

`required` has one all-or-nothing algorithm on both drivers:

1. acquire the driver migration lock;
2. begin one native write transaction, acquire its database-native history lock, and revalidate the
   exact history schema and already-Applied prefix;
3. execute every statement in file order;
4. revalidate the same schema/prefix, then insert and reread the applied
   history row in the same transaction;
5. commit; an error observed before commit rolls back the whole file, while an uncertain commit
   response uses the reconciliation rule below.

If the engine rejects a statement in that transaction, the migration fails and records no applied
row. The tool never retries it outside the transaction.

`forbidden` is the explicit exceptional path for a native statement that must run outside an
ordinary transaction. Version 1 accepts exactly one database-authoritative statement in such a
file. Before execution, under the migration lock, the tool inserts a history row with
`state = Applying`, version/name/checksum, and zero completed statements. It then executes the one
statement outside a transaction and changes the row to `Applied` only after success. An error is
best-effort recorded as `Failed`; process loss leaves `Applying`. Either non-final state is
**dirty**, blocks all later migrations, and is reported by `status`/`check`. The tool never assumes
whether the statement took effect and never automatically retries it.

Recovery is deliberately visible:

```text
alignc db repair --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
                --version N --accept-applied --expect-checksum HASH
alignc db repair --entry ENTRY --migrations DIR --driver sqlite --sqlite-path PATH
                --version N --clear-dirty --expect-checksum HASH

alignc db repair --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
                --version N --accept-applied --expect-checksum HASH
alignc db repair --entry ENTRY --migrations DIR --driver postgres --postgres-url-env NAME
                --version N --clear-dirty --expect-checksum HASH
```

Both forms require an exact current-file checksum and a dirty row. `--accept-applied` marks the row
applied after the operator verifies native state. `--clear-dirty` removes only the dirty history
row after the operator verifies that retry is safe; it does not undo database effects. Applied rows
cannot be repaired by these commands. D11 tests crash/error boundaries before the statement, after
native success but before the history update, and during error recording on SQLite and PostgreSQL.

A database commit or connection-loss error may have an outcome the client cannot observe. The runner
never converts that uncertainty into a claimed rollback. It closes the failed native connection,
opens one fresh connection, and rereads exact history once under the native lock above. SQLite
retains its separate OS-lock descriptor across connection replacement; PostgreSQL reacquires its
connection-owned advisory lock before the native table lock and reread.
For a Required transaction, an exact Applied row proves that both SQL and history committed; absence
proves neither committed, and the command reports not-applied without retrying in that invocation.
For Forbidden execution, the Applying insert must be durably observed before native execution. An
uncertain final update reconciles as Applied or as the still-dirty Applying row. Failure to reconnect,
relock, or obtain either permitted state reports an explicit outcome-unknown error. A later invocation
begins with ordinary full history reconciliation.

Down migrations are optional and are not automatically generated.

### 17.6 Version-1 history, lock, and command contract

D11 owns one persistent history format. It is package operational state, not a Query artifact or a
normal-build input. SQLite owns the table `__align_migrations_v1`. PostgreSQL owns schema
`align_internal` and table `align_internal.migrations_v1`. `migrate` may create these objects;
`status`, `check`, and `repair` never create them. An absent history object means an empty history.
An existing object with rows or field values outside this exact contract is invalid history rather
than a surface that the tool upgrades in place.

The logical record is exact:

```text
format_version: u32 = 1
version: u32                 # 1 through 9999, primary key
filename: str                # exact canonical catalog filename
checksum: str                # lowercase 32-hex Hash128::of(exact file bytes)
policy: u8                   # 0 Required, 1 Forbidden
state: u8                    # 0 Applying, 1 Applied, 2 Failed
completed_statements: u32
```

The creation DDL is canonical. SQLite executes this exact statement:

```sql
CREATE TABLE "__align_migrations_v1" (
  "format_version" INTEGER NOT NULL CHECK (typeof("format_version") = 'integer' AND "format_version" = 1),
  "version" INTEGER NOT NULL PRIMARY KEY CHECK (typeof("version") = 'integer' AND "version" BETWEEN 1 AND 9999),
  "filename" TEXT NOT NULL CHECK (typeof("filename") = 'text'),
  "checksum" TEXT NOT NULL CHECK (typeof("checksum") = 'text' AND length("checksum") = 32),
  "policy" INTEGER NOT NULL CHECK (typeof("policy") = 'integer' AND "policy" IN (0, 1)),
  "state" INTEGER NOT NULL CHECK (typeof("state") = 'integer' AND "state" IN (0, 1, 2)),
  "completed_statements" INTEGER NOT NULL CHECK (typeof("completed_statements") = 'integer' AND "completed_statements" BETWEEN 0 AND 4294967295)
)
```

PostgreSQL creates the schema and table in one bootstrap transaction and executes this exact table
statement after `CREATE SCHEMA "align_internal"`:

```sql
CREATE TABLE "align_internal"."migrations_v1" (
  "format_version" integer NOT NULL CHECK ("format_version" = 1),
  "version" integer NOT NULL PRIMARY KEY CHECK ("version" BETWEEN 1 AND 9999),
  "filename" text NOT NULL,
  "checksum" text NOT NULL CHECK (length("checksum") = 32),
  "policy" integer NOT NULL CHECK ("policy" IN (0, 1)),
  "state" integer NOT NULL CHECK ("state" IN (0, 1, 2)),
  "completed_statements" bigint NOT NULL CHECK ("completed_statements" BETWEEN 0 AND 4294967295)
)
```

Both absent PostgreSQL objects mean empty history. Exactly one object present is invalid history;
the tool never adopts a pre-existing schema. SQLite requires the one table's `sqlite_schema.sql` to
equal the canonical DDL bytes above and permits no other `sqlite_schema` row whose `tbl_name` is
`__align_migrations_v1`. It also permits no `sqlite_temp_schema` row whose `tbl_name` is that table;
this excludes persistent and connection-local indexes, triggers, views, and rewritten table forms.
No other main-database table may have a foreign key that references the history table; this prevents
a required exact-snapshot restore from cascading into application data.
Every history query and mutation explicitly qualifies `main.__align_migrations_v1`, so a temporary
object cannot shadow the owned table.
PostgreSQL requires a permanent ordinary heap table owned by the current role, with no partition or
inheritance relation, row security, forced row security, trigger (including an internal trigger from
an inbound foreign key), rewrite rule, policy, extra index, generated/default/identity expression,
non-owner table or column ACL, or other behavior-affecting object
attached to the history table. Its only index is the valid, ready, immediate,
default-btree primary-key index on `version`; its constraints are exactly that primary key and the
six immediate validated checks in the DDL. The schema is owned by the current role. Unrelated schema
objects and schema grants do not affect fully qualified history DML and are not part of this table
invariant.

Readers validate that complete table-attached ancillary-object inventory plus exact column order,
declared types, nullability, primary key, and check expressions through one joined driver-catalog
query, then validate every row's
storage/type, exact lowercase checksum spelling, filename/version agreement, and field combinations.
PostgreSQL `completed_statements` alone is `bigint`; its contract is still the full unsigned 32-bit
range above. A schema disagreement, unrepresentable native value, or semantic row violation fails
the command before any mutation. A replaced or weakened table therefore fails closed rather than
being upgraded.
`Applying` and `Failed` require `policy = Forbidden` and `completed_statements = 0`. `Applied`
with a current catalog file requires `completed_statements` to equal the driver-screened statement
count. A HistoryOnly Applied row has no file to rescreen: Forbidden still requires one and Required
requires a nonzero `u32`; reintroducing that version restores the exact-count check. Required migrations
publish only one `Applied` row inside their migration transaction; they never expose Applying or
Failed. Forbidden migrations expose Applying with zero, then Applied with one after native success;
best-effort error recording changes only Applying to Failed and leaves zero.

The lock covers history bootstrap/read/validation and the complete operation. SQLite uses a
persistent empty sibling file formed by appending `.align-migrate.lock` to the exact database path.
Every command atomically opens or creates that one file without replacement, rejects a symlink,
non-regular file, or nonempty file after `fstat`, and then holds an exclusive OS lock for
`migrate`/`repair` or a shared OS lock for `status`/`check`. An absent-path creator uses exclusive
creation with mode `0600`; an `AlreadyExists` race reopens the winner without truncation. All
cooperating operations therefore linearize at acquisition of the same persistent inode. Creating
this operational lock is the only filesystem write permitted to `status`/`check`; they still open
the database read-only and never create history/schema objects. PostgreSQL holds session advisory
lock key
`(1095518535, 1296647985)` (`ALIG`, `MIG1`) in exclusive or shared mode respectively. Process or
connection loss releases the OS/advisory lock; the persistent SQLite file is never deleted.

The OS/advisory lock serializes cooperating Align commands; database-native locking closes the
validation-to-history-DML interval against non-cooperating database connections. SQLite bootstrap,
Required, repair, and each Forbidden history transition use `BEGIN IMMEDIATE`; validation occurs
only after that transaction acquires its write reservation. SQLite `status`/`check` use one read
transaction whose first `main.sqlite_schema` read fixes the snapshot before schema and row
validation. Each PostgreSQL history phase uses one explicit `READ COMMITTED` transaction. Its first
SQL after `BEGIN` blindly attempts
`LOCK TABLE "align_internal"."migrations_v1" IN ACCESS EXCLUSIVE MODE` before history
validation/mutation or `IN SHARE ROW EXCLUSIVE MODE` before `status`/`check` validation. The latter
still permits ordinary readers but conflicts with history DML and both ordinary and concurrent
index/DDL lock modes. The lock therefore waits
for prior writers before the first catalog/history read, and later reads see no table writer until
transaction end. No existence query precedes the lock.

Only SQLSTATE `42P01` (undefined table) or `3F000` (invalid schema name) selects the
absent-owned-object path, after rollback of that failed transaction. `migrate` then attempts the
exact schema plus table bootstrap in one new transaction,
validates its new objects, commits, and restarts from the blind-lock phase; a pre-existing schema or
any creation race fails rather than being adopted. `status`/`check` use one new transaction and one
catalog query: both objects absent is the empty snapshot, while exactly one present is invalid.
`repair` reports missing history. Every other lock error fails directly. This makes bootstrap and
first-reader races deterministic without weakening the rule that only `migrate` creates history.

Forbidden user SQL runs on a separate worker connection after Applying commits. That worker never
performs history DML and is closed before the history connection starts the final native-lock,
revalidation, and Applied/Failed transaction. Thus worker-local temporary objects or settings cannot
affect history mutation and are outside the recorded history-connection invariant; persistent changes
remain visible to the final validation. The runner retains the exact Applied-prefix-plus-Applying
history snapshot published before the worker. Under the final native lock it validates the owned
schema and compares the complete history to that snapshot. An unchanged snapshot is not rewritten:
only its current Applying row is updated to Applied or Failed. Any row change restores and rereads
the exact snapshot, then fails visibly with Applying preserved. If the worker removed the owned table, this migrate
invocation recreates the exact table (and the PostgreSQL schema only when both owned objects are
absent), restores Applying, and fails visibly;
a malformed replacement is not dropped or adopted and blocks later migration. User SQL or a
non-cooperating writer therefore cannot erase or forge the dirty checkpoint into an automatic retry.
Required user SQL remains on the one transaction connection,
so its connection-local inventory is validated before the row insert. Lock acquisition always
precedes validation, and the native transaction/lock is released only after the corresponding read
or history mutation completes.

The migration directory is project-root-relative. The entry must be an existing regular,
non-symlink `.align` file; its lexical parent is the project root. An absolute migration path,
`..`, a symlinked directory, or canonical escape is rejected before enumeration. A relative SQLite
target is resolved against that project root; an absolute target remains explicit. The final target
and lock file may not be symlinks. Every command may create only a missing lock after all catalog and
policy validation; `migrate` alone may also create the final database. Every operation except
`migrate` requires the database to exist. PostgreSQL
environment names match `[A-Za-z_][A-Za-z0-9_]*`, must not begin with `PG`, and use the complete URL
and ambient-`PG*` rejection rules from §16.2.

Validation precedence is one deterministic sequence:

1. visit argv tokens in source order, decoding each as non-empty UTF-8 without U+0000 before
   rejecting an unknown or duplicate option at that token;
2. validate the operation-specific required fields, exactly one matching target, repair action,
   version, and lowercase expected checksum;
3. validate entry, project-relative catalog containment, then the complete §16.6 catalog;
4. classify every first-line policy, screen every complete statement, reject an empty file,
   transaction-control statement, or Forbidden file whose database-authoritative count is not one;
5. read the one PostgreSQL URL environment value when selected and validate all connection inputs;
6. validate the exact target, acquire its migration lock, open/read the target and validate the
   complete history, then perform the selected operation. SQLite acquires the file lock before its
   database open; PostgreSQL connects and immediately acquires the advisory lock before any history
   or user-schema request.

Within phase 1, the operation name is validated first, then tokens are visited in source order. For
each token, UTF-8 decoding precedes the empty and U+0000 checks; missing option values, unknown
options, and the second occurrence of a duplicate option fail at that token. Phase 2 validates
`--entry`, `--migrations`, `--driver`, and the matching target in that order. Repair then validates
`--version`, exactly one action, and `--expect-checksum`; non-repair commands reject those fields in
the same order.

Phase 3 validates the entry before the migration path. It snapshots all immediate directory entries;
an enumeration error wins, then any non-UTF-8 name produces one path-independent error. Remaining
names are sorted by UTF-8 bytes. In that order, metadata-read failure wins over symlink and invalid
regular `.sql` name errors for the same entry; unrelated non-SQL regular files, directories, and
other non-regular entries remain ignored. Selected names then validate version, duplicate, and gap
rules in numeric order. Selected contents are read in numeric order; read/count overflow,
invalid UTF-8, then U+0000 is the per-file precedence. Phase 4 also visits files in numeric order.
Within one file, policy classification precedes lexical completeness, empty-script, transaction-
control in statement order, and finally Forbidden-count validation. Phase 5 uses §16.2's ambient
`PG*`, selected-variable presence/value, and complete-URL validation order. Phase 6 reports target,
lock, schema shape, history row, and selected-operation errors in that order; within history, version
order and record-field order above determine the first error. These traversal rules select one
winner for every multi-invalid input and are shared by both drivers.

The policy directive is recognized only as the exact first physical line terminated by LF or EOF.
CRLF or leading bytes do not match it and therefore select the default Required policy. SQL
screening ignores quoted strings, quoted identifiers, line/block comments, and PostgreSQL
dollar-quoted bodies. It rejects a top-level statement whose first tokens form `BEGIN`, `START
TRANSACTION`, `COMMIT`, `END`, `ROLLBACK`, `ABORT`, `SAVEPOINT`, `RELEASE`, `PREPARE TRANSACTION`,
`SET TRANSACTION`, `SET LOCAL TRANSACTION`, `SET SESSION TRANSACTION`, or `SET SESSION
CHARACTERISTICS AS TRANSACTION`. SQLite statement
boundaries, including trigger bodies, use `sqlite3_complete`; PostgreSQL boundaries use the same
dollar-quote-aware driver scanner. Both drivers finish this complete screening phase before opening
the target or publishing an Applying row; native execution remains authoritative for SQL validity.
After user SQL and before every history insert/update, the runner revalidates the complete owned
persistent and applicable history-connection-local schema/ancillary-object inventory and expected
prefix under both the operation lock and database-native lock above. Required also rereads its new
row before commit. Forbidden closes its worker, acquires the final native lock on the history
connection, then performs the same validation before the final update. SQL that alters, replaces,
shadows, or attaches behavior to history therefore rolls back a Required file or leaves a Forbidden
row visibly dirty; it cannot silently erase or forge progress.

History comparison is version ordered. Every current catalog version produces exactly one status:
`Pending`, `Applied`, `NameMismatch`, `ChecksumMismatch`, `PolicyMismatch`, `DirtyApplying`, or
`DirtyFailed`. A history version absent from the current catalog is
`HistoryOnly`. When a current and history row share a version, mismatch precedence is Name, Checksum,
Policy, Applying, Failed, then Applied. A missing earlier Applied row, any mismatch/dirty/history-only
row, or a non-prefix Applied set blocks `migrate` before executing a pending file.

The printed state tags are respectively `pending`, `applied`, `name_mismatch`, `checksum_mismatch`,
`policy_mismatch`, `dirty_applying`, `dirty_failed`, and `history_only`. `mismatched` counts checksum,
name, and policy mismatches; the other summary fields count their same-named states. The five summary
counts therefore cover every printed row exactly once.

All four commands print current catalog rows in version order followed by HistoryOnly rows in
version order. Every field is always present. `catalog_*` values come only from the current catalog;
`history_*` values come only from the stored row. A missing side uses the exact unavailable token
`-`, `history_state` is `applying`, `applied`, `failed`, or `-`, and `history_completed` is the
stored decimal `u32` or `-`:

```text
migration version=0001 catalog_name=0001_create_users.sql catalog_checksum=<32hex> catalog_policy=required history_name=0001_create_users.sql history_checksum=<32hex> history_policy=required history_state=applied history_completed=1 state=applied
summary driver=sqlite applied=1 pending=0 dirty=0 mismatched=0 history_only=0
```

`status` exits successfully after a complete read even when the summary is not current. `check`
uses the same rows and succeeds only when every catalog row is Applied and there are no other rows.
Successful `migrate` prints the final all-Applied view. Successful `repair` prints the final view
after changing exactly one checksum-matching dirty row. Errors and PostgreSQL output never include
the URL or an environment value. `--accept-applied` records the screened statement count;
`--clear-dirty` removes only that row. Neither action accepts an Applied row, a non-current version,
or a current/history checksum different from `--expect-checksum`.

---

## 18. Metadata and introspection

Metadata is fine-grained and opt-in. Requesting one category must not silently scan all related
catalogs.

### 18.1 Detail level

```text
db.MetaDetail.Names
db.MetaDetail.Summary
db.MetaDetail.Full
```

`Full` means full information for the requested category, not recursive retrieval of every child
category.

### 18.2 Separate operations

The common result records are flat `RegionPlain` values. Their first-release field contract is:

```text
db.MetaTableKind      Table | View | MaterializedView | Native
db.MetaNullability    Yes | No | Unknown
db.MetaKeyKind        Primary | Unique | Foreign | Check | Exclusion | Native
db.MetaForeignKeyMatch Simple | Full | Partial
db.MetaReferentialAction NoAction | Restrict | Cascade | SetNull | SetDefault
db.MetaIndexTermKind  Key | Included
db.MetaSortOrder      Asc | Desc
db.MetaNullOrder      First | Last
db.MetaQueryState     Declared | DatabaseChecked
db.MetaQueryEntry     Summary | Parameter | Column
db.MetaStatementClass Select | Dml | Ddl | Native | Unknown
db.PlanFormat         Text | Json | Native

db.SchemaRef
  name: str

db.TableRef
  schema, name: str

db.DatabaseMeta
  driver: Driver
  name, engine_version: str
  default_schema, encoding, collation: Option<str>
  read_only, transactional_ddl: Option<bool>

db.SchemaMeta
  name: str
  owner: Option<str>
  visible, system: bool

db.TableMeta
  schema, name: str
  kind: MetaTableKind
  native_kind, owner, comment: Option<str>
  estimated_rows: Option<f64>

db.ColumnMeta
  schema, table, name: str
  ordinal: i64
  logical_type, native_type: Option<str>
  native_type_id: Option<i64>
  nullable: MetaNullability
  default_sql, generated_sql, identity_kind, collation, comment: Option<str>
  origin_schema, origin_table, origin_column: Option<str>

db.KeyMeta
  schema, table: str
  name: Option<str>
  kind: MetaKeyKind
  key_ordinal: i64
  term_ordinal: i64
  local_column, referenced_schema, referenced_table, referenced_column, expression: Option<str>
  match_policy: Option<MetaForeignKeyMatch>
  on_update, on_delete: Option<MetaReferentialAction>
  deferrable, initially_deferred, validated: Option<bool>

db.IndexMeta
  schema, table, name: str
  unique, primary_backed: Option<bool>
  term_ordinal: i64
  term_kind: MetaIndexTermKind
  column, expression, predicate, native_method, native_opclass: Option<str>
  sort_order: Option<MetaSortOrder>
  null_order: Option<MetaNullOrder>
  valid, ready: Option<bool>

db.QueryMeta
  query_id: str
  driver: Driver
  driver_restriction: DriverRestriction
  statement_class: MetaStatementClass
  artifact_digest: str
  state: MetaQueryState
  metadata_fingerprint: Option<str>
  source_sql_hash, driver_wire_sql_hash: str
  rewrite_format_version: i64
  prepare_identity, schema_identity, server_identity: Option<str>
  entry: MetaQueryEntry
  ordinal: Option<i64>
  source_name, source_alias, logical_type, native_type: Option<str>
  native_type_id: Option<i64>
  origin_schema, origin_table, origin_column: Option<str>
  nullable: MetaNullability

db.QueryPlan
  driver: Driver
  format: PlanFormat
  analyzed: bool
  body: str
```

#### 18.2.1 Detail projection, rows, and ordinals

Every optional field outside the selected detail is `None`. At the selected detail, unavailable
engine evidence is also `None`; empty text, zero, and `false` are real reported values and never
stand for absence. A field that is not applicable to that record discriminator is `None` even at
`Full`. A required evidence enum uses its explicit `Unknown`/`Native` state when evidence is
suppressed or unavailable; it is never guessed. Required identity fields are present at every
detail. An optional identity explicitly named in the matrix is requested at every detail and uses
`None` only when the engine exposes no value. Results use byte-lexicographic `str` ordering after
the category keys below, so engine/catalog iteration order is never observable.

| Category | Rows and required fields at every detail | `Names` | `Summary` | `Full` |
|---|---|---|---|---|
| `DatabaseMeta` | exactly one; `driver`, `name`, `engine_version` | all optional fields `None` | request `default_schema`, `read_only`, `transactional_ddl` | Summary plus `encoding`, `collation` |
| `SchemaMeta` | one per selected schema, ordered by `name`; `name`, `visible`, `system` | `owner = None` | request `owner` | same common fields as Summary |
| `TableMeta` | one per selected table/view, ordered by `(schema, name)`; `schema`, `name`, `kind` | all optional fields `None` | request `native_kind`, `owner`, `estimated_rows` | Summary plus `comment` |
| `ColumnMeta` | one per column, physical/result order; `ordinal` is zero-based; `schema`, `table`, `name`, `ordinal` | optional fields `None`; `nullable = Unknown` | request `logical_type`, `native_type`, and catalog-column `nullable` | Summary plus `native_type_id`, default/generated/identity/collation/comment, and view-origin fields |
| `KeyMeta` | one row per key/constraint term, ordered by `(key_ordinal, term_ordinal)`; both ordinals are zero-based; schema/table, `kind`, ordinals required; optional `name` identity requested at every detail | only `name` may be `Some`; other optional fields `None` | Names plus local/referenced names and `expression` | Summary plus match/update/delete, deferral, and validation evidence |
| `IndexMeta` | one row per term, ordered by `(name, term_ordinal)` with key terms before included terms; `term_ordinal` is zero-based across that order; identity, ordinal, `term_kind` required | all optional fields `None` | request unique/primary backing, column/expression/predicate, and sort/null order | Summary plus native method/opclass and valid/ready evidence |
| `QueryMeta` | ordering and discriminator rules below; Query/driver identity, class, artifact/state, source/wire hashes, rewrite version, and `entry` required on every row | one `Summary` row only | exactly Summary, then every Parameter, then every Column; source names/aliases and logical types | same row groups plus checked native/origin/nullability and prepare/schema/server evidence |

`ColumnMeta.nullable` describes the catalog column declaration when requested; it does not prove
the nullability of a Query result. It is `Unknown` at Names and whenever catalog evidence is
unavailable at Summary/Full. `QueryMeta.nullable` follows §16.3.1.

Constraint names are optional and are not assumed unique; `None` means the engine exposes no name
and no synthetic name is fabricated. For each table, the driver builds each complete common key
group, then canonically sorts groups by the complete Full-detail signature: `kind` declaration tag;
`name` Option tag/UTF-8 bytes; the ordered term sequence including every local/reference/expression
field; `match_policy`; `on_update`; `on_delete`; `deferrable`; `initially_deferred`; and
`validated`. Enum and Option tags use declaration order, strings use byte-lexicographic order, and
booleans use `false` before `true`. A driver must normalize group-level policy/evidence to one value
before sorting and reject contradictory engine rows. Only byte-identical complete common groups can
tie, so their physical order cannot affect common output. The driver assigns `key_ordinal` from zero
after sorting. Every term of a group repeats that `key_ordinal`; `term_ordinal` starts at zero within
the group.
Names/Summary may suppress non-identity fields after this identity/order computation, never before
it.

`QueryMeta` has these exact discriminator rules:

| `entry` | Row presence and order | `ordinal` | Applicable optional fields |
|---|---|---|---|
| `Summary` | exactly one first | `None` | `metadata_fingerprint` at Summary/Full when checked; `prepare_identity`, `schema_identity`, and `server_identity` only at Full when checked |
| `Parameter` | absent at Names; one per distinct source parameter at Summary/Full, ordered by protocol ordinal | one-based protocol ordinal (`$1` is 1) | `source_name`, `logical_type`; plus checked `native_type`/`native_type_id` at Full |
| `Column` | absent at Names; one per Row field at Summary/Full, ordered by decoder position | zero-based decoder ordinal | `source_alias`, `logical_type`; plus checked `native_type`/`native_type_id`, structured origin, and §16.3.1 `nullable` at Full |

The complete row order is the one Summary entry, every Parameter in increasing protocol ordinal,
then every Column in increasing decoder ordinal; the groups are never interleaved. On `Summary` and
`Parameter` entries, `nullable` is `Unknown`. A Column entry is also `Unknown` at Summary, at Full
when checked evidence is unavailable/ambiguous, and at every detail for a `Declared` Query.
`source_alias` is inapplicable to Parameter, `source_name` is inapplicable to Column, and every
origin field is inapplicable to Parameter. The `metadata_fingerprint` and prepare/schema/server
identities occur only on the `Summary` entry; they are not duplicated on Parameter/Column rows. A
`Declared` Query has no checked-only fields at any detail.

`artifact_digest` is the 32-character lowercase hexadecimal `Hash128::of(...).to_hex()` value
(`lo`, then `hi`) of the exact versioned D1 `StaticQueryArtifact` bytes emitted for this Query
descriptor (the digest is not embedded in those bytes). The bytes cover Query identity, driver
restriction, source SQL, static
Query options, Params/Row fingerprints, binder/decoder ABI versions, and every permitted driver's
wire SQL/rewrite/binding/checked-metadata entry in `Driver` enum order. The same digest is repeated
on each driver-specific `QueryMeta` row for that descriptor. Runtime options, connection/secret
data, requested `MetaDetail`, and metadata output ordering are excluded.

Keys/constraints and indexes with several terms are repeated flat rows. Key groups use
`key_ordinal` because `name` need not be unique; terms use `term_ordinal`. Index key terms precede
included terms and `term_kind` distinguishes them. A category result never hides a nested
allocation. `db.QueryMeta` begins with one
`Summary` row followed by ordered parameter and column rows. `source_sql_hash`,
`driver_wire_sql_hash`, and `rewrite_format_version` are always present because D1 creates them for
every static Query/driver even in `Declared` state. Other optional fields are `None` when the
requested detail level, checked state, or engine evidence does not supply them; base identity fields
are always present. Driver-native operations return corresponding driver-specific flat
`RegionPlain` records
and may add native fields, but use the same destination rule.

The exact common declarations are:

```text
pub fn meta_database(
  exec: db.exec, detail: db.MetaDetail, out: region, options: slice<db.MetaOption>,
) -> Result<db.DatabaseMeta, db.Error>
pub fn meta_schemas(
  exec: db.exec, detail: db.MetaDetail, out: region, options: slice<db.MetaOption>,
) -> Result<array<db.SchemaMeta>, db.Error>
pub fn meta_tables(
  exec: db.exec, schema_filter: Option<db.SchemaRef>, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.TableMeta>, db.Error>
pub fn meta_table(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<db.TableMeta, db.Error>
pub fn meta_columns(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.ColumnMeta>, db.Error>
pub fn meta_keys(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.KeyMeta>, db.Error>
pub fn meta_indexes(
  exec: db.exec, table_ref: db.TableRef, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.IndexMeta>, db.Error>
pub fn meta_query<P, R>(
  exec: db.exec, query: db.query<P, R>, detail: db.MetaDetail,
  out: region, options: slice<db.MetaOption>,
) -> Result<array<db.QueryMeta>, db.Error>
pub fn explain<P, R>(
  exec: db.exec, query: db.query<P, R>, params: P,
  out: region, options: slice<db.ExplainOption>,
) -> Result<db.QueryPlan, db.Error>
```

The block above is exact API signature notation, not a bodyless Align source file. Calls below are
ordinary syntax-checked Align positional expressions; types belong on bindings/declarations, never
in an argument:

```align
schema_filter: Option<db.SchemaRef> = None
table_ref: db.TableRef = db.TableRef { schema: "main", name: "users" }
q := query()
database: db.DatabaseMeta =
  db.meta_database(exec, detail, out, [])?
schemas: array<db.SchemaMeta> =
  db.meta_schemas(exec, detail, out, [])?
tables: array<db.TableMeta> =
  db.meta_tables(exec, schema_filter, detail, out, [])?
table: db.TableMeta =
  db.meta_table(exec, table_ref, detail, out, [])?
columns: array<db.ColumnMeta> =
  db.meta_columns(exec, table_ref, detail, out, [])?
keys: array<db.KeyMeta> =
  db.meta_keys(exec, table_ref, detail, out, [])?
indexes: array<db.IndexMeta> =
  db.meta_indexes(exec, table_ref, detail, out, [])?
query_meta: array<db.QueryMeta> =
  db.meta_query(exec, q, detail, out, [])?
plan: db.QueryPlan =
  db.explain(exec, q, params, out, options)?
```

These are distinct public primitives. Each metadata category has exactly one form whose final
argument is `slice<db.MetaOption>`; `[]` means no options. The corresponding
`sqlite.meta_*_native`/`postgres.meta_*_native` form receives that common slice plus one
driver-native option slice. `meta_table(Full)` does not automatically fetch columns, keys, indexes,
or plans. `meta_table` returns `db.Error.NotFound` when absent; it does not return an optional
partially initialized record.

`SchemaRef` and `TableRef` are Copy view inputs borrowed only until the metadata call returns; the
driver never stores them. `SchemaRef.name` selects one exact engine schema/attached-database name.
`None` in `meta_tables` means all accessible non-system schemas, plus system schemas only when the
explicit option requests them. `TableRef` always carries an exact schema and name, so lookup never
depends on PostgreSQL search path or an inferred SQLite `main`. Names are bound/escaped through the
driver metadata API and are never pasted into SQL.

Every `SchemaRef.name`, `TableRef.schema`, and `TableRef.name` is checked for U+0000 before any
driver/native metadata call. Rejection returns `db.Error.Encode(db.ContractError { query_id: None,
item, message: "metadata identifier contains U+0000" })`. `item` is exactly
`"metadata.schema"`, `"metadata.table.schema"`, or `"metadata.table.name"` for the corresponding
component. No SQL/catalog request is sent on this error.
Validation follows public record declaration order. `SchemaRef` checks `name`; `TableRef` checks
`schema` before `name`. If both TableRef components contain U+0000, the result is therefore
`item = "metadata.table.schema"`. Both-driver tests pin this dual-invalid precedence.

Every string byte and every result array is allocated in the explicit `out` region before the native
metadata/result buffer is released. No returned value borrows a connection, result, statement, or
temporary catalog row, and no metadata primitive chooses the heap. Array construction uses L6's
region builder and its one measured compacting pass. Native forms take the same `out` immediately
before the common/native option slices. EXPLAIN follows the same rule for `QueryPlan.body`.

### 18.3 Database metadata

Common fields where available:

```text
driver
database name
server/library version
default schema/search path
encoding/collation summary
read-only status
transaction capabilities
```

Native driver metadata may expose more.

### 18.4 Schema metadata

```text
schema/catalog name
owner where available
visibility/search-path state
system/user classification
```

SQLite maps attached databases (`main`, `temp`, attached names) into its native metadata model; the
common schema view should not invent PostgreSQL semantics.

### 18.5 Table/view metadata

```text
name
schema/catalog
kind (table/view/materialized/native kind)
owner where available
estimated row information where available
comment where available
```

### 18.6 Column metadata

```text
name
ordinal
common logical type
native type name/id
driver type id/OID where available
nullable
default/generated/identity state
collation
comment
```

### 18.7 Keys and constraints

The common model should represent:

```text
primary key
unique key/constraint
foreign key
check constraint when representable
column order
referenced table/columns
match/update/delete actions
deferrability where supported
optional engine-reported constraint name
```

Composite keys preserve ordered columns and `key_ordinal` grouping even when names are absent or
duplicated.

### 18.8 Index metadata

Common fields:

```text
name
table
unique
primary-backed
ordered key columns
included columns where available
expressions where available
partial predicate where available
sort/null ordering where available
method/type summary
valid/ready state where available
```

No common field should erase information required to identify an index correctly. Native detail holds
what is not portable.

### 18.9 Query metadata

```text
Query identity
source SQL hash
driver wire SQL hash and rewrite-format version
driver restriction
verification state
parameter names/order/common types/native types/native ids
result column names/order/common types/native types/native ids/nullability
origin table/column when the database reports it
prepare/schema/server identity
```

### 18.10 Query plans

Plan retrieval is explicit because it may be expensive. Common `db.explain` is inspection-only.
PostgreSQL execution analysis is selected only by
`postgres.explain_native(..., [postgres.ExplainOption.Analyze, ...])`.

```text
EXPLAIN only        plan without running user statement where supported
EXPLAIN ANALYZE     PostgreSQL-native Analyze option explicitly executes
```

The native API makes “executes the statement” visible and counts it. SQLite exposes no v1 Analyze
option. Plan output may be text, JSON/native structured plan, or both.

### 18.11 PostgreSQL native metadata

The native package may expose:

- OIDs;
- relkind/persistence;
- namespaces;
- domains/enums/ranges;
- opclasses/opfamilies;
- access methods;
- index expressions/predicates/include columns;
- storage parameters;
- constraint validation/deferrability;
- statistics and planner estimates;
- extension ownership;
- server plan JSON.

### 18.12 SQLite native metadata

The native package may expose:

- attached database name;
- declared type and affinity;
- `STRICT`;
- `WITHOUT ROWID`;
- generated/hidden columns;
- virtual table/module;
- index origin and partial status;
- raw creation SQL;
- PRAGMA-derived details;
- query-plan rows;
- primary/extended result code details.

---

## 19. Dynamic SQL escape hatch

Static typed Queries are the default. Runtime SQL is explicit and weaker.

```align
rows := db.dynamic_rows(
  exec,
  db.Driver.SQLite,
  sql_text,
  params,
  out,
  [],
)?
```

Dynamic results use:

```text
db.row
db.value
```

The second argument is an exact `db.Driver` value, not `AnySupportedDriver` and not an inferred
property of `exec`. `db.dynamic_execute` and `db.dynamic_rows` compare it with the execution handle
and return `DriverMismatch` before sending SQL. Their parameter values are an explicit slice, and
their final argument is `slice<db.ExecuteOption>`. A driver-native variant is itself qualified as
`sqlite.dynamic_*_native`/`postgres.dynamic_*_native`; that module path is its exact restriction,
and it receives one common option slice plus one native option slice.

They do not silently decode into an arbitrary typed struct using reflection.

Identifiers cannot be bound as ordinary values. A future safe identifier/composition API must quote
according to one explicit driver and must not become a general hidden Query builder. Static branching
between named Queries is preferred.

String concatenation of untrusted values into SQL is not a package-supported binding mechanism.

---

## 20. Concurrency, cancellation, and timeouts

- A connection/transaction/statement is not assumed thread-safe unless the driver contract explicitly
  says so.
- Concurrent work normally uses separate physical connections.
- A timeout is the exact operation-scoped `TimeoutNs`/`BeginTimeoutNs` option, not an implicit
  default.
- D9 must enforce a requested deadline or reject it before SQL send; it may not start an operation
  and then ignore the timeout.
- An applied deadline uses driver-owned native interruption/cancellation machinery and maps expiry to the
  stable `Timeout` category with native detail. An engine-reported cancellation not caused by the
  local deadline maps to `Cancelled`. Neither path issues a hidden SQL statement.
- PostgreSQL v1 uses nonblocking libpq wait plus native cancellation. SQLite v1 returns
  `Unsupported` for the common operation deadline before SQL send; its native `BusyTimeoutNs`
  controls lock waits only and must not masquerade as a whole-query deadline. Applying a later
  SQLite deadline requires the general noncapturing C-callback boundary (or another separately
  proved native mechanism), not a database-specific compiler exception.
- V1 has no externally shareable cancel resource: L3 resources/resource references are non-Send and
  synchronous execution exposes no sound concurrent caller. A public user-triggered cancel handle
  is not part of the accepted v1 design. A future concrete proposal must first schedule a general
  Send/thread-safe-resource prerequisite and its own roadmap slice; D9 does not imply either.
- After timeout or cancellation, the driver may return a connection for reuse only after it has
  drained required results and proved protocol and transaction state synchronized. Otherwise it
  poisons/closes the connection; uncertain state never returns to a caller or future pool.
- A dropped row stream must release/finalize its driver result promptly.
- A connection with an active unread result follows the driver’s documented rule; the package must not
  silently buffer an unbounded remainder to make reuse appear possible.

Pooling is separate future work and must expose wait, timeout, size, and connection affinity.

---

## 21. Performance contract

Performance is a headline requirement, but measured behavior wins over slogans.

### 21.1 Required invariants

```text
1. no hidden SQL statements
2. no per-row reflection
3. no required per-row heap allocation
4. prepared mapping reused across rows
5. explicit materialization/streaming
6. borrowed text/blob where lifetime permits
7. one-pass compound shaping
8. native binary/columnar paths remain possible
9. database-specific optimization remains visible and usable
10. parameter bind retention/copy bytes are explicit and measurable
```

### 21.2 One statement is not automatically fastest

The standard compound-read form uses one named Query and one SQL statement because execution count
and database relational work remain visible. The package does not claim that every possible relation
should be forced into one product join.

The SQL author uses CTEs, native aggregates, tagged streams, or another visible plan to avoid row
explosion. If an application explicitly chooses two Queries after measurement, it writes two calls.

### 21.3 Local measurement anchors

The implementation keeps named local measurements for at least:

- SQLite parameter bind + one-row typed decode;
- SQLite streamed text/blob bind with transient-copy bytes and allocations separated;
- PostgreSQL parameter bind + one-row typed decode;
- file/inline Query/command artifact generation and cold/warm rebuild;
- structural contract/artifact and QueryMeta-plan bytes/time at 1/10/100 reachable definitions;
- canonical checked-metadata JSON encode/decode/validation at 10/100/1000 columns;
- SQLite canonical migration catalog fingerprint/replay at 10/100/1000 files;
- SQLite active-execution lease acquire/release/rejected-overlap overhead;
- prepared repeated execution;
- large streamed flat result;
- one-to-many one-pass shaping;
- segmented parent/child output;
- text vs PostgreSQL binary result where supported;
- direct SoA/batch decode when implemented;
- metadata request granularity, destination region bytes/compact count, and native-buffer copy bytes;
- package overhead compared with direct libsqlite3/libpq code.

The common layer should be within measurement noise of an equivalent direct driver loop after
preparation, excluding costs explicitly requested by the caller.
Run these measurements when their named path first lands or materially changes, or for an explicit
performance investigation. They are not regression, integration, PR, release, or milestone gates.

### 21.4 Execution-count tests

Every compound Query example has a test hook proving:

```text
expected SQL executions = 1
actual SQL executions   = 1
```

Shaping tests run without any execution handle, making follow-up SQL structurally unavailable.

---

## 22. Diagnostics

Diagnostics should reference Query identity, SQL file/line where possible, parameter/result name, and
driver.

Examples:

```text
query db.queries.user_with_groups:
  result column `group_id` may be NULL
  Row.group_id has type i64
  expected Option<i64>
```

```text
query db.queries.customer_totals:
  parameter `minimum_orders` is required by SQL
  Params has no field with that name
```

```text
checked metadata is stale:
  source SQL hash changed for db/queries/user_by_id.sql
  run: alignc db prepare
```

```text
PostgreSQL Query cannot execute on SQLite connection:
  Query requires PostgreSQL (uses native Query options)
```

Unsupported/unknown native options must identify the option and driver rather than disappearing.

---

## 23. Roadmap

The implementation follows a small number of prerequisite and vertical capability PRs. A database
PR is not allowed to paper over a missing prerequisite with a package-name special case. Roadmap
labels own acceptance closure; they do not each require a separate PR. Every executable milestone runs
an `.align` program end to end and tests execution count, decoded values, ownership/Drop, cleanup, and
errors.

The D1–D14 contracts below are unchanged. Delivery groups them by useful
consumer outcome:

| Wave | Acceptance owners | Mergeable outcome | Default publication boundary |
|---|---|---|---|
| P0 native evidence | D0 | recorded SQLite/libpq behavior with no public API | no product PR; run during prerequisites |
| Q1 static Query | D1 | generated Query/command executes end to end over the fake driver | one capability PR |
| Q2 dual-driver scalar | D2 + D4 | the same scalar Query/command surface runs on SQLite and PostgreSQL | one coordinated capability PR |
| Q3 checked/offline parity | D3 + D5 | both drivers share one offline checked-metadata and invalidation contract | one coordinated capability PR |
| Q4a reusable execution | D6 + D7 | prepared statements and transactions share one reusable execution/ownership model | one capability PR after Q2, parallel with Q3 |
| Q4b streaming resilience | D8 + D9 | typed streaming, deadline, cancellation, and cleanup form one resilient lifecycle | one capability PR after Q4a |
| Q5 schema tooling/inspection | D11 + D12 | migrations plus read-only metadata/EXPLAIN complete the schema-facing product | two parallel capability PRs are permitted because mutation and inspection are independent failure domains |
| Q6 compound product | D10 | many-to-one and one-to-many Output run once end to end | one capability PR after Q4b |
| A1 throughput/native train | D13 | batch/SoA and driver-native throughput surfaces | independently usable common/driver rails may merge in parallel |
| A2 dynamic/callback train | D14 | dynamic rows plus proved native callbacks | dynamic SQL and driver callback rails may merge in parallel |

Q1–Q4b and Q6 do not split at their internal D labels. A split is justified only
when both sides already execute end to end, are independently useful, and do
not repeat the same matrix, review, or broad gate. Q5 is the deliberate
exception because migrations mutate external state while metadata/EXPLAIN is
read-only. A1/A2 are additive release trains, so independent native rails do
not serialize each other; complete-roadmap status still waits for every D13 and
D14 acceptance cell.

A1 defaults to four consumer-visible rails: common batch/SoA, PostgreSQL native
throughput, SQLite native services, and the explicit pool. A2 defaults to the
dynamic SQL/value/row rail followed by independently proved SQLite and
PostgreSQL callback rails. A rail may use multiple commits but receives one
review and one selected broad gate when its useful surface is stable. No
unspecified additional driver is required for completion; any added driver is
its own consumer-backed rail after the common contracts are proven.

During active implementation, eight hours must leave a compiling,
focused-owner-backed source checkpoint and twenty-four hours must leave a whole
capability PR-ready, or one independently useful A1/A2 rail. If not, record the
dominant cost and re-cut at the nearest consumer boundary. Do not answer a miss
with another dormant seam, documentation expansion, repeated broad review, or
benchmark/full-suite work unrelated to the changed path. Review and broad
verification run once on the stable wave candidate; documentation changes only
when the public contract changes.

### L1a–L7 — mandatory Align library-boundary prerequisites

Land the milestones and closed acceptance cells specified in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md). Follow its
dependency DAG; do not serialize independent L3/L4/L5 work or turn every
internal acceptance cell into a separate PR:

```text
L1a recursive DropPlan framework + Option<string> fields
L1b Move sum/Option/Result payload completion
L2a parameter-mode and borrow/region-summary representation and interface identity
L2b recursive parameter/capture return provenance and function-value joins
L2c cleanup-ABI record and dynamic bit for recursively Move returns
L2d shared borrow over stable bound Copy/Move storage
L2e borrow mut/out, all-peer aliases, Copy/Move replacement, and Pure shaping
L3  package-defined opaque/dependent Move resource + linkable Drop thunk + resource_ref/native views
L4  named arena binding + region + clone_in
L5  deterministic tagged file/inline inputs + one-item Query/command identity + artifacts/descriptors
L6  region-backed PlainStruct array_builder
L7  nested generic package APIs + closed structural RegionPlain bound
```

All focused correctness tests in that plan are gates. Benchmarks are local
measurements run only when their named performance path first lands or changes;
they are not integration, PR, or release gates. No SQLite or PostgreSQL safe
public connection type lands before L3; no Query file support lands outside L5;
no compound-output private vector lands before L6.

The common generic `rows_stmt<P,R>`/`all<P,R>` implementation does not land before L7.

### D0 — native driver feasibility probes

Read-only/throwaway probes establish:

- exact libsqlite3 and libpq library/ABI availability on supported targets, including libpq's
  libssl/libcrypto dependency closure;
- SQLite prepare tail-pointer statement-count behavior;
- SQLite column pointer validity across `step/reset/finalize`;
- PostgreSQL extended-query single-statement behavior;
- libpq full-result and single-row pointer validity;
- parameter/result metadata and nullability actually available from each engine;
- cancellation and cleanup behavior.

#### D0 measured evidence — 2026-08-07

The Q2 author probe compiled the exact consumed C signatures with `-Wall -Wextra -Werror` on
Apple Silicon macOS 26.5.2 and exercised Homebrew SQLite 3.53.3 plus libpq 18.4 against PostgreSQL
18.4. The selected arm64 dynamic libraries were `libsqlite3.0.dylib` with compatibility version 9
and `libpq.5.dylib` with compatibility version 5. Unqualified local `pkg-config` advertised SQLite
3.51.0 while the explicitly selected Homebrew library reported 3.53.3; Q2 must therefore test and
report the linked library's runtime version rather than infer it from discovery metadata. On the
supported targets the package declares libpq's libssl/libcrypto TLS dependency closure explicitly;
it does not rely on transitive linker state to retain those libraries.

The observed SQLite contract was:

- `sqlite3_prepare_v3` compiled with the exact tail-pointer signature and left the tail at byte 74,
  immediately after the first statement terminator; preparing the comment-prefixed tail produced
  the second statement. The package must inspect the complete tail and accept only whitespace and
  comments rather than treating a non-null tail pointer as sufficient.
- A two-row result reported runtime storage classes `INTEGER, NULL, INTEGER, TEXT`. Base columns
  exposed declaration and origin names with `ENABLE_COLUMN_METADATA`; the expression exposed
  neither. The first text pointer remained usable during reads of the current row, the next step
  reused its address for the second row, and `step`, `reset`, and `finalize` are all pointer
  invalidation boundaries. Q2 may cache decoded scalar values before the next step but may not
  retain native column pointers.
- Cross-thread `sqlite3_interrupt` made the active step return `SQLITE_INTERRUPT` (9). Finalizing
  that statement left autocommit enabled and the same connection immediately executed `SELECT 42`.

The observed PostgreSQL/libpq contract was:

- `PQexecParams` rejected `SELECT $1::bigint; SELECT 2` with `PGRES_FATAL_ERROR` and SQLSTATE
  `42601`, confirming extended-query single-statement enforcement in addition to the package's
  static scanner.
- A buffered result retained row bytes while a separate result was obtained and until its owning
  `PGresult` was cleared. In single-row mode, the first `PGRES_SINGLE_TUPLE` bytes remained valid
  while the second result was obtained because the first result remained owned; the terminal result
  was `PGRES_TUPLES_OK` with zero rows and the same two fields. Every pointer becomes unusable when
  its own `PGresult` is cleared.
- A base `bigint` column reported OID 20, nonzero table OID, and attribute ordinal 1. An expression
  reported table OID and attribute ordinal zero. `PQgetisnull` supplied runtime NULL state, but the
  ordinary result API supplied no complete declared-nullability fact. D5 must combine origin and
  catalog evidence and fail closed when that proof is unavailable.
- `PQcancelBlocking` succeeded; draining the connection produced `PGRES_FATAL_ERROR` with SQLSTATE
  `57014`. After all results were cleared, the same idle connection executed `SELECT 42`. Q2 owns
  synchronous cleanup; the later public cancellation surface may reuse the connection only after
  the complete result drain.

The deliverable is recorded evidence in this document or a focused audit, not production raw handles.

### D1 — generated Query/command plans over a fake driver

#### Q1/D1 implementation closure matrix

Q1 is one executable capability. The public package declarations, compiler-produced artifact,
generated binder/decoder plans, QueryMeta plan, and fake-driver consumer share one descriptor ABI and
one cache identity; splitting any producer before its first consumer would publish a static value
that cannot execute or would duplicate the same structural-contract proof.
This capability is deliberately above the roughly 1,000 hand-written-line threshold: keeping the
descriptor, artifact, runtime data, and first consumer together removes dormant seams and proves
one ABI/cache boundary once, which is lower integration risk than repeating that proof across
producer-only PRs.

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| public source surface | Check in `pkg.db`, `pkg.db.sqlite`, and `pkg.db.postgres` Query/command descriptor and D1 option sums with the exact generic signatures. Constructors remain complete single-expression descriptor bodies and expose no raw/native state. File constructors accept both their implicit sibling path and one explicit leading relative path through the compiler-owned constructor signature rule. | `pkg_db_q1::public_surface_whole_and_per_unit` and `pkg_db_q1::file_constructors_accept_explicit_paths_on_the_shipped_surface` |
| typed semantic descriptor | Resolve Query Params/Row and command Params from the concrete generic return type, retain their post-compaction HIR identities, and decode only literal static options. Reject wrong descriptor kind/arity, command Row contracts, unresolved types, duplicate/conflicting options, and runtime option values before publication. | `pkg_db_q1::typed_descriptor_contract_matrix` and `pkg_db_q1::static_option_rejection_matrix` |
| artifact formation | Build the exact versioned Query/command artifact from the resolved static input, reachable structural contracts, source identity/bytes, placeholder occurrences, SQLite identity wire SQL, PostgreSQL `$n` rewrite/spans, binding ordinals/retention, declared metadata plan, ABI versions, and checked-metadata snapshots. A current semantic snapshot becomes `DatabaseChecked`; stale Optional evidence remains Declared and stale/missing Required evidence fails. Validate before publishing bytes. | `pkg_db_q1::artifact_semantics_and_checked_in_goldens`, the checked Query/command metadata promotion tests, plus independent Query/command byte and digest goldens |
| generated runtime data | Resolve canonical type names once during artifact formation into closed value tags, nullability, and declaration-order field ordinals. Emit producer-owned immutable `ALIGNQST`/`ALIGNCST` descriptor data containing the artifact digest, per-driver wire/bind plans, and direct binder thunk plan; Query additionally carries one ordinal decoder thunk plan and the Declared QueryMeta materialization plan. No runtime field-name lookup, source/artifact I/O, map, dictionary, reflection, or consumer-side generic instantiation is permitted. | `pkg_db_q1::generated_runtime_data_is_producer_owned` inspects the exact MIR/LLVM constants and direct ordinal plans; `pkg_db_q1::fake_driver_query_and_command_end_to_end` invokes the generated bind/decode/meta plans in whole and separate-compilation modes |
| fake-driver execution | Execute one inline Query, one sibling-file Query, one inline command, and one sibling-file command through the same generated binder; decode every admitted first-release scalar/nullable Query row shape, count one execution, distinguish Query from command, and report bind/decode/cardinality errors without a database. | `pkg_db_q1::fake_driver_query_and_command_end_to_end` in whole-program and per-unit modes, plus `pkg_db_q1::scalar_bind_and_decode_shape_matrix` |
| interface, implementation, and cache identity | Public Params/Row/restriction/static-option edits change the interface. SQL, rewrite, checked metadata, binder/decoder ABI, and private descriptor edits change producer implementation/artifact identity without recompiling an unchanged public consumer. Query and command use the same statement-artifact identity rules; command omits exactly Row/decoder/QueryMeta. | `pkg_db_q1::interface_impl_cache_invalidation_matrix` |
| fail-closed and Q1 ownership boundary | Reject malformed SQL UTF-8/NUL/statement shape, malformed placeholders, unmatched Params fields, unsupported field types, overflowed offsets/counts, malformed checked metadata, and every artifact corruption before codegen or fake execution. Q1 borrows fake inputs and returns owned fake observations; it owns no native resource yet. Generated plans record each future native driver's exact `BindValue`/`BindCopy` retention without pretending to exercise D2/D4 native cleanup. | `pkg_db_q1::malformed_static_query_matrix`, `pkg_db_q1::inline_nul_diagnostic_points_at_the_exact_source_bytes`, fake-driver invalid-plan/error cases, and the existing static-input/artifact corruption suites |

#### Q1 review finding-to-fix ledger

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| checked snapshots stayed Declared | Preserve the parsed semantic record beside the unchanged L5 input snapshot, compare every artifact-bound field, and derive the exact server/prepare identities before promotion. Optional and Required now take their specified stale/missing branches. | `pkg_db_q1::checked_metadata_promotes_current_snapshots_and_obeys_policy_on_stale_data`, `pkg_db_q1::checked_command_metadata_promotes_without_query_evidence`, and the checked-metadata parser/revalidation suite |
| explicit file paths failed ordinary arity checking | Make the compiler-owned static-constructor signature rule insert the exact leading `str` parameter only for the explicit file form; do not introduce general overloads or duplicate package declarations. | `pkg_db_q1::file_constructors_accept_explicit_paths_on_the_shipped_surface` and `align_sema::static_file_descriptor_preserves_explicit_decoded_path` |
| PostgreSQL escape strings exposed false placeholders | Treat token-boundary `E'...'`/`e'...'` as escape-aware opaque SQL tokens, including backslash-escaped and doubled quotes. | `static_artifacts::tests::scanner_keeps_postgres_escape_strings_opaque` |
| every `WITH` statement was classified Select | Track top-level CTE bodies and classify the first statement keyword after the final CTE, including recursive and multiple-CTE forms. | `static_artifacts::tests::scanner_classifies_the_main_statement_after_ctes` |

The author-side matrix-to-diff pass must point every runtime descriptor field to its artifact
producer and fake-driver consumer, and every accepted Params/Row field class to both a direct
binder/decoder owner and a malformed twin. Q1 reopens this matrix if any descriptor can execute
without validated artifact identity or if generated code falls back to reflection/name lookup.

The D1 private runtime-plan prefix is exact and is not the separately distributed artifact codec.
Both records start with an eight-byte magic (`ALIGNQST` or `ALIGNCST`), `format_version: u32 = 1`,
the descriptor ID as a `u32`-length UTF-8 string, the artifact `Hash128` (`lo`, then `hi`), the
static-option count and records, and the driver count. Each static option stores owner `u8`, value
tag `u8`, and its exact payload: Check policy `u8`, three SQLite version `u32` values, or two
`u32`-length PostgreSQL UTF-8 strings. Each driver is ordered SQLite then PostgreSQL and stores its `u8` tag, `u32`-length
wire bytes, and dense bind fields. A bind field stores Params ordinal `u32`, protocol ordinal `u32`,
retention `u8`, then shape `(kind: u8, nullable: u8)`.
Query then stores dense decoder fields with the same shape, followed by statement-class `u8`, dense
declared parameter rows, and dense declared column rows; command ends after its driver records.
Every text/byte field and sequence is `u32` bounded before descriptor MIR is installed. Artifact
validation and runtime-plan formation complete before codegen, so emitted constants are trusted
producer data; the fake consumer still rejects non-dense ordinals and zero protocol ordinals.

Before a native database:

- construct `db.query<P,R>` from inline and sibling SQL;
- construct `db.command<P>` from inline and sibling SQL;
- emit/load `StaticQueryArtifact` and `StaticCommandArtifact`;
- serialize structural reachable-definition Params/Row contracts/fingerprints and binder/decoder
  ABI versions exactly as L5's versioned artifact schema requires;
- preserve exact source SQL identity while generating deterministic SQLite/source and
  PostgreSQL/`$n` wire entries plus reverse span maps;
- generate the shared Params binder for both kinds and a Query-only decoder thunk;
- generate the producer-owned Declared QueryMeta plan without reflection or runtime artifact/source
  I/O; D12 adds the materialization thunk only with its first consumer;
- record per-driver `BindValue`/`BindCopy` retention classes for every Params field;
- exercise named-parameter occurrence tables;
- decode a fake flat scalar row;
- prove Query and command interface/implementation/cache invalidation boundaries;
- prove command identity, checked-policy state, source/wire/static-option hashes, and binder
  retention use the same versioned statement-artifact schema without a result contract;
- define/encode `db.QueryOption`, `db.CommandOption`, `sqlite.QueryOption`/
  `sqlite.CommandOption`, and `postgres.QueryOption`/`postgres.CommandOption` exactly as §11–§13;
- prove no runtime reflection or per-row name lookup.

Tests mutate SQL-only, private Query/command, same-path Params/Row field name/order/type/Option/
reachable definitions, driver restriction, binder/decoder ABI versions, and per-driver metadata
digests independently. They match the independent byte/digest goldens, materialize a separately
compiled Declared QueryMeta table, compile-fail a command with a Row/decode contract, and prove an
unchanged public command consumer is not recompiled by a SQL-only producer edit. When this path
first lands or materially changes, measure generated Query/command binder/decoder plans, the
QueryMeta plan, and warm-cache behavior locally.

#### Q2/D2+D4 implementation closure matrix

Q2 is one dual-driver capability. The common `execute`/`one` surface, the producer-owned native
descriptor ABI, the connection resource, and both driver consumers land together. Splitting SQLite
from PostgreSQL would let the first driver define common option, error, cardinality, descriptor, and
cleanup behavior without its required portability peer. Splitting the descriptor consumer from the
native packages would publish another dormant compiler seam. The capability is expected to exceed
roughly 1,000 hand-written lines: one coordinated boundary proves the descriptor/thunk ABI,
resource cleanup, common error model, and identical scalar surface once, which is less duplicated
proof and lower integration risk than two driver PRs plus a producer-only bridge.

The Q1 canonical `ALIGNQST`/`ALIGNCST` bytes remain unchanged. Q2 adds a target-native,
producer-owned `QueryStatic`/`CommandStatic` execution header addressed by the descriptor's one
compiler-private raw field. All currently supported database targets are 64-bit. Execution-header
v1 has alignment 8, size 96, and this exact byte layout:

```text
0   u32 abi_version = 1
4   u8  statement_kind       // 0 Query, 1 command
5   u8  driver_mask          // bit 0 SQLite, bit 1 PostgreSQL
6   u16 reserved = 0
8   raw q1_plan
16  str descriptor_id        // pointer at 16, i64 length at 24
32  str sqlite_wire_sql      // pointer at 32, i64 length at 40
48  str postgres_wire_sql    // pointer at 48, i64 length at 56
64  raw binder_thunk
72  raw validate_static_thunk
80  raw validate_row_thunk   // null for command
88  raw decode_row_thunk     // null for command
96  end
```

Both driver slots are always present in SQLite/PostgreSQL order. A driver absent from the mask has
the exact canonical empty slot `(null, 0)`; a present driver has a non-null pointer, positive length,
and one NUL sentinel outside that length. Query requires all four thunk pointers; command requires
binder plus static validation and requires the other two to be null. The compiler checks
mask/slot/kind/thunk/reserved-field agreement before object publication; consumers check version
and kind before their
first load. The header is an in-object relocation-bearing constant, not a persisted codec or
untrusted runtime input. Independent LLVM/object goldens pin every offset, relocation, null slot,
alignment, and total size for masks 1, 2, and 3. The producer-owned Q1 QueryMeta plan remains outside
this execution header. D12 adds a new header version and exact materializer ABI only when its native
metadata consumer lands, rather than publishing a dormant Q2 call edge.

Binder ABI v1 is `fn(context: raw, borrow params: P) -> i32`. The producer treats `context` as an
opaque call-scoped token and may only pass it to the exact package callback
`pkg.db.internal.bind_i64_v1(context, protocol_ordinal: u32, value: i64) -> i32`; it never loads,
stores, retains, returns, or frees that pointer. The package creates and owns the execution context,
validates its private version/driver/ordinal state in the callback, and destroys it after the
synchronous operation. Every status callback returns exactly `0_i32` on success or `1_i32` after
recording the context-owned first failure; it never returns a negative value. A generated thunk
maps any nonzero callback result to `1_i32`, so `-1_i32` remains reserved to the compiler-generated
unsupported-shape path. The package materializes one selected owned `db.Error` before native
cleanup, context Drop frees an untaken record, and the binder stops after the first failure.

`P` may be Copy or Move under the general shared-borrow rule; the generated callback always reads
the caller's stable storage and never creates a by-value aggregate copy. If `P`, or a Query's `R`,
contains a shape outside Q2's closed non-null `i64` subset, `validate_static_thunk` returns exact
`-1_i32` before invoking any field callback. That reserved status does not select or preallocate a
context failure record. After receiving it, the package constructs exactly one owned
`db.Error.Unsupported(db.ContractError { query_id: Some(id), item: "db.descriptor.shape",
message: "static database descriptor uses a field shape unsupported by this execution milestone" })`.
Supported executions allocate no error. Exact `1_i32` selects the context-owned first-failure
record; exact `-1_i32` means only the unsupported-shape case above. No other status is produced by
a valid v1 thunk. Phase 6 fails before lease or native send. Query
binder/row-validator/decoder pointers remain non-null but are unreachable after that failure.
Command row-validator and decoder slots remain null exactly as required by the fixed header.

Static-option validation ABI v1 is `fn(context: raw) -> i32`. It visits the Q1 canonical sorted
option sequence and emits no callback for the common Check option, whose policy/state was already
closed during artifact formation. For SQLite it calls exactly
`pkg.db.internal.require_sqlite_version_v1(context: raw, major: u32, minor: u32, patch: u32) -> i32`;
the package compares the linked `sqlite3_libversion_number()` as a component tuple without arithmetic
overflow. For PostgreSQL `i64` it calls exactly
`pkg.db.internal.set_postgres_i64_type_v1(context: raw, protocol_ordinal: u32,
canonical_type_name: str) -> i32`; Q2 accepts exact `int8`, records OID 20 in the call-scoped
`PQexecParams` type vector, and returns `Unsupported` for every other requested mapping before send.
Exact zero/one status, first-failure recording, opaque-context provenance, and immediate stop match the binder
ABI. Compiler publication validates exact Q1-option/thunk agreement, so no static option is omitted.

Query row-validation ABI v1 is `fn(context: raw) -> i32`. It first calls exactly
`pkg.db.internal.validate_row_count_v1(context: raw, expected: u32) -> i32`, then calls exactly
`pkg.db.internal.validate_i64_v1(context: raw, ordinal: u32, expected_name: str) -> i32` in declared
ordinal order. The per-column callback checks the exact UTF-8 name bytes, then NULL, then the
driver-native representation, then full-range `i64` parsing; on success it caches the parsed scalar
inside the package context. Zero means success. Exact one selects the same context-owned
first-failure record as binding, and the generated validator stops immediately. This catches same-typed column
reordering under `DeclaredOnly` before construction. Decoder ABI v1 is `fn(context: raw) -> R`; it
may call only `pkg.db.internal.read_i64_v1(context: raw, ordinal: u32) -> i64` after successful
validation, then writes direct field offsets. `read_i64_v1` reads only the cached validated scalar
and is otherwise infallible. The context token has the same call-scoped, non-retained provenance for
validation/decode. Changing a header, thunk, or callback calling/layout contract increments the
existing artifact ABI version and the execution-header/callback version.

Only compiler-validated generic bodies owned by `pkg.db` may project this opaque header or invoke
its thunks. The trusted operations are explicit in HIR/MIR, carry the concrete `P`/`R` and complete
function signature, and lower to fixed header loads plus an indirect call. They are not
name/reflection lookups and are unavailable to application source. The backend only lowers this
MIR contract; SQLite/PostgreSQL option, connection, error, lease, bind, step/result, and cleanup
semantics remain ordinary first-party Align package code and direct `sqlite3`/`pq` FFI. No database
semantic helper or handle is added to `align_runtime`.

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| public common surface | Define the exact §4/§6/§13/§15 `db.conn`, `db.exec`, `db.Driver`, `db.exec_result`, structured owned errors, `db.ExecuteOption`, `exec_conn`, `execute`, and `one` shapes plus the SQLite/PostgreSQL `execute_native`/`one_native` forms with separate common/native option slices. `one` retains its explicit output `region` even though the Q2 row is `i64`-only. `db.exec` preserves the settled conn/tx sum shape; Q2 rejects the unconstructable transaction arm without native work until D7. | package whole/per-unit interface golden; compiled common and native command/portable Query on both drivers; compile-fail wrong option scope/arity and escaped execution view |
| namespaced builtin type identity | Generalize type lookup rather than renaming `db.Error`. The closed alias/explicit table is `Error`/`core.Error` (always-in-scope syntactic core), `argon2_params`/`crypto.argon2_params` (`import std.crypto`), and `regex_match`/`regex.regex_match` (`import std.regex`). A non-entry same-module declaration wins bare lookup; a local miss falls back to the alias; exact explicit spelling retains builtin identity; std-qualified type use satisfies the unused-import lint; `error(c)` always targets `core.Error.Code(c)`; entry canonical collisions reject. No package name appears in the rule. | parameterized same-module local signatures/constructors for all three aliases; qualified importer construction/match; exact explicit builtin use and missing-import negatives; unchanged bare builtin fallback; local-Error `error(c)`; entry collision rejection; whole/per-unit interface and executable parity |
| native descriptor ABI | Emit one relocation-bearing `QueryStatic`/`CommandStatic` per descriptor while preserving the exact Q1 plan bytes and fixed 96-byte v1 layout above. Validate version/reserved/kind/mask/slot/thunk and Q1 static-option agreement before object publication; keep the public descriptor a one-pointer Copy value. SQL-only producer edits replace producer constants/object identity without changing an unchanged public consumer interface. Q2 emits no dormant QueryMeta pointer. | exact MIR/LLVM/object header inventory and relocation golden for masks 1/2/3; ABI-version, null-slot, size/alignment goldens; Query/command omission twin; static-option-thunk inventory; QueryMeta-slot absence; two same-typed runtime-selected descriptors; whole/per-unit executable parity |
| generated binder/decoder | Generate direct ordinal `i64` binders for Query and command plus Query-only validation and decoder thunks. The binder treats context as opaque, calls only `bind_i64_v1`, and stops at its first failure. Validation checks exact column count, exact UTF-8 field-name bytes in declared order, NULL, and driver-native `i64` representation before decoder invocation. Decoder uses only `read_i64_v1` and constructs `R` without names, maps, boxing, or reflection. Unsupported Q2 field shapes fail before native send; later mappings remain owned by D8. | portable `CAST(:value AS BIGINT)` binder/validator/decoder on both drivers, repeated PostgreSQL placeholder ordinal, same-typed alias reorder and name/type/count twins on both drivers, callback version/context misuse rejection, unsupported-shape no-send, and generated thunk MIR/LLVM inspection |
| type and monomorph closure | Preserve concrete `P`/`R` through generic instantiation, function-value signatures, interface serialization, whole-program compilation, per-unit compilation, and cache/implementation identity. No unresolved type, wrong header kind, absent decoder, or mismatched thunk signature reaches MIR/codegen. | whole/per-unit Query and command execution; generic mono-key/header-signature golden; malformed HIR/MIR/header rejection owners |
| connection formation and ownership | SQLite/PostgreSQL descendants validate inputs/options, open one physical native connection, allocate one tagged package state, and construct the root `db.conn`. Moving/returning/replacing the owner preserves one state; `db.exec` carries only its generation-checked `resource_ref`; source nulling, branch/loop joins, early `?`, and Drop close/free exactly once. | resource owner matrix for construction, move-in/out, return, replacement, malformed null, early return/`?`, branch/loop joins, use-after-move, and whole/per-unit producer Drop thunk linkage |
| common validation precedence | Apply §13.4's exact phases: fail-closed header/identity, common option source order, native option source order, restriction, connection state, generated static options, then lease. Q2 returns `Unsupported` for a requested common deadline because D9 owns enforcement. Thus malformed header beats every descriptor-dependent error without reading identity; timeout beats mismatch/closed; mismatch beats closed/static-option/overlap; closed beats static-option/overlap. No losing phase acquires state or sends SQL. Contract-error allocations are explicit and counted/freed; only success promises no error allocation. | complete pairwise multi-invalid winner table across every phase and source-order option twins; malformed-header no-identity/no-embedded-dereference owners; no-send/no-lease/native-state counters; exact owned-error allocation/drop counters |
| static descriptor options | Invoke the exact generated static-option thunk after a valid matching open connection and before lease/send. Common Check has no runtime callback because Q1 already closes its artifact policy/state. SQLite `RequireVersionAtLeast` compares linked components and PostgreSQL `ParameterType` maps only Q2 `i64` `int8` to OID 20; every unsupported mapping fails before send. | Check-policy no-runtime-call inventory; SQLite below/equal/above/large-u32 version matrix and no-send; PostgreSQL absent/int8/int4, reordered option and repeated-placeholder OID vectors; malformed Q1-option/thunk publication rejection; whole/per-unit parity |
| SQLite connection options | Implement every §11.2 `sqlite.ConnectOption`, exact `[]` defaults, conflicts, positive-duration rule, NUL-free path, PRAGMA name grammar/value quoting, duplicate PRAGMA rejection, linked-capability rejection, and setup failure. Convert positive nanoseconds to native milliseconds by overflow-safe ceiling division; `1..=1_000_000` ns becomes 1 ms, and a result above `i32::MAX` returns `Unsupported` before SQLite. Validate before open/setup where specified; never silently degrade a flag or PRAGMA. | parameterized option disposition table, duration edges at 1/1_000_000/1_000_001 ns and `i32::MAX` milliseconds, overflow rejection, multi-invalid precedence, open/setup counters, PRAGMA round trip, unsupported-capability injection, and exactly-once failed-open cleanup |
| SQLite execution lease/options | Acquire the one connection-wide lease before bind/timeout/native work. `sqlite.ExecuteOption.BusyTimeoutNs` is positive, unique, uses the same exact ceiling/overflow rule as connection setup, temporarily replaces the tracked native-millisecond value, and restores before synchronous return on success, bind error, prepare/step error, zero/one/>1 cardinality, and Drop unwinding. A second operation fails before reading/restoring the first lease; restore failure poisons/closes. Preserve the first operation error; a later cleanup/restore failure poisons but replaces success only when no earlier error exists. | duration-boundary owners, overlap table, busy-timeout apply/restore counters, failed-second-operation owner, success/error/cardinality/early-`?` cleanup, first-error/cleanup-error precedence, restore-failure poisoning, and execution count |
| SQLite command/query lifecycle | Prepare exactly one statement and require only whitespace/comment tail, bind the `i64`, step commands to completion, reject a command that produces a row, and read nonnegative affected rows. For `one`, zero rows is Cardinality; otherwise validate/cache the first row before probing a second, return that first-row validation error immediately if invalid, return Cardinality after a valid first row when a second exists, and decode the cached first row only after the second probe returns DONE. Never validate the second row. Finalize exactly once before lease restoration/return on every path; primary and extended codes plus owned message survive finalize. A finalize error follows the first-error precedence above. | in-memory insert/select vertical; zero/one/>1; two-row malformed-first validation winner and valid-first/malformed-second Cardinality winner; exact step/validator/decoder counts; command-returned-row, second-statement, fault injection, affected rows, first/finalize precedence, error ownership, direct comparison |
| PostgreSQL connection options | Implement every §12.2 `postgres.ConnectOption`, exact `[]` defaults, source-order semantic-key conflict detection including URL collisions, SSL/target attributes, and arbitrary parameter name/value handling. Convert positive nanoseconds by overflow-safe ceiling to seconds, then apply the cross-version libpq floor: `1..=2_000_000_000` ns encodes 2, `2_000_000_001` encodes 3, and a result above `i32::MAX` returns `Unsupported` before libpq. Encode exact decimal ASCII plus a NUL sentinel. Reject U+0000 before libpq and never place secrets in static artifacts. Reserve direct/`options`-embedded `client_encoding`, append package-owned `UTF8` after all inputs, require `CONNECTION_OK`, then verify `PG_UTF8` before exposing the connection with the exact fallback error above. | option disposition and pairwise/source-order multi-invalid tables; duration edges at 1/1_000_000_000/2_000_000_000/2_000_000_001 ns, libpq floor, overflow no-call; URL/option conflict owners; direct and every accepted `options` spelling of client-encoding rejection; malformed escape; `PGCLIENTENCODING=LATIN1` isolation; null/`CONNECTION_BAD`/OK-encoding-mismatch precedence with encoding-call counters, exact errors, exactly-once close/no-execute; embedded-NUL no-call; unreachable/auth ownership; secret absence |
| PostgreSQL execution options/binding | Implement Text `i64` binding and exact baseline `postgres.ExecuteOption` validation. Unknown/duplicate parameter names, Binary `i64`, and unavailable result formats return `Unsupported` before send; repeated source names reuse one `$n`. The synchronous call owns parameter transport until return. Close the shared bytea codec now: Text produces `\\x` plus two lowercase hex digits per byte and a NUL sentinel outside its recorded length; Binary alone exposes raw bytes with the explicit length. Q2 does not make bytea an executable descriptor shape before D8. | format disposition table, no-send counters, `CAST($1 AS BIGINT)` execution, repeated-placeholder owner, independent Text/Binary bytea byte goldens including embedded zero/high bytes, and parameter buffer allocation/free counters |
| PostgreSQL result/cardinality | Use explicit `BufferedFull`. Query requires `PGRES_TUPLES_OK`. `one` applies the same observable order as SQLite despite knowing the buffered count: zero rows is Cardinality; otherwise validate/cache only row zero first, return its validation error before considering additional rows, return Cardinality when row zero is valid and a second row exists, and decode cached row zero only when the total is exactly one. It never validates row one or later. Check exact column count/name order, NULL, and full-range decimal `i64` parsing before decoder invocation. Command requires `PGRES_COMMAND_OK`, rejects every tuple-producing status as `InvalidQuery`, and parses nonempty `PQcmdTuples` as nonnegative decimal fitting `i64` before `PQclear`; empty means `None`, malformed/negative/overflow is a native failure. Clear each `PGresult` exactly once before connection reuse/return. | Query zero/one/>1; two-row malformed-first validation winner and valid-first/malformed-second Cardinality winner; exact validator/decoder counts and no row-one validation; same-typed reorder/name/NULL/type/range/count; command OK/tuple/status and affected empty/zero/positive/malformed/negative/overflow; clear counters; pointer probe; direct comparison |
| native error ownership | Copy SQLite codes/message and PostgreSQL SQLSTATE/message/detail/constraint/table/column into the exact owned `db.NativeError` before statement/result cleanup. Map stable constraint/serialization/deadlock categories without parsing message text. Native errors contain no Query ID. Only `db.ContractError` carries `query_id`: `Some(id)` after a Query/command identity is trusted, and `None` for Query-less connection input or an invalid header before identity trust. | error-field golden after finalize/clear/connection Drop, SQLSTATE category table, SQLite primary/extended table, contract/native identity-shape twins, malformed-header and Query-less connection errors, and allocation/drop counters |
| FFI/ABI and malformed input | Pin every used SQLite/libpq declaration, enum/status constant, pointer/length signedness, destructor order, and linked library on all supported targets. Check the descriptor raw pointer for null before its first header load; validate the complete fixed header before following any embedded pointer or invoking any thunk. Reject negative/overflow lengths, null-with-positive-length, invalid UTF-8/native text, and malformed header/thunk state before native side effects. | D0 probe record, compile-time C signature probes, Rust/Align declaration inventory, null/header/embedded-pointer malformed boundary tests, ASan/Valgrind-or-platform owner where available, and x86_64/ARM64/macOS CI |
| native library dependency closure | Keep the package's native link contract explicit: `sqlite3` for SQLite, `pq` for PostgreSQL, and `ssl` plus `crypto` for the supported libpq TLS closure. The common dispatch may retain both driver paths, so a SQLite-only call must still link a complete, ordered closure on ELF without relying on transitive `DT_NEEDED` discovery; final linking preserves the discovered list and appends the ordered `pq`/`ssl`/`crypto`/`zstd`/`z` tail even when another unit first requests `crypto` or a suffix library introduces further references. | `pkg_db_q2::common_surface_dispatches_to_sqlite_without_driver_cycle` whole/per-unit executable and ordered link inventory; required PostgreSQL integration on Linux |
| allocation parity | Success for scalar connect/execute/one allocates only the visible connection/execution/native objects and PostgreSQL Text parameter storage required by the driver; no per-row heap allocation, error allocation, runtime dictionary, or artifact/source I/O occurs. Every partial allocation has one owner and one cleanup edge. | allocation/copy counters for success and each injected partial failure, emitted-symbol inventory excluding DB runtime helpers, and package-versus-direct driver measurements |
| required PostgreSQL gate | Add a pinned provisioned `db-postgres` CI job. The job supplies its ephemeral connection through `ALIGN_DB_POSTGRES_URL` and sets `ALIGN_DB_POSTGRES_REQUIRED=1`; missing or unreachable configuration is a failure and the same portable Query runs against both drivers. Local absence alone may report a reasoned skip. Native libraries and server versions are printed as evidence, while the URL is never embedded in source, artifacts, or logs. | required CI script self-test for missing URL, provisioned PostgreSQL job, portable dual-driver integration target, and no unconditional/required-mode skip branch |

The author-side matrix-to-diff pass must map every acquired native pointer, active lease, timeout
override, statement/result, parameter buffer, and owned error string to one cleanup owner on success,
each native error, cardinality exit, early `?`, and Drop. A reviewer finding that changes the header
or thunk strategy, lets a second SQLite operation touch connection-global state, sends PostgreSQL
input before complete validation, or moves driver semantics into the runtime reopens this matrix and
requires the high-risk review path.

#### Q2 plan-review finding-to-fix ledger

| Finding | Root-cause closure | Owner evidence required above |
|---|---|---|
| execution-header offsets varied with driver mask | Fix one 96-byte, 8-aligned layout with both ordered driver slots always present, canonical null/zero absence, exact thunk nullability, and masks 1/2/3 relocation goldens. | native descriptor ABI |
| binder `raw` exposed an undefined cross-unit context layout | Make context an opaque call-scoped token. Generated code calls only versioned package callbacks; package code alone owns, validates, materializes the first failure, and destroys the context. | generated binder/decoder; FFI/ABI and malformed input |
| ordinal decoding could accept same-typed reordered aliases | Add the Query validation thunk and require exact count, UTF-8 field-name bytes/order, NULL, and native representation before decode on both drivers. | generated binder/decoder; PostgreSQL result/cardinality; SQLite command/query lifecycle |
| PostgreSQL command result semantics were absent | Require `PGRES_COMMAND_OK`, reject tuple-producing command results, and parse/range-check `PQcmdTuples` before clear. | PostgreSQL result/cardinality |
| native errors were incorrectly required to carry Query identity | Preserve the settled shape: native errors carry native detail only; contract errors alone carry `Some(query_id)` for a statement subject. | native error ownership |
| zero-allocation validation contradicted owned `ContractError` | Keep no-send/no-lease/no-native-state assertions while explicitly counting and freeing required owned error allocations; retain the no-error-allocation promise only for success. | common validation precedence; allocation parity |
| nanosecond conversion could disable or overflow native timeouts | Use overflow-safe ceiling conversion to SQLite milliseconds and libpq seconds, reject values beyond signed native bounds before the library call, and pin all edge values. | SQLite connection/execution options; PostgreSQL connection options |
| row-validation callback ABI remained implicit | Name and type exact count/i64 callbacks, fix count/ordinal/name/NULL/native/parse order, share the context-owned first-failure status rule, and cache only validated scalars. | generated binder/decoder; FFI/ABI and malformed input |
| Q2 reserved an unused QueryMeta function pointer without a calling contract | Remove the dormant QueryMeta slot; the consumed static-option validator now occupies the fourth Q2 thunk position in the fixed 96-byte v1 header. Keep the Q1 metadata plan inert until D12 introduces its materializer ABI with the first consumer. | native descriptor ABI; type and monomorph closure |
| multi-invalid precedence named tests but no winning rule | Make connection and execution validation phase order plus source-order option ownership normative in §13.4 and require every pairwise winner. | common validation precedence; driver connection options |
| PostgreSQL client encoding could drift from UTF-8 | Reserve direct and startup-option `client_encoding`, append package-owned UTF8 after user inputs, ignore ambient PGCLIENTENCODING, and verify PG_UTF8 before returning the connection. | PostgreSQL connection options; FFI/ABI and malformed input |
| option validation required untrusted descriptor identity | Validate the complete safety header first; malformed state returns one query-less exact header error, and only a validated ID may enter later option errors. | common validation precedence; FFI/ABI and malformed input |
| static descriptor options had no Q2 consumer | Add one generated static-option thunk and exact SQLite-version/PostgreSQL-int8 callbacks before lease/send; preserve Check as an artifact-time decision. | static descriptor options; native descriptor ABI |
| libpq could reinterpret one-second connect timeout as two seconds | Apply the documented cross-version two-second floor after ceiling conversion and pin its boundary. | PostgreSQL connection options |
| post-connect encoding mismatch had no deterministic error | Always return the exact query-less Unsupported contract error, independent of libpq error-buffer state, before exactly-once close. | PostgreSQL connection options; native error ownership |
| QueryMeta thunk ownership drifted across plans | Keep the producer-owned plan in D1 and move only materializer ABI/code plus its execution-header version to D12 in every current ledger and plan. | native descriptor ABI; D12 metadata owner |
| cardinality and row-validation precedence could diverge by driver | Validate/cache row zero first, then detect a second row, then decode only an exact singleton; first-row validation beats multiplicity and no later row is validated. | SQLite command/query lifecycle; PostgreSQL result/cardinality |
| PostgreSQL encoding check could hide a failed connection | Check `PQstatus` first, preserve/own the native connection error for non-OK status, and call `PQclientEncoding` only after `CONNECTION_OK`. | PostgreSQL connection options; native error ownership |
| Q1 prose still required a dormant metadata thunk | Replace the remaining Q1 capability and measurement wording with binder/decoder and QueryMeta plans; D12 alone owns materializer code. | D12 metadata owner; current ledgers |
| global builtin `Error` reservation contradicted module type identity and the settled `pkg.db.Error` API | Make non-entry bare lookup local-first and retain explicit `core.Error`; reject only true entry canonical collisions. Apply the rule to compiler-provided aliases generally, with no database special case. | public common surface; namespaced builtin type identity |
| the first namespace revision contradicted the core and semantic-interface contracts | Update core EN/JA and the L2 interface contract; imported units accept same-spelled local definitions and semantic import resolves them local-first, while producer entry collisions reject before publication. | namespaced builtin type identity; whole/per-unit parity |
| `core.Error` had no legal relationship to the capability-import rule | Make it an always-in-scope language-syntactic-core path for which `import core` is neither valid nor required; std-owned explicit spellings retain normal imports and count for the unused-import lint. | namespaced builtin type identity; explicit import owners |
| `error(c)` could textually bind to a local `Error` | Define the sugar as a direct construction of `core.Error.Code(c)` and test it in a colliding module. | namespaced builtin type identity; error owner |
| the general alias rule named no complete provider map and tested only `Error` | Close the table over `Error`, `argon2_params`, and `regex_match`, including exact provider spellings and parameterized owner coverage. | namespaced builtin type identity; core/std EN/JA |
| the exact binder ABI required borrowing Copy `P`, but shared borrow rejected Copy as redundant | Generalize shared borrow to stable bound Copy or Move places, preserve the pointer-to-caller-storage ABI, and keep temporary rejection. Use the same rule for source, function values, interfaces, generated MIR, and whole/per-unit codegen without a database exception. | generated binder/decoder; type and monomorph closure; language borrow owners |
| Q1 descriptors outside the Q2 scalar subset had no publishable execution header | Make `validate_static_thunk` return reserved `-1_i32` in phase 6 for unsupported `P` or Query `R`, before field callbacks, lease, or send; only after that status does the package construct the exact `db.descriptor.shape` owned `Unsupported` failure. Query keeps non-null but unreachable binder/row/decode thunks; command row/decode slots stay null. | generated binder/decoder; native descriptor ABI; unsupported-shape no-send and supported-success zero-error-allocation owners |

### D2 — minimal SQLite Query vertical

The first native vertical is deliberately exact:

- in-memory SQLite connection as `db.conn`;
- one `db.command<Params>` inserting an `i64`;
- one sibling-file `db.query<Params,Row>` selecting one `i64`;
- Params and Row contain only self-contained scalar fields;
- `execute` and `one`;
- zero/one/more-than-one cardinality behavior;
- structured SQLite primary/extended errors;
- the exact SQLite connection and baseline execution option sums from §11, including every
  conflict/unsupported branch;
- one connection-wide active-execution lease with pre-native overlap rejection and exact
  exhaustion/error/Drop restoration;
- one execution-count hook;
- close/finalize exactly once on success and every error exit.

It does not include text views, `all`, streaming, transactions, migrations, dynamic rows, metadata
catalogs, or later native breadth. A named local measurement compares prepared bind + one-row
decode with an equivalent direct libsqlite3 loop when this path first lands or changes.

### D3 — checked Query metadata core + SQLite

#### Q3/D3+D5 implementation closure matrix

Q3 is one checked/offline capability. The regeneration command, canonical metadata codec,
SQLite and PostgreSQL describers, and normal-build consumer land together. Splitting either driver
would let the first environment define shared path, identity, stale-state, and diagnostic behavior
without its portability peer; splitting the writer from normal compilation would publish metadata
that no checked descriptor can safely consume. The capability is expected to exceed roughly 1,000
hand-written lines because it closes one tool/native/codec/compiler boundary once instead of
duplicating its proof across driver-only and producer-only PRs.

The existing v1 JSON record and `ALIGNMIG`/`ALIGNSID`/`ALIGNSRV`/`ALIGNPRP` streams remain the exact
public contract. Preparation adds no ambient build input: only `alignc db prepare` may open a
database or enumerate the explicitly named migration directory, while normal `check`/`build`
continue to read only the exact derived metadata paths already recorded in the static-input
manifest.

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| command and input grammar | Implement exactly `alignc db prepare ENTRY --driver sqlite|postgres`, repeatable `--query`, `--check`, and the driver-specific environment forms from §16.2. Reject missing, duplicate, cross-driver, unknown, empty, NUL-bearing, and non-UTF-8 inputs before compilation or native work. SQLite accepts exactly database+schema-id or memory with optional migrations; PostgreSQL accepts exactly url-env+schema-id and reads that one environment variable only after complete option validation. | `pkg_db_q3::prepare_cli_input_and_precedence_matrix` with native-open, environment-read, migration-enumeration, and write counters |
| regeneration compile and selection | Compile only the entry-reachable graph in an explicit regeneration mode that enforces the normal descriptor/source/options/type contracts but treats missing/stale checked evidence as output to replace. Sort descriptors by exact UTF-8 ID; apply repeated `--query` as a closed set; reject unknown IDs, duplicate selectors, descriptors that exclude the selected driver, and hash collisions before opening the database. Query and command share the inventory and path rule. | `pkg_db_q3::regeneration_ignores_missing_required_metadata_and_is_deterministic` and `pkg_db_q3::selection_rejects_unknown_and_duplicate_ids_before_native_open` |
| canonical metadata codec | Produce the exact one-line-LF §16.3 JSON bytes with independent SQLite-Query and PostgreSQL-command byte/digest goldens. Production writer, existing fail-closed reader, and a test-only independent encoder must agree for every Option state, escaping class, signed/native ID, dense ordinal, source identity, origin, and nullability tag. Re-encoding a decoded production record is byte-identical; malformed/noncanonical input is rejected without panic or partial publication. | `pkg_db_q3::checked_metadata_sqlite_query_and_postgres_command_goldens` and the static-input malformed codec matrix |
| schema and server identities | Implement the exact `ALIGNMIG` catalog and `ALIGNSID` schema streams plus the derived `ALIGNSRV`/`ALIGNPRP` identities. Migration validation completes before the first SQLite apply and follows the exact immediate-entry/name/version/gap/symlink/UTF-8/NUL/order rules. Database SQLite uses only explicit schema-id; PostgreSQL binds explicit schema-id, reported search path, and canonically sorted extensions. | `pkg_db_q3::schema_identity_goldens_match_an_independent_encoder`, `pkg_db_q3::migration_catalog_validates_before_sqlite_open_and_applies_atomically`, and `pkg_db_q3::migration_catalog_rejects_invalid_names_and_symlinks` |
| SQLite describe | Open only the explicit database or private in-memory target, apply a validated migration catalog transactionally when selected, enforce every `RequireVersionAtLeast` tuple against the linked SQLite components before preparing that descriptor, then prepare every chosen descriptor with its exact SQLite wire SQL. Require one statement/tail, dense parameter count/names, exact result count/names, and supported native declaration/storage mapping. Record only engine-reported origin; nullability remains `Unknown` unless an owned query-level API proves it. Finalize each statement and close the connection exactly once on success and every error. | `pkg_db_q3::sqlite_native_prepare_describes_the_selected_query`, `pkg_db_q3::sqlite_prepare_enforces_static_version_options`, and `pkg_db_q3::migration_catalog_validates_before_sqlite_open_and_applies_atomically` |
| PostgreSQL describe | Reject every ambient `PG*` variable before libpq load, connect using only the selected non-`PG*` environment value, require an open UTF-8 connection, map each supported `ParameterType` only for its matching `i64` or `Option<i64>` field by binding protocol ordinal into the exact `PQprepare` OID vector, prepare/describe each exact `$n` wire statement under a collision-free generated native name, and record dense parameter/result OIDs, names, and reported table/attribute origins. Record server version, ordered search path, and sorted extensions; catalog `NOT NULL` never upgrades arbitrary Query nullability above `Unknown`. Unsupported static type mappings fail before statement preparation. Zero-column Query results match an empty Row; only commands reject native result columns. Deallocate prepared state and finish exactly once on every path. | `pkg_db_q3::postgres_rejects_ambient_connection_defaults_before_native_load`, `pkg_db_q3::prepare_rejects_unsupported_static_options_before_native_work`, `pkg_db_q3::postgres_native_prepare_describes_the_selected_query`, and required provisioned PostgreSQL CI |
| comparison, publication, and failure atomicity | Form and validate every selected canonical record in memory before filesystem mutation. `--check` performs the same native work and exact byte comparison but never writes. Normal mode writes only changed records through same-directory temporary files and atomic replacement after the complete batch succeeds; no failed batch leaves a partial driver set. Existing unselected files are untouched. | `pkg_db_q3::schema_identities_and_publication_are_exact_and_check_is_read_only` plus the selection and native-failure owners above |
| offline compiler consumption | A subsequent normal build promotes only exact current driver evidence to `DatabaseChecked`; Optional missing/stale stays Declared, Required fails per permitted driver, and `AnySupportedDriver` requires both files. One shared publication guard covers the complete whole-program or multi-unit build, so no build can combine metadata generations. SQL/options/Params/Row/schema edits invalidate the exact producer/cache identity. Checked native evidence never removes runtime name/type/NULL validation and normal builds perform zero environment, network, database, or directory-enumeration work. | `pkg_db_q3::generated_metadata_is_consumed_offline_and_stale_required_evidence_fails`, `static_inputs::tests::metadata_publication_lock_closes_first_publish_and_overlap_races`, and the cumulative Q1/Q2 runtime validation owners |

The author-side matrix-to-diff pass must map every emitted JSON field to one artifact input or
driver-owned observation, and every native handle/allocation to its success, early-error, and Drop
cleanup edge. Q3 reopens this matrix if regeneration needs an ambient input, if a partial batch can
publish, if either driver infers `No` nullability from catalog state, or if normal compilation can
contact a database.

Q3's checked-in native evidence matrix is:

| Driver | Required environment | Owned observation | Fail-closed rule | Owner |
|---|---|---|---|---|
| SQLite | macOS arm64 Homebrew SQLite 3.53.3; Ubuntu CI system SQLite ABI 3 | prepare tail, parameter/result order and names, declaration/origin APIs, migration transaction, runtime library version | expression declarations and query nullability remain unavailable; record `null`/`Unknown` and retain runtime storage/NULL validation | `pkg_db_q3::sqlite_native_prepare_describes_the_selected_query`, `pkg_db_q3::migration_catalog_validates_before_sqlite_open_and_applies_atomically` |
| PostgreSQL | required CI PostgreSQL 16.4 with libpq ABI 5; client version printed by CI | UTF-8 connection, prepared parameter/result OIDs and names, table/attribute origin, search path, extensions, server/client versions | only the closed §10.3 OID mapping is accepted; catalog `NOT NULL` never upgrades result nullability above `Unknown` | `pkg_db_q3::postgres_native_prepare_describes_the_selected_query` |

The macOS PostgreSQL test may skip with a reported reason when no server URL is configured. The
required `db-postgres` CI job sets `ALIGN_DB_POSTGRES_REQUIRED=1`, provisions PostgreSQL 16.4, and
turns that absence or an unreachable server into failure.

The first full-diff review reopened the Q3 matrix for three deterministic-state gaps. Their closure
is one finding-to-fix set:

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| ambient libpq defaults | Require one complete URL with explicit user, password, single host, port, and database before library load; reject target/startup overrides, append package-owned `client_encoding=UTF8`, and override `PGOPTIONS` with an empty startup-option sequence through `PQconnectdbParams`. | `pkg_db_q3::postgres_rejects_ambient_connection_defaults_before_native_load` and required PostgreSQL CI |
| per-statement schema drift | SQLite establishes one read transaction by reading `sqlite_schema`; PostgreSQL establishes one read-only repeatable-read transaction. Each remains open across environment capture and every selected describe and is released by connection Drop. | SQLite native/migration owners and `pkg_db_q3::postgres_native_prepare_describes_the_selected_query` |
| mixed filesystem generations | The persistent implementation-owned `.align-db/.publication.lock` file is not a build input. Normal readers hold its shared OS lock across the complete metadata snapshot; publication holds the exclusive lock across comparison, staging, replacement, and rollback. A reader that predates the first lock file rejects if that file appears during resolution. Process exit releases either lock automatically. | `static_inputs::tests::metadata_publication_lock_closes_first_publish_and_overlap_races`, `pkg_db_q3::schema_identities_and_publication_are_exact_and_check_is_read_only`, and offline whole/per-unit consumption |

The required second full-diff review found that the first connection closure enumerated URL keys but
did not close libpq's independent environment lookup, and that regeneration hashed but did not apply
native static options. This is a boundary redesign rather than another keyword patch:

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| ambient libpq target selectors | Reject a `--url-env` name beginning with `PG` and reject the presence of every ambient `PG*` variable before library load. This closes current selectors such as `PGHOSTADDR`, `PGTARGETSESSIONATTRS`, and `PGLOADBALANCEHOSTS` plus future libpq additions without mutating process-global environment state. The complete URL, package UTF-8, and empty startup-option rules remain required. | `pkg_db_q3::postgres_rejects_ambient_connection_defaults_before_native_load` and required PostgreSQL CI |
| prepare/runtime static-option drift | Apply the complete Q3 native-option set during regeneration: SQLite compares every requested version tuple before statement preparation, while PostgreSQL maps only the supported `int8` contract to OID 20 by protocol ordinal and passes the dense vector to `PQprepare`; unsupported mappings fail before statement preparation. | `pkg_db_q3::sqlite_prepare_enforces_static_version_options`, `pkg_db_q3::postgres_native_prepare_describes_the_selected_query`, and required PostgreSQL CI |
| per-unit snapshot scope | Retain one outer shared publication guard from the first static producer through the complete per-unit walk and validate a legacy absent-file guard after the last unit. Per-unit resolution keeps its inner snapshot check, but publication cannot interleave generations between producer units. | `pkg_db_q3::generated_metadata_is_consumed_offline_and_stale_required_evidence_fails` and `static_inputs::tests::metadata_publication_lock_closes_first_publish_and_overlap_races` |

The required redesign re-review found two local closure errors. Both close against their exact owners
without changing the strategy:

| Finding | Root-cause closure | Owner evidence |
|---|---|---|
| native type accepted without its logical pair | Resolve the named Params field before native work and accept the Q2 `int8` mapping only for `i64` or `Option<i64>`. The same validation precedes both connection/environment capture and OID-vector construction. | `pkg_db_q3::prepare_rejects_unsupported_static_options_before_native_work` |
| zero-column Query classified as a command | Query versus command is the artifact discriminator, not whether the native column vector is empty. Both describers accept zero columns for an empty Row; only a command with native result columns is rejected. | PostgreSQL zero-column case in `pkg_db_q3::postgres_native_prepare_describes_the_selected_query` and exact result-count validation |

- canonical `.align-db/sqlite` artifact;
- `alignc db prepare` and `--check`;
- exact derived path, canonical fail-closed JSON, independent byte/digest golden, and
  producer-owned checked QueryMeta evidence;
- explicit temporary/in-memory schema setup with `ALIGNMIG`/`ALIGNSID`
  catalog/order/fingerprint goldens;
- Declared/CheckedOptional/CheckedRequired;
- the §16.3.1 SQLite origin/nullability evidence matrix, with ambiguous and outer-join results
  remaining `Unknown`;
- stale SQL/options/type/schema diagnostics;
- normal-build no-network test;
- runtime SQLite storage-class and NULL validation.

This milestone lands before promising that a typed Query is database-checked.

### D4 — minimal PostgreSQL Query vertical

- PostgreSQL `db.conn` over libpq;
- the same common Query module shape as D2;
- explicit `BufferedFull` delivery observation; `one` decodes at most two rows but transport may
  contain the complete result;
- dialect-aware named-source scan and `$n` rewrite;
- repeated source name reuses one ordinal;
- scalar bind/decode;
- install the exact §10.3 first-release mapping table and reject every unowned mapping; the
  executable D4 vertical remains `i64`, while D8 owns the complete runtime type matrix;
- reject U+0000 in SQL, Text Params, URL, and connection option strings before libpq;
- encode Text-format `bytea` as exact lowercase PostgreSQL hex and pass raw `bytea` only in Binary
  format with an explicit length;
- SQLSTATE and owned native error detail;
- the exact PostgreSQL connection and baseline execution option sums from §12, including every
  conflict/unsupported branch;
- driver restriction before SQL send;
- portable `CAST(:value AS BIGINT)` Query exercised on both drivers;
- execution-count hook and cleanup tests.

Local developer runs may skip with a reported reason when no PostgreSQL URL is configured. D4 cannot
merge, and no database release can pass its gate, on such a skip: a required `db-postgres` CI job
provisions a pinned ephemeral PostgreSQL version, sets `ALIGN_DB_POSTGRES_REQUIRED=1`, and turns
missing/unreachable configuration into test failure. The same non-skippable job runs the portable
common Query against SQLite and PostgreSQL. The direct-libpq comparison benchmark is
local mandatory evidence when D4 first lands or that execution path changes; it
is never part of an unrelated PR, integration suite, or database release gate.

### D5 — PostgreSQL checked metadata

- canonical `.align-db/postgres` artifact;
- the same exact JSON/path/derived-identity codec and independent PostgreSQL command golden;
- engine version, search path, extension, and schema fingerprints;
- type names plus OID evidence;
- the §16.3.1 PostgreSQL origin/nullability evidence matrix, including proof that catalog `NOT NULL`
  alone remains `Unknown` through arbitrary result expressions and outer joins;
- `--check` reproducibility across equivalent recreated schemas;
- runtime describe comparison.

### D6 — prepared statement lifecycle

- typed dependent `db.stmt<P,R>` resource;
- connection binding and driver mismatch checks;
- the exact common/SQLite/PostgreSQL prepare option sums from §11–§13 and their disposition tests;
- `rows_stmt` with a `borrow mut` statement parameter and repeated sequential execution after each
  rows Drop;
- text/blob rebind replaces the previous transient native copy only after the prior rows Drop;
- partial-bind failure clears every installed binding and drops moved Params exactly once;
- finalize/close on Drop and errors;
- no implicit global statement cache.

Measure direct prepared execution, common-layer execution, and re-prepare cost separately when
this path first lands or changes.

### D7 — transaction and common execution view

- `db.begin` consumes `db.conn`;
- `db.exec_conn` and `db.exec_tx` return the same borrowed type;
- `db.commit`/`db.rollback` consume `db.tx` and return `db.conn`;
- Drop rollback closes the connection;
- the exact common/SQLite/PostgreSQL transaction option sums from §11–§14;
- SQLite begin modes;
- PostgreSQL isolation/read-only/deferrable combinations and pre-BEGIN conflict rejection;
- use-after-end and conn/tx alias rejection.

### D8 — typed row streaming

- dependent `db.rows<R>` resource;
- `next(borrow mut rows)` generation invalidation;
- owner-tied `resource.view_from_raw` with invalid pointer/length/UTF-8 rejection;
- SQLite text/blob Params source Drop/mutation after `rows` returns and before first `next`;
- transient bind copied-byte/allocation/partial-error cleanup counts, with no per-row parameter copy;
- complete runtime bind/decode/nullability tests for every exact first-release common type mapping;
- SQLite current-row views;
- SQLite ordinary/timeout stream overlap rejection in both directions, failed-second-attempt
  non-restoration, and first-stream exhaustion/error/Drop lease cleanup;
- PostgreSQL `BufferedFull` path first; `rows` is one-pass decode over that owned result, while
  explicitly selected single-row/portal delivery is D13;
- early Drop/finalize;
- local one-million-row scalar and borrowed-text measurements when the streaming path lands or changes;
- compile-fail tests for use-after-next, storage in a longer-lived builder, return, branch, and loop.

### D9 — scoped native options, deadline enforcement, and cancellation cleanup

- complete the common deadline/cancellation machinery over the already-owned D1/D2/D4/D6/D7
  option APIs;
- audit common versus driver-qualified entry points across connection/Query/prepare/execute/
  transaction scopes;
- complete the cross-scope applied/unsupported/conflicting and precedence matrix;
- no-silent-ignore tests;
- SQLite busy/locking controls;
- SQLite pre-send Unsupported disposition for common deadlines, PostgreSQL deadline/cancel path,
  and proof that native BusyTimeout is not treated as a common deadline;
- explicit proof that no public external cancel resource exists in v1;
- statement/result cleanup under cancellation, including drain-and-resynchronize or poison/close
  before any connection reuse.

### D10 — compound Output

- canonical Pure state-transition step plus one visible Query-local rows loop;
- `region` output and `clone_in`;
- one parent plus one-to-many output;
- repeated-parent consistency and null-child rules;
- segmented many-parent output;
- transaction + master projection;
- User + Groups;
- exactly one SQL execution;
- no hidden sort/hash/materialization;
- correctness-pinned builder allocation/copy counts and a local high-fanout measurement.

Compound Query support is part of the first product contract, not a later ORM enhancement.

### D11 — SQL migrations

- SQL migration discovery/order;
- the exact `ALIGNMIG` catalog and `ALIGNSID` schema-identity byte/digest goldens shared with D3;
- driver-specific multi-statement execution rules;
- checksums/history/status/check;
- exact first-line `transaction=required|forbidden` policy;
- required-by-default atomic behavior;
- one-statement forbidden files, dirty `Applying`/`Failed` states, and checksum-bound repair;
- reject U+0000 in every migration before applying the first file;
- explicit `alignc db migrate/status/check/repair`.

Migration implementation reuses connections/resources but does not change typed Query semantics.

#### Q5a/D11 implementation closure matrix

Q5a is one external-state capability spanning CLI, canonical catalog reuse, driver-owned
screening/locking, persistent history, migration execution, inspection, and explicit repair. SQLite
and PostgreSQL land together so history/state/repair semantics cannot drift by driver. This is the
deliberate mutation half of Q5; D12 remains an independent read-only capability.
The capability is expected to exceed roughly 1,000 hand-written lines because it includes two FFI
adapters and their fault/concurrency owners. Splitting by driver or command would leave a dormant
producer/consumer seam or duplicate the same persistent-state proof; one shared state machine with
thin driver adapters produces less duplicated proof and lower integration risk.

| Closure cell | Required implementation closure | Exact owner evidence |
|---|---|---|
| CLI and target identity | Implement the exact migrate/status/check/repair forms and validation precedence in §17.6. Resolve entry, project-relative catalog, and relative SQLite target without cwd discovery; read only the selected non-`PG*` PostgreSQL URL variable after all non-secret validation. | `pkg_db_q5a::migration_cli_rejects_invalid_forms_before_catalog_environment_or_native_work` and path/symlink matrix |
| catalog and policy screening | Reuse the Q3 `ALIGNMIG` catalog bytes/digest. Parse the exact first physical line, count complete driver statements, and reject empty, transaction-control, NUL, invalid UTF-8, and multi-statement Forbidden files before target mutation. | cumulative Q3 catalog goldens plus `pkg_db_q5a::migration_policy_and_statement_screening_is_exact` |
| history codec/state reconciliation | Create only the exact §17.6 owned objects during migrate, validate the complete persistent/session-local history-table and attached-object inventory plus every row/state combination, reconcile in version order, and classify the complete current/extra/mismatch/dirty matrix without panic or silent upgrade. One joined PostgreSQL catalog query owns the table invariant; unrelated schema objects are excluded. Reject malformed schema or row state before mutation. | `pkg_db_q5a::migration_history_state_matrix_is_fail_closed` for both drivers, including SQLite TEMP-trigger/shadow/inbound-FK and PostgreSQL user/inbound-FK trigger, rule, RLS, default, index, and table/column ACL negatives |
| overlap exclusion and cleanup | Every SQLite command may create then holds the exact persistent OS-lock inode; every PostgreSQL command holds the exact advisory key across the operation. After those cooperating locks, SQLite read snapshots/`BEGIN IMMEDIATE` and blind-first-SQL PostgreSQL `READ COMMITTED` SHARE ROW EXCLUSIVE/ACCESS EXCLUSIVE table locks make validation plus history access atomic against non-cooperating DB writers. SQLSTATE-bound rollback/bootstrap handles an absent table without an existence race. Forbidden isolates user SQL on a worker that never mutates history. Every success/error/Drop/process-loss edge releases worker, native transaction/table lock, and operation lock in that order. | SQLite absent-lock creation, external writer race, TEMP-trigger, and two-process owners; required PostgreSQL concurrent-session/external-DDL-DML/bootstrap-race owners |
| Required execution | Under the migration lock, run each Required file and its Applied history insert in one transaction. Statement/history failure rolls back that complete file; an uncertain commit closes/reconnects/relocks and classifies exact Applied versus absent without same-invocation retry. Already Applied current prefixes are not re-executed. | SQLite and required PostgreSQL atomic/multi-statement/error/restart/outcome-unknown owners |
| Forbidden execution and dirty state | Require one screened statement and durably observe Applying(0) plus its exact history snapshot before executing outside a transaction. Under the final native lock compare and restore that snapshot, recording Applied(1) or Failed(0) only when the observed snapshot was unchanged. Any row change or absent owned history object restores Applying and fails visibly, while a malformed replacement fails closed. Native error best-effort records Failed(0); uncertain final publication reconciles as Applied or dirty Applying. Either dirty state blocks continuation and is never retried automatically. | both-driver before/after/error-recording/history-forgery/outcome-unknown fault matrix and execution counters |
| status/check | Create at most the operational lock file and perform no schema/history creation, migration, or repair. Emit exact ordered rows/summary with catalog/history provenance; status succeeds after inspection while check succeeds only for one exact current Applied set. Missing history is empty; missing SQLite target is an input error. | `pkg_db_q5a::status_and_check_are_read_only_and_ordered` for empty/current/compound-mismatch/dirty/history-only states |
| repair | Require one current version, one action, and exact lowercase checksum matching argv, catalog, and dirty history. Accept records Applied with the screened count; clear removes only the dirty row. Applied, absent, stale, or mismatched rows change nothing. | `pkg_db_q5a::repair_is_dirty_and_checksum_bound` for both actions/drivers |
| secrets, allocation, and diagnostics | Own copied native errors/history strings before result cleanup, never print a PostgreSQL URL/value, bound all native counts before allocation, reject malformed rows rather than indexing/panicking, and close every statement/result/connection exactly once. | malformed native/history owners, URL redaction owner, self-review FFI checklist, required PostgreSQL CI |

The author-side matrix-to-diff pass must point each cell to one implementation owner and test, plus
the local 10/100/1000 catalog/history measurement because D11 explicitly promises a scaling record.
No normal compiler path imports or calls the D11 module.

The pre-implementation adversarial review closed these root-cause classes before source work:

| Finding | Contract closure |
|---|---|
| P1 absent-lock readers could race a first writer | Every command atomically creates/opens and locks the same persistent SQLite inode; read-only refers to the database/history, with the operational lock as the sole filesystem write. |
| P1 commit response loss was incorrectly called rollback/dirty | Required and Forbidden publication use one fresh-connection history reconciliation with exact permitted outcomes and no automatic same-invocation retry. |
| P1 columns/checks did not exclude behavior-changing attached objects | The SQLite schema-row inventory and PostgreSQL history-table relation/index/constraint/trigger/rule/RLS/ACL inventory are now closed and fail-closed. |
| P2 compound mismatches had no total order or value provenance | Name, checksum, policy, native state is one total order; every output row carries always-present catalog and history fields with exact unavailable markers. |
| P2 validation phases had unspecified internal winners | Token, field, directory, file, statement, connection, schema, and history traversal/error precedence is now exact. |
| P1 revised review found SQLite connection-local behavior outside `sqlite_schema` | The matrix was reopened around persistent plus session-local modifiers; exact `main` qualification and `sqlite_temp_schema` rejection close TEMP trigger and shadowing paths. |
| P1 focused continuation found a validation-to-DML race with non-cooperating DB writers | The matrix was redesigned around two lock layers: OS/advisory locks serialize Align, while SQLite transactions and PostgreSQL table locks cover native validation/history access; Forbidden worker connections never perform history DML. |
| focused lock review included unprotected schema-wide state and a racy PostgreSQL existence branch | The invariant now contains only history-table-attached behavior validated by one catalog query; blind first-SQL locking plus SQLSTATE-bound rollback/bootstrap removes the existence race. Worker-local state is explicitly outside the history-connection invariant, while Applying remains an explicit no-retry dirty state requiring repair. |
| final inspection review found PostgreSQL `SHARE` compatible with index creation | Inspection uses `SHARE ROW EXCLUSIVE`, which permits readers while conflicting with DML and ordinary/concurrent index or DDL modes. |
| author-side implementation pass found Forbidden user SQL could erase or forge its own Applying row before final validation | The runner now retains the exact pre-worker history snapshot, compares and restores it under the final native lock, and leaves restored Applying plus a visible failure after any row change. An absent owned table is recreated only to restore Applying and fail; a malformed replacement remains a blocking error. |
| focused implementation review found malformed rows were treated as restorable changes and uncertain restore commits checked only the current Applying row | Both adapters now validate complete row semantics before any restore and leave malformed replacements untouched. Every Applying commit reconciliation compares the complete expected history snapshot, so a partial or unapplied restore cannot be reported as exact. |
| Align compiler self-review found the public native PostgreSQL entry relied on CLI-only input checks, inherited preparation-specific diagnostics, missed two `SET ... TRANSACTION` spellings, and omitted inbound-FK triggers and column ACLs from attached behavior | The native entry now repeats ambient/complete-URL validation before libpq load through context-specific shared validators. Screening rejects `SET LOCAL TRANSACTION` and `SET SESSION TRANSACTION`. The joined inventory counts all table triggers and non-owner column ACLs, with owner tests. |
| full-diff review found token-free SQLite tails were rejected and unchanged Forbidden history was destructively rewritten | SQLite screening now ignores token-free trailing bytes after checking every complete statement. Both adapters update an unchanged Applying row in place and restore only a changed or missing snapshot; SQLite rejects inbound foreign keys so a necessary restore cannot cascade into application data. |
| required PostgreSQL CI found first-run bootstrap treated only an absent table as missing history and the reached inventory query relied on ambiguous internal-`"char"` concatenation | Every blind history lock now recognizes both PostgreSQL absent-object SQLSTATEs: `42P01` for a missing table and `3F000` for a missing schema. The following exact inventory query still decides whether both owned objects are absent, and casts every internal-character discriminator to text before building a signature. |

### D12 — category-specific metadata and EXPLAIN

Common categories:

- database;
- schema/attached database;
- table/view;
- columns;
- primary/unique/foreign keys and constraints;
- indexes;
- static Query parameters/results;
- explicit EXPLAIN.

D12 owns the exact common/SQLite/PostgreSQL metadata and EXPLAIN option sums from §11–§13.

Each common category operation has one explicit destination `region` and one `MetaOption` slice.
Each driver-native form has the same destination, that common slice, and one separate native option
slice; neither has an optionless or hidden-heap overload. Common strings and flat arrays use the
exact `DatabaseMeta`/`SchemaMeta`/`TableMeta`/`ColumnMeta`/`KeyMeta`/`IndexMeta`/`QueryMeta`/
`QueryPlan` shapes in §18.

Native detail:

- PostgreSQL OIDs/types/opclasses/index details/JSON plans;
- SQLite STRICT/WITHOUT ROWID/hidden columns/index origin/query-plan details.

Tests prove that one requested category does not fetch unrelated categories; every returned string
survives native-result cleanup until its destination arena ends; category arrays allocate only in
that arena; every §18.2.1 category/detail/entry projection, ordering, ordinal base, unavailable
field, and artifact digest is exact; multi-term keys/indexes remain flat ordered rows; and
`meta_table` reports `NotFound`. Both drivers reject U+0000 in every `SchemaRef`/`TableRef`
component with the exact Query-less Encode item before any native/catalog request. The matrix
includes Declared/checked/unknown-nullability cells, a Query with both Parameters and Columns, a
TableRef whose schema and name are both invalid, and two same-named constraints whose canonical
`key_ordinal` groups are pinned even when their terms match and only their action/deferral policy
differs. Contradictory native policy rows fail instead of acquiring an ordinal. An unnamed
constraint returns `name = None`.
The ordinary positional call example in §18.2 is parsed/formatted by the documentation example
test; the exact signature-notation block is compared with the owning API table. A separately
compiled Query's metadata rows come from its producer-owned plan/thunk with zero runtime
source/artifact reads.
Query-specific metadata/EXPLAIN contract errors preserve `Some(query_id)`, while Query-less
category/operation validation records `None`. `EXPLAIN ANALYZE` remains a visibly executing
operation.

### D13 — batch, SoA, and high-value native paths

- bounded `next_batch` and batch generations;
- PostgreSQL binary parameter/result formats;
- segmented child buffers and nullable validity bitmaps;
- direct eligible `soa<Row>` decode with no intermediate AoS;
- a separately specified owned-Row/owned-collection materializer only if a measured consumer needs
  `string`/dynamic-array Row storage; it must not weaken the v1 `RegionPlain` path;
- PostgreSQL COPY/pipeline/single-row/LISTEN-NOTIFY;
- SQLite backup/incremental blob/FTS helpers;
- explicit pool package;
- additional drivers only after the common contracts are proven.

No roadmap item may add hidden relationship loading or a Query-builder DSL.

### D14 — dynamic SQL and native callbacks

- settle and implement the minimal owned `db.value` sum and indexed `db.row`;
- require a visible dynamic SQL string, explicit value slice, exact `db.Driver` restriction, and
  execute-option slice; reject driver mismatch before SQL send;
- reject U+0000 in dynamic SQL before SQL send;
- keep dynamic execution separate from typed Query descriptors and checked artifacts;
- decode dynamic rows by indexed access only, with no struct reflection or name-based field writes;
- add SQLite function/collation and PostgreSQL notice/COPY callback surfaces only after the
  callback-capture, abort, reentrancy, and thread rules are proved;
- pin statement count, value allocation, callback lifetime, and cleanup behavior.

This capability follows the typed Query product. It is not permission to route typed Query
execution through a reflective dynamic engine.

### Initial release gate

The D labels define acceptance ownership. Publication follows the delivery
waves above:

```text
prerequisite gate -> Q1 -> Q2
                           +-> Q4a reusable -> Q4b streaming -> Q6 compound --+
                           +-> Q3 checked/offline -+-> Q5a migrations -------+-> initial release
                                                   +-> Q5b metadata/EXPLAIN -+

P0 runs in parallel before the first native product wave.
```

Q2 closes D2 and D4 together so the common API cannot drift between drivers.
Q3 closes D3 and D5 together for the same reason. Q3 and Q4a start in parallel
after Q2. Q4a closes D6/D7 prepared and transaction reuse; Q4b closes D8/D9
streaming and cancellation resilience. Each publishes and reviews one useful
capability. Q5a and Q5b may proceed in parallel after Q3; Q5a also consumes
the shared migration identity owned by D3. Q6 follows Q4b. The release gate
still waits for every required D1–D12 acceptance cell.

The first release presented as Align database support requires L1a–L7 and D1–D12 for both SQLite
and PostgreSQL where the milestone is driver-relevant. D11 supplies SQL migration lifecycle and D12
supplies the required category metadata and explicit Query-plan access; neither is omitted from the
release whose earlier sections promise those surfaces. D13–D14 remain committed additive work for
batch/SoA/native breadth, dynamic SQL, and proved callbacks.

For completion reporting, the first release and the complete committed roadmap
are distinct. The first release is L1a–L7 plus D1–D12. The complete committed
`pkg.db` roadmap additionally requires D13 and D14. D13 follows the typed
streaming/cancellation/compound paths. D14 follows both driver verticals and
the proved cancellation/callback rules; it has no dependency on D13.

A release that handles only single-table model loading is incomplete even if CRUD works. At least one
many-to-one/master projection and one one-to-many compound Output must be end-to-end,
execution-count pinned, and documented.

---

## 24. Acceptance criteria

The design is implemented correctly only if all are true:

1. SQL is visible and remains the source of relational behavior.
2. The primary public unit is a named Query module.
3. Same-basename `.align`/`.sql` pairing works without path strings at call sites.
4. An explicit relative SQL linkage remains available.
5. A typed Query has explicit `Params` and exact flat `Row` contracts.
6. A Query may expose a logical `Output` built by ordinary Align code.
7. One named Query executes one SQL statement once.
8. Shaping cannot issue SQL and is one-pass by default.
9. No field access triggers I/O.
10. SQLite and PostgreSQL both implement the initial common surface.
11. Common-only code keeps the same Align interface across both drivers.
12. Driver-specific controls are available for connection, Query, prepare, execute, transaction,
    metadata, and explain.
13. Unsupported native options fail rather than being ignored.
14. Normal builds are offline.
15. Database-checked metadata is explicit, hashed, and stale-safe.
16. SQL NULL maps to `Option` and no ambiguous implicit conversion occurs.
17. Typed row mapping does not use runtime reflection.
18. Streaming/materialization/allocation modes are visible.
19. Borrowed result views cannot outlive their row/batch/result buffer.
20. Migrations remain SQL.
21. Metadata is category-specific and opt-in, including keys and indexes.
22. Query plan retrieval is explicit, and ANALYZE visibly executes the statement.
23. Compound examples include transaction+master and User+Groups.
24. Tests pin SQL execution count and no hidden follow-up Queries.
25. Package overhead remains locally measurable against direct native-driver loops; the
    measurement is not a PR, release, or milestone gate.
26. All handles use the general opaque-resource/borrow machinery; no `pkg.db`-named ownership rule
    exists in the compiler.
27. Caller-selected materialization uses `arena name {}` and `region`, with no ambient allocator.
28. SQL-only edits invalidate the producer/query artifact without needlessly recompiling unchanged
    consumers.
29. Canonical compound shaping uses Pure `borrow mut` state and separate-builder transitions, so
    transitive database I/O is rejected; its Query-local orchestrator contains one visible rows
    loop.
30. Region builder allocation and its single compacting pass remain locally measurable and visible.
31. Structured owned `db.Error` and Move Output use ordinary recursive `Option`/`Result`/sum Drop;
    the successful path allocates no error storage.
32. `pkg.db`, `.sqlite`, and `.postgres` are acyclic modules in one `pkg/db` package subtree, not
    falsely independent package versions.
33. Contextual `borrow`/`out`/`resource` parsing accepts every canonical signature and intrinsic.
34. Mutable borrowing Copy state and indirect borrow-mode calls preserve caller mutation and ABI.
35. Imported resource Drop links through the producer thunk and executes exactly once.
36. Each descriptor contains one whole-body constructor and owns one unique artifact/thunk slot.
37. Arena-built compound arrays enter Output inline and never cross an ordinary by-value call.
38. Indirect borrow-returning calls conservatively retain every possible target owner/region.
39. Inline SQL has an item-based tagged source identity and maps diagnostics to its `.align` literal.
40. SQLite migration-backed prepare validates and fingerprints one numeric order before execution.
41. Unresolved higher-order calls retain provenance embedded in compatible by-value Move inputs.
42. Resource Drop hooks use normal fully qualified module paths; no resource-only lookup exists.
43. PostgreSQL `BufferedFull` reports full transport/buffering separately from the two-row decode
    limit of `one`/`maybe_one`.
44. L1a admits only `Option<string>` field leaves; L1b alone admits `Option<MoveStruct>`.
45. Dependent resource construction and checked owner-tied native views remain explicit typed MIR
    operations through lowering.
46. `borrow mut` rejects every overlapping peer argument, including recursively provenance-bearing
    by-value Copy/Move values.
47. Every execute/result/prepare primitive has exactly one option-bearing signature; `[]` means no
    options and no optionless overload exists.
48. Canonical examples bind `rows`/`stmt` as `mut` when their callee parameter is `borrow mut`, and
    never put a parameter mode at a call site.
49. English and Japanese prepared-statement examples type-check against the same signature.
50. The first public database release completes driver-relevant D1–D12; the complete committed
    roadmap additionally completes D13 and D14.
51. `rows`/`rows_stmt` release all source Params provenance at return; SQLite v1 uses measured
    transient text/blob bind copies and permits source invalidation before first `next`.
52. Dynamic SQL names one exact `db.Driver` in source and checks it before sending SQL.
53. Verified core signature tables contain only shipped forms; L4/L6 forms are marked required but
    unimplemented until their owning PRs land.
54. Every category metadata primitive has one `MetaOption` slice, and native forms add one separate
    native option slice.
55. Metadata and EXPLAIN results use the exact flat `RegionPlain` shapes and explicit destination
    region in §18; they neither borrow native buffers nor allocate on a hidden heap.
56. Checked state is per permitted driver; `CheckedRequired` for `AnySupportedDriver` requires
    current SQLite and PostgreSQL artifacts.
57. `StaticCommandArtifact`/`CommandStatic` share Query source/wire/binder/retention/checked/cache
    rules and omit only Row/result/decode data.
58. The common, SQLite, and PostgreSQL first-release option sums/defaults/conflicts are the mandatory
    finite sets in §§11–13, not implementation-selected examples.
59. D1/D2/D4/D6/D7 own option APIs needed by their operations; D9 completes deadline
    enforcement/native cancellation cleanup and cross-scope disposition without creating an interim
    surface.
60. Migration transaction policy is the exact first-line required/forbidden directive; required is
    atomic by default, and forbidden uses one statement plus dirty-state/checksum-bound repair.
61. A LEFT JOIN child is absent only when all child fields are NULL; either partial-NULL direction is
    a contract error.
62. `next_batch` is absent from the D1–D12 common surface and lands only in D13.
63. Canonical many-parent shaping builds parallel parent/child/offset arrays and does not push an
    array-bearing per-parent Output through a region builder.
64. PostgreSQL may skip only in optional local runs; D4 merge and releases require the non-skippable
    provisioned `db-postgres` CI gate.
65. Ordinary package code can define `rows_stmt<P,R>` and `all<P,R: RegionPlain>` with
    `query<P,R>`, `rows<R>`, and `array<R>`; all nested types are concrete before MIR and no DB
    builtin implements those helpers.
66. Every recursively Move Query/helper return forwards one dynamic cleanup bit across
    direct/indirect/imported ABIs; arena-owned `Ok` and individually owned `Err` paths Drop exactly.
67. `borrow mut` rejects direct or recursively embedded overlap in every peer mode, including
    distinct aggregate holders and `Out`.
68. A capturing function value that returns a captured view/resource reference remains tied to the
    selected environment and captured owner across direct/indirect calls, joins, and moves.
69. Replacing an owned value through `borrow mut` drops the old pointee exactly once and updates the
    caller bit; an unchanged pointee receives no callee function-exit Drop.
70. `resource.into_raw` accepts only a standalone owned resource root and rejects every
    field/element/projection/borrowed/out/temporary operand.
71. Static manifests/action keys include exact per-driver checked-metadata
    `Missing | Present(hash, format_version)` state; create/change/delete invalidates
    CheckedOptional/CheckedRequired and Any tracks both drivers without directory scanning.
72. Synthetic field-selector/function-value facts use the same capture-root summary and
    Move-return cleanup ABI as named functions.
73. Static/dynamic/migration SQL, PostgreSQL Text Params, and libpq connection/control strings reject
    U+0000 before the native call; Binary-format bytea remains length-aware.
74. The first-release PostgreSQL mapping is exactly integer/float/bool/text/bytea/Option; every
    temporal/numeric/UUID/JSON/array/range/domain mapping requires a later explicit contract.
75. D9 enforces or pre-send rejects every deadline, issues no hidden SQL, and exposes no external
    cancel resource until a general Send/thread-safe-resource prerequisite is scheduled.
76. SQLite `BusyTimeoutNs` remains active through streamed `next` calls and restores the
    package-tracked prior value on exhaustion/error/Drop before connection reuse.
77. Synthetic field selectors recursively retain receiver provenance for nested view-bearing return
    types, not only a top-level `str`/slice.
78. PostgreSQL Text-format `bytea` is lowercase `\x` hex and never raw bytes; Binary format alone
    uses raw bytes plus explicit length.
79. Timeout/cancel returns a PostgreSQL connection only after proved protocol/transaction
    resynchronization; otherwise the connection is poisoned/closed.
80. `ContractError` represents Query-less operation/input validation without fabricating a Query ID.
81. First-release examples and mappings use only the exact integer/float/bool/text/bytea/Option set;
    deferred logical types require a later explicit contract before appearing in public examples.
82. `ContractError.query_id` is `Some(id)` for every Query/command subject, including metadata and
    EXPLAIN, and `None` only when the operation has no Query/command subject.
83. `db.exec_result` is the exact allocation-free Copy record
    `{ rows_affected: Option<i64> }`; no native result-buffer view escapes through it.
84. A resolved finite struct field may be Copy or recursively Move when one ordinary Drop plan is
    known; L1a's `Option<string>` field does not contradict the aggregate field rule.
85. `resource.borrow` is a public safe ownership operation wherever the resource type is visible,
    while all raw construction, extraction, transfer, and owner-tied raw-view operations remain
    declaring-subtree privileged.
86. Every D11 live command, including both repair actions, receives explicit entry, migration
    catalog, driver, and matching SQLite/PostgreSQL target inputs and uses no ambient default.
87. Metadata filters use exact Copy `SchemaRef`/`TableRef` inputs with no search-path, `main`, or SQL
    interpolation inference.
88. `KeyMeta` preserves foreign-key match/update/delete, deferrability, initial deferral, and
    validation evidence when available, and groups duplicate names deterministically by
    `key_ordinal`.
89. `IndexMeta` preserves key/included position, uniqueness/primary backing, sort/null order,
    predicate/expression/native method/opclass, and valid/ready evidence when available.
90. `ColumnMeta` and `QueryMeta` contain the exact native identity, origin, checked-artifact,
    rewrite, prepare/schema/server, and descriptive fields promised by §§16 and 18.
91. D0 records engine/version nullability/origin evidence; D3 and D5 own fail-closed support
    matrices before merge, `Unknown` never proves non-null, and every runtime NULL guard remains.
92. Every metadata category/detail and `MetaQueryEntry` discriminator follows §18.2.1's exact row,
    field-presence, Unknown-state, group-ordering, ordinal, and artifact-schema/digest contract.
93. Metadata schema/table inputs reject U+0000 as the exact Query-less `db.Error.Encode` before any
    native/catalog request on both drivers, with declaration-order precedence for multi-invalid
    inputs.

---

## 25. Open decisions before implementation

The load-bearing implementation shape is settled here and in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md). Remaining
consumer-driven type/native-surface decisions are:

1. Decimal precision/scale representation.
2. UUID, temporal, JSON/JSONB, PostgreSQL array/range/domain, and SQLite custom-type mappings.
3. The minimal safe dynamic `db.row`/`db.value` set.
4. Native callback safety for SQLite functions/collations.
5. Which COPY/pipeline/backup/blob operations have a measured consumer.

The engine/version nullability/origin support matrix is settled by §16.3.1 and owned by
D0/D3/D5 rather than this list. The remaining items have roadmap homes in D12–D14. They do not
permit weakening Query identity,
one-execution semantics, ownership, static artifacts, runtime validation, or option rejection.
“The driver library made the choice for us” is not a design rationale.

---

## 26. Instructions for implementation agents

1. Read this document and `../17-library-boundary-prerequisites.md` completely.
2. Follow the L1a–L7 dependency DAG; do not serialize independent L3/L4/L5 work, and do not start
   a safe driver API before the complete prerequisite gate.
3. Run the Align compiler self-review for every Rust PR.
4. Use the fewest independently correct capability PRs that keep the owner matrices coherent.
   Roadmap and acceptance labels are not PR boundaries. Add every listed negative/cleanup owner
   test for the capability being closed.
5. Do not introduce a database keyword, ORM, Query DSL, runtime reflection, public trait hierarchy,
   ambient allocator, manual public close, or package-name ownership special case.
6. Do not replace a missing prerequisite with `raw`, an explicit destroy function, a hidden heap
   vector, a lint-only lifetime rule, or a whole-program-only shortcut.
7. Preserve the separate-compilation/cache contract from D1 onward.
8. Record measured native metadata behavior rather than guessing from SQLite/PostgreSQL documentation.
9. Treat SQL execution-count and allocation/copy-count tests as correctness tests.
10. Do not build single-table CRUD and call the Query model complete.
