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
- `db.exec_result` contains affected-row and available command-status information.
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
- safe wrappers around private `extern "C" link("sqlite3")` and `link("pq")` declarations;
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
types. Basic driver calls go directly from package-generated code to libsqlite3/libpq.

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
  SQL source identity: File(logical path) | Inline(query_id)
  exact source bytes/hash
  deterministic per-driver wire bytes/hash and source map
  parameter occurrence/source-span table
  per-driver binding plan and parameter retention classes
  per-driver checked-metadata policy/state/digest
  generated bind and decode thunks
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
  rows_affected
  native command status when available
```

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

`RequireVersionAtLeast` is static, pins the descriptor to SQLite, and participates in its public
semantic contract and artifact. `Persistent`/`Normalize` apply only to preparation.
`BusyTimeoutNs` temporarily replaces the connection busy timeout for that execution and restores it
when that execution actually ends; it conflicts with another busy-timeout value. For
`execute`/`one`/`maybe_one`/`all`, restoration precedes return. For `rows`/`rows_stmt`, the rows
resource retains the override and the connection's package-tracked prior value, then restores it on
exhaustion, terminal step error, or Drop before releasing the connection/statement dependency.
Restoration failure poisons/closes the connection rather than returning it with unknown policy.
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

Each driver documents whether an operation-scoped option may override a connection default. The
recommended rule:

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
  connection input, transaction-option, or category-metadata validation. `item` names the exact
  operation and input in both cases;
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
alignc db prepare app.align --driver sqlite --database dev.sqlite
alignc db prepare app.align --driver sqlite --memory --migrations db/migrations
alignc db prepare app.align --driver postgres --url-env ALIGN_DB_PREPARE_URL
alignc db prepare app.align --driver postgres --url-env ALIGN_DB_PREPARE_URL --check
```

The entry `.align` path and one `--driver` are required in v1. Query discovery uses exactly the
entry's reachable import graph; it never scans the project for Query `.sql` files. The explicit
SQLite `--migrations <dir>` catalog is a separate tool input governed by §16.6. `--query
<fully-qualified-id>` may repeat to restrict the Query set. The output root is the entry build's
project root plus `.align-db/`. A later tool-only config file may shorten these flags, but it is not
a compiler manifest and is not required by the first implementation.

SQLite accepts exactly one schema environment: `--database <path>`, or `--memory` optionally
initialized from the canonical migration sequence in `--migrations <dir>` (§16.6). PostgreSQL accepts `--url-env
<name>`; the environment variable's value is read only by this explicit command. Direct URL flags
may exist for local use but must be warned as shell-history-visible. The command prints the selected
driver, schema source, server/library version, and Query count before writing.

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

Preparation compiles the reachable units in an explicit regeneration mode that still enforces every
language/Query contract but temporarily treats missing/stale checked metadata as the artifact to
regenerate. A normal `build`/`check` never enters that mode. `--check` performs the same describe
work, compares canonical bytes, writes nothing, and exits nonzero on missing/stale/different output.

Connection URLs and secrets come from explicit command arguments or environment variables used by
this command only. They are not compiler build inputs and are never written. A normal build reads
only the selected metadata artifact and never contacts the database.

### 16.3 Metadata location

A possible layout:

```text
.align-db/
  sqlite/
    <query-id-hash>.json
  postgres/
    <query-id-hash>.json
```

Version 1 uses canonical UTF-8 JSON with fixed field order, LF newlines, sorted arrays where order is
not semantic, and no JSON object whose iteration order affects identity. Every file contains:

```text
format_version
query_id, module, item, driver
SQL source identity: File(logical path) | Inline(query_id)
source-SQL hash, driver-wire-SQL hash, rewrite-format version, static-options hash
Params fingerprint and, for Query only, Row fingerprint
schema fingerprint
engine/driver version
PostgreSQL search_path and extension assumptions, when applicable
parameters: source name, protocol ordinal, logical/native type
columns for Query only: ordinal, source alias, logical/native type, nullable/origin evidence
```

Native PostgreSQL OIDs may be recorded as environment evidence, but volatile OIDs are not the sole
canonical type or schema identity. Canonical type names and schema-qualified identities are stored
alongside them.

Secrets and connection URLs are never stored.

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
- computes the schema-input fingerprint from the ordered tuples
  `(version, exact UTF-8 filename, content hash)`.

All name/version/gap/symlink/UTF-8 errors are reported before applying the first migration.
`alignc db migrate` in D11 reuses this exact catalog rule rather than inventing another order.
Declared PRAGMAs, attached databases, and extension availability also enter the recorded schema
environment; undeclared ambient connection state is forbidden.

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

### 17.2 Commands

Proposed tooling:

```sh
alignc db migrate
alignc db status
alignc db check
alignc db prepare
```

`migrate` applies pending migrations with driver-appropriate locking and a migration history table.
`status` reports applied/pending/checksum state. `check` compares configured schema/migrations/live
state as available. `prepare` produces Query metadata.

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
2. begin one transaction;
3. execute every statement in file order;
4. insert the applied history row in the same transaction;
5. commit, or roll back the whole file on any error.

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

```sh
alignc db repair --version N --accept-applied --expect-checksum HASH
alignc db repair --version N --clear-dirty --expect-checksum HASH
```

Both forms require an exact current-file checksum and a dirty row. `--accept-applied` marks the row
applied after the operator verifies native state. `--clear-dirty` removes only the dirty history
row after the operator verifies that retry is safe; it does not undo database effects. Applied rows
cannot be repaired by these commands. D11 tests crash/error boundaries before the statement, after
native success but before the history update, and during error recording on SQLite and PostgreSQL.

Down migrations are optional and are not automatically generated.

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
db.MetaQueryState     Declared | DatabaseChecked
db.MetaQueryEntry     Summary | Parameter | Column
db.MetaStatementClass Select | Dml | Ddl | Native | Unknown
db.PlanFormat         Text | Json | Native

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
  nullable: MetaNullability
  default_sql, generated_sql, origin_schema, origin_table, origin_column: Option<str>

db.KeyMeta
  schema, table, name: str
  kind: MetaKeyKind
  term_ordinal: i64
  local_column, referenced_schema, referenced_table, referenced_column, expression: Option<str>

db.IndexMeta
  schema, table, name: str
  unique: Option<bool>
  term_ordinal: i64
  column, expression, predicate, native_method, native_opclass: Option<str>

db.QueryMeta
  query_id: str
  driver: Driver
  driver_restriction: DriverRestriction
  statement_class: MetaStatementClass
  artifact_digest: str
  state: MetaQueryState
  metadata_fingerprint: Option<str>
  entry: MetaQueryEntry
  ordinal: Option<i64>
  source_name, source_alias, logical_type, native_type, origin: Option<str>
  nullable: MetaNullability

db.QueryPlan
  driver: Driver
  format: PlanFormat
  analyzed: bool
  body: str
```

Keys/constraints and indexes with several terms are repeated flat rows sharing `name`, ordered by
`term_ordinal`; a category result never hides a nested allocation. `db.QueryMeta` begins with one
`Summary` row followed by ordered parameter and column rows. Optional fields are `None` when the
requested detail level or engine evidence does not supply them; base identity fields are always
present. Driver-native operations return corresponding driver-specific flat `RegionPlain` records
and may add native fields, but use the same destination rule.

The exact common signatures and calls are:

```align
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
  db.meta_query(exec, query(), detail, out, [])?
plan: db.QueryPlan =
  db.explain(exec, query(), params, out, options)?
```

These are distinct public primitives. Each metadata category has exactly one form whose final
argument is `slice<db.MetaOption>`; `[]` means no options. The corresponding
`sqlite.meta_*_native`/`postgres.meta_*_native` form receives that common slice plus one
driver-native option slice. `meta_table(Full)` does not automatically fetch columns, keys, indexes,
or plans. `meta_table` returns `db.Error.NotFound` when absent; it does not return an optional
partially initialized record.

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
constraint name
```

Composite keys preserve ordered columns.

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

### 21.3 Bench anchors

The implementation roadmap must add benchmarks for at least:

- SQLite parameter bind + one-row typed decode;
- SQLite streamed text/blob bind with transient-copy bytes and allocations separated;
- PostgreSQL parameter bind + one-row typed decode;
- file/inline Query/command artifact generation and cold/warm rebuild;
- SQLite canonical migration catalog/replay at 10/100/1000 files;
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

The implementation follows small prerequisite and vertical PRs. A database PR is not allowed to
paper over a missing prerequisite with a package-name special case. Every executable milestone runs
an `.align` program end to end and tests execution count, decoded values, ownership/Drop, cleanup, and
errors.

### L1a–L7 — mandatory Align library-boundary prerequisites

Land the eight PRs specified in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md), in order:

```text
L1a recursive DropPlan framework + Option<string> fields
L1b Move sum/Option/Result payload completion
L2  contextual borrow modes + Copy mutation + Fn modes/joined provenance + interface summaries
L3  package-defined opaque/dependent Move resource + linkable Drop thunk + resource_ref/native views
L4  named arena binding + region + clone_in
L5  deterministic tagged file/inline inputs + one-item Query/command identity + artifacts/descriptors
L6  region-backed PlainStruct array_builder
L7  nested generic package APIs + closed structural RegionPlain bound
```

All focused tests and benchmarks in that plan are gates. No SQLite or PostgreSQL safe public
connection type lands before L3; no Query file support lands outside L5; no compound-output private
vector lands before L6.

The common generic `rows_stmt<P,R>`/`all<P,R>` implementation does not land before L7.

### D0 — native driver feasibility probes

Read-only/throwaway probes establish:

- exact libsqlite3 and libpq library/ABI availability on supported targets;
- SQLite prepare tail-pointer statement-count behavior;
- SQLite column pointer validity across `step/reset/finalize`;
- PostgreSQL extended-query single-statement behavior;
- libpq full-result and single-row pointer validity;
- parameter/result metadata and nullability actually available from each engine;
- cancellation and cleanup behavior.

The deliverable is recorded evidence in this document or a focused audit, not production raw handles.

### D1 — generated Query/command plans over a fake driver

Before a native database:

- construct `db.query<P,R>` from inline and sibling SQL;
- construct `db.command<P>` from inline and sibling SQL;
- emit/load `StaticQueryArtifact` and `StaticCommandArtifact`;
- preserve exact source SQL identity while generating deterministic SQLite/source and
  PostgreSQL/`$n` wire entries plus reverse span maps;
- generate the shared Params binder for both kinds and a Query-only decoder thunk;
- record per-driver `BindValue`/`BindCopy` retention classes for every Params field;
- exercise named-parameter occurrence tables;
- decode a fake flat scalar row;
- prove Query and command interface/implementation/cache invalidation boundaries;
- prove command identity, checked-policy state, source/wire/static-option hashes, and binder
  retention use the same versioned statement-artifact schema without a result contract;
- define/encode `db.QueryOption`, `db.CommandOption`, `sqlite.QueryOption`/
  `sqlite.CommandOption`, and `postgres.QueryOption`/`postgres.CommandOption` exactly as §11–§13;
- prove no runtime reflection or per-row name lookup.

Tests mutate SQL-only, private Query/command, public Params/Row, driver restriction, and per-driver
metadata digests independently. They compile-fail a command with a Row/decode contract and prove an
unchanged public command consumer is not recompiled by a SQL-only producer edit. Benchmark generated
Query/command binders, the Query decoder, and warm-cache behavior.

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
- one execution-count hook;
- close/finalize exactly once on success and every error exit.

It does not include text views, `all`, streaming, transactions, migrations, dynamic rows, metadata
catalogs, or later native breadth. The benchmark compares prepared bind + one-row decode with an
equivalent direct libsqlite3 loop.

### D3 — checked Query metadata core + SQLite

- canonical `.align-db/sqlite` artifact;
- `alignc db prepare` and `--check`;
- explicit temporary/in-memory schema setup with canonical migration catalog/order/fingerprint
  tests;
- Declared/CheckedOptional/CheckedRequired;
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
environment-gated for ordinary PRs but is mandatory evidence when D4 first lands and at a database
release performance gate.

### D5 — PostgreSQL checked metadata

- canonical `.align-db/postgres` artifact;
- engine version, search path, extension, and schema fingerprints;
- type names plus OID evidence;
- conservative nullability/origin evidence;
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

Benchmark direct prepared execution, common-layer execution, and re-prepare cost separately.

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
- PostgreSQL `BufferedFull` path first; `rows` is one-pass decode over that owned result, while
  explicitly selected single-row/portal delivery is D13;
- early Drop/finalize;
- one-million-row scalar and borrowed-text benchmarks;
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
- builder allocation/copy-count and high-fanout benchmarks.

Compound Query support is part of the first product contract, not a later ORM enhancement.

### D11 — SQL migrations

- SQL migration discovery/order;
- driver-specific multi-statement execution rules;
- checksums/history/status/check;
- exact first-line `transaction=required|forbidden` policy;
- required-by-default atomic behavior;
- one-statement forbidden files, dirty `Applying`/`Failed` states, and checksum-bound repair;
- reject U+0000 in every migration before applying the first file;
- explicit `alignc db migrate/status/check/repair`.

Migration implementation reuses connections/resources but does not change typed Query semantics.

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
that arena; multi-term keys/indexes remain flat ordered rows; and `meta_table` reports `NotFound`.
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

This slice is committed after the typed Query product. It is not permission to route typed Query
execution through a reflective dynamic engine.

### Initial release gate

The first release presented as Align database support requires L1a–L7 and D1–D12 for both SQLite
and PostgreSQL where the milestone is driver-relevant. D11 supplies SQL migration lifecycle and D12
supplies the required category metadata and explicit Query-plan access; neither is omitted from the
release whose earlier sections promise those surfaces. D13–D14 remain committed additive work for
batch/SoA/native breadth, dynamic SQL, and proved callbacks.

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
25. Benchmarks compare package overhead against direct native-driver loops.
26. All handles use the general opaque-resource/borrow machinery; no `pkg.db`-named ownership rule
    exists in the compiler.
27. Caller-selected materialization uses `arena name {}` and `region`, with no ambient allocator.
28. SQL-only edits invalidate the producer/query artifact without needlessly recompiling unchanged
    consumers.
29. Canonical compound shaping uses Pure `borrow mut` state and separate-builder transitions, so
    transitive database I/O is rejected; its Query-local orchestrator contains one visible rows
    loop.
30. Region builder allocation and its single compacting pass are measured and visible.
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
50. The first public database release completes driver-relevant D1–D12; D13/D14 remain committed
    additive work.
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

---

## 25. Open decisions before implementation

The load-bearing implementation shape is settled here and in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md). Remaining
consumer-driven type/native-surface decisions are:

1. Decimal precision/scale representation.
2. UUID, temporal, JSON/JSONB, PostgreSQL array/range/domain, and SQLite custom-type mappings.
3. The exact conservative nullability/origin evidence each supported engine/version exposes.
4. The minimal safe dynamic `db.row`/`db.value` set.
5. Native callback safety for SQLite functions/collations.
6. Which COPY/pipeline/backup/blob operations have a measured consumer.

These items have roadmap homes in D12–D14. They do not permit weakening Query identity,
one-execution semantics, ownership, static artifacts, runtime validation, or option rejection.
“The driver library made the choice for us” is not a design rationale.

---

## 26. Instructions for implementation agents

1. Read this document and `../17-library-boundary-prerequisites.md` completely.
2. Implement L1a–L7 in order; do not start a safe driver API before their owning gate.
3. Run the Align compiler self-review for every Rust PR.
4. Keep every PR at one roadmap slice and add its listed negative/cleanup tests.
5. Do not introduce a database keyword, ORM, Query DSL, runtime reflection, public trait hierarchy,
   ambient allocator, manual public close, or package-name ownership special case.
6. Do not replace a missing prerequisite with `raw`, an explicit destroy function, a hidden heap
   vector, a lint-only lifetime rule, or a whole-program-only shortcut.
7. Preserve the separate-compilation/cache contract from D1 onward.
8. Record measured native metadata behavior rather than guessing from SQLite/PostgreSQL documentation.
9. Treat SQL execution-count and allocation/copy-count tests as correctness tests.
10. Do not build single-table CRUD and call the Query model complete.
