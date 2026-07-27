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

- `one` reads at most the rows needed to prove exactly one;
- `maybe_one` reads at most the rows needed to prove zero-or-one;
- `all` materializes the full result into the supplied region;
- `rows` is a one-pass stream;
- `next_batch` materializes one bounded batch;
- shaping is one pass unless its code visibly asks for another data structure or sort.

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
- deterministic compiler-registered static source inputs and Query artifacts;
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
expected `Params` and `Row` types, the current source path, a SQL-content hash, and build-dependency
tracking. This is a narrow builtin-driven static-data feature, analogous to other compiler-known
formats, not general metaprogramming.

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
  contract, driver restriction, SQL hash, and static options.
- `db.command<P>` is the non-row descriptor. A statement with `RETURNING` is a Query.
- `db.stmt<P, R>` is a prepared Move statement carrying an inferred dependency on the physical
  connection that prepared it.
- `db.rows<R>` is a Move, one-pass typed row stream for exactly one execution, carrying a dependency
  on its statement/connection while native buffers need it.
- `db.exec_result` contains affected-row and available command-status information.
- `db.row` and `db.value` are explicit dynamic escape hatches.

The common package declares the resource types. `pkg.db.internal` owns a tagged native state and the
FFI declarations/drop/operation dispatch for the two first-party drivers; public driver constructors
call into that internal module. The root/common module therefore does not import its driver
submodules and no module cycle is created. Adding a first-party driver deliberately extends this
closed internal dispatch. There is no public driver trait, trait object, or driver-provided resource
ABI.

Execution views use the required borrowed-parameter surface:

```align
pub fn exec_conn(borrow c: db.conn) -> db.exec
pub fn exec_tx(borrow t: db.tx) -> db.exec
```

`db.exec` is a Copy sum over `resource_ref<db.conn>` and `resource_ref<db.tx>`. Its provenance is
therefore the source resource generation. Moving/dropping a connection, or committing/rolling back a
transaction, invalidates every derived execution view.

### 4.4 Implementation boundary

After L1a–L6, the following are ordinary first-party Align package code:

- public handles/descriptors/options/errors/metadata data shapes;
- the closed common/SQLite/PostgreSQL module API;
- safe wrappers around private `extern "C" link("sqlite3")` and `link("pq")` declarations;
- connection/transaction/statement/rows lifecycle code using general resources;
- driver-specific bind/step/result/metadata calls;
- Query-local `run`, Pure shaping steps, builders, and output types;
- migrations as SQL and their explicit package/tool orchestration.

The compiler/frontend owns only work that cannot be expressed as runtime package code:

- L1a–L6 language, ownership, region, interface, and MIR support;
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
- the exact SQL bytes are hashed and sent to the database without newline normalization;
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
  SQL logical path and exact bytes/hash
  parameter occurrence/source-span table
  checked-metadata policy/digest
  generated bind and decode thunks
```

The Query ID is the fully-qualified module path plus descriptor function name, never an absolute
filesystem path. A SQL-only edit recompiles/relinks the producer and invalidates checked metadata,
but it does not recompile consumers when `Params`, `Row`, driver restriction, and static semantic
options are unchanged. A public-contract change updates `interface_hash` and invalidates consumers.

The runtime descriptor points to immutable producer-owned static data and generated binder/decoder
thunks. `P` and `R` are compile-time contracts; there is no runtime reflection, per-row field-name
lookup, or consumer-side Query-body instantiation.

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
  AND created_at >= :from_date
```

```align
pub Params {
  active: bool,
  from_date: db.timestamp,
}
```

The prepare step lowers named parameters to the driver protocol:

- SQLite resolves and binds each native `:name`;
- PostgreSQL assigns `$1`, `$2`, ... by first lexical occurrence and reuses the same ordinal for
  later occurrences of the same name;
- both retain original byte spans and names in diagnostics and metadata.

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
execute(exec, command, params)   -> Result<db.exec_result, db.Error>
one(exec, query, params, out)    -> Result<R, db.Error>
maybe_one(..., out)              -> Result<Option<R>, db.Error>
all(..., out)                    -> Result<array<R>, db.Error>
rows(...)                        -> Result<db.rows<R>, db.Error>
prepare(...)                     -> Result<db.stmt<P,R>, db.Error>
```

Semantics:

- `one`: zero rows -> `NotFound`; more than one -> `Cardinality`.
- `maybe_one`: zero -> `None`; one -> `Some`; more than one -> `Cardinality`.
- `one`/`maybe_one`/`all`: clone view-bearing fields into the supplied `region`; their results are
  tied to that region.
- `all`: grows region chunks and performs the region builder's one documented compacting pass.
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
  groups: array_builder<Group>,
}

fn step(
  borrow mut state: State,
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
    groups: array_builder(out),
  }
  rows := db.rows(exec, query(), params, [])?
  loop {
    row := db.next(rows)? else { break }
    step(state, row, out)?
  }
  return finish(state)
}
```

This keeps the SQL path and result mode inside the named Query module while preserving visible
execution and allocation. The normal `loop` is intentional: it avoids adding a database-special
generic callback ABI or extending Align's closed minimal-generics surface merely to hide a loop.
`borrow mut` gives the Pure step exclusive access without transferring the arena-owned Move state
across a by-value call.

### 6.4 Prepared statements

Preparation is explicit:

```align
stmt := db.prepare(exec, query(), [])?
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
`rows_stmt(borrow mut stmt, params, options)` returns a row resource tied to the statement's fresh
generation. The compiler therefore rejects reuse/finalization of the statement until that row
resource drops; after Drop, another execution may borrow the statement again. Direct
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
  groups: array_builder<Group>,
}

fn step(
  borrow mut state: State,
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

  group_id := row.group_id else return Ok(())
  group_name := row.group_name else return Err(partial_child())
  state.groups.push(Group {
    id: group_id,
    name: group_name.clone_in(out),
  })
  return Ok(())
}
```

`run` initializes `user_name` to the static empty view while `seen == false`, constructs exactly one
`db.rows` stream, advances it in a normal `loop`, calls `step(state, row, out)` once per row, and
freezes the builder in `finish`. The helper error constructors above return fully populated
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
step(borrow mut state: State, row: Row, out: region) -> Result<(), db.Error>
```

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

The common package should define or settle logical types only where primitive Align types are
insufficient:

```text
db.timestamp
db.date
db.time
db.decimal
db.uuid (only if a stable common representation is accepted)
```

No lossy implicit conversion is permitted.

### 10.2 SQLite mapping

Initial SQLite mapping:

```text
INTEGER         signed integer widths, checked on decode
REAL            f64 (f32 only by explicit expected conversion policy)
TEXT            str/string
BLOB            bytes/array<u8>
NULL            Option<T>
```

SQLite is dynamically typed. A typed Query therefore requires a stable declared/checked result type.
When SQLite cannot establish one, the SQL author must use an explicit `CAST`, a schema declaration, or
a Query-native type option. The package must not guess from the first row.

SQLite `STRICT` tables and declared column affinities should strengthen checking when available.

### 10.3 PostgreSQL mapping

Initial PostgreSQL mapping should include:

```text
int2/int4/int8          i16/i32/i64
float4/float8           f32/f64
bool                    bool
text/varchar/name       str/string
bytea                   bytes/array<u8>
date/time/timestamp     db logical temporal types
numeric                 db.decimal or explicit text/native representation
uuid                    stable db/native UUID type
json/jsonb              bytes/str view or explicit typed JSON decode
arrays                  native PostgreSQL array support when declared
```

Parameter/result OIDs are retained in checked metadata. Unknown/user-defined types require a visible
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

The driver must expose:

- open flags;
- URI mode;
- busy timeout/handler policy;
- arbitrary explicit PRAGMA where safely representable;
- shared/private cache flags where supported;
- thread/open mode;
- extension loading policy (disabled by default; explicit if ever supported).

No requested option may be silently ignored.

### 11.2 Query/prepare/execute options

Examples of required extension points:

```text
sqlite.QueryOption
sqlite.PrepareOption.Persistent
sqlite.PrepareOption.Normalize
sqlite.ExecuteOption
```

The exact option set follows libsqlite3 capabilities and measured consumers. The type system should
prevent a SQLite option from being passed to PostgreSQL.

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

The driver must permit explicit native parameter type and format controls:

```text
postgres.QueryOption.ParameterType(name, native_type)
postgres.PrepareOption.ParameterOid(name, oid)
postgres.ExecuteOption.ParameterFormat(name, Text|Binary)
postgres.ExecuteOption.ResultFormat(Text|Binary)
```

The first implementation may use text format for broad correctness, but the surface must not prevent
binary parameters/results. Unknown or conflicting OIDs are errors, not ignored hints.

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
metadata     db.metadata(exec, request, slice<db.MetaOption>)
EXPLAIN      db.explain(exec, query, params, slice<db.ExplainOption>)
```

The corresponding `driver.*_native` form receives the common option slice and one additional
driver option slice. It does not reinterpret a driver option as a common option. There are no
default arguments, optionless overloads, fluent option builders, string-key option maps, or
process-global option state. Query-local `run` helpers decide visibly whether to accept an option
slice from their caller or pass `[]`.

Connection, Query, prepare, execution, transaction, metadata, and EXPLAIN therefore have distinct
static types even when two variants happen to carry the same scalar. D9 may add variants to these
settled containers; it must not invent another representation.

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

A Query descriptor records:

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
  query_id: string,
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

A Query has an observed verification state and a static policy:

```text
state
Declared
  static SQL source exists; Params/Row are valid Align types; hash is known

DatabaseChecked
  the selected database engine prepared/described the SQL and emitted metadata

policy
DeclaredOnly
CheckedOptional
CheckedRequired
```

`CheckedRequired` makes missing or stale metadata a compile error. `CheckedOptional` uses current
matching metadata when present and otherwise remains honestly `Declared`. The package/compiler must
not describe a merely Declared Query as fully database-type-checked.

### 16.2 Explicit preparation command

```sh
alignc db prepare app.align --driver sqlite --database dev.sqlite
alignc db prepare app.align --driver sqlite --memory --migrations db/migrations
alignc db prepare app.align --driver postgres --url-env ALIGN_DB_PREPARE_URL
alignc db prepare app.align --driver postgres --url-env ALIGN_DB_PREPARE_URL --check
```

The entry `.align` path and one `--driver` are required in v1. Query discovery uses exactly the
entry's reachable import graph; it never scans the project for `.sql` files. `--query
<fully-qualified-id>` may repeat to restrict that set. The output root is the entry build's project
root plus `.align-db/`. A later tool-only config file may shorten these flags, but it is not a
compiler manifest and is not required by the first implementation.

SQLite accepts exactly one schema environment: `--database <path>`, or `--memory` optionally
initialized from the ordered SQL files in `--migrations <dir>`. PostgreSQL accepts `--url-env
<name>`; the environment variable's value is read only by this explicit command. Direct URL flags
may exist for local use but must be warned as shell-history-visible. The command prints the selected
driver, schema source, server/library version, and Query count before writing.

This is the only workflow allowed to contact a database. It:

1. reads explicit command flags and optional tool-only preparation configuration;
2. applies/selects the intended schema environment;
3. asks SQLite or PostgreSQL to prepare/describe every selected static Query;
4. records parameter/result/native type information and nullability where available;
5. records driver/version/schema identity;
6. records the SQL hash and relevant option hash;
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
SQL hash, static-options hash
Params and Row fingerprints
schema fingerprint
engine/driver version
PostgreSQL search_path and extension assumptions, when applicable
parameters: source name, protocol ordinal, logical/native type
columns: ordinal, source alias, logical/native type, nullable/origin evidence
```

Native PostgreSQL OIDs may be recorded as environment evidence, but volatile OIDs are not the sole
canonical type or schema identity. Canonical type names and schema-qualified identities are stored
alongside them.

Secrets and connection URLs are never stored.

For each static Query/driver pair, the compiler derives the one metadata pathname from the Query ID
hash. Its action key records `Missing` or `Present(content_hash)` even under `CheckedOptional`.
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

`CheckedOptional` with missing/stale metadata emits the same honest Declared descriptor as if no
metadata existed; it never embeds stale evidence. `CheckedRequired` fails before object-cache reuse.
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

The chosen source is explicit in project configuration/command output.

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

The tool must not claim all migrations are transactional. PostgreSQL and SQLite differ, and some
native statements cannot be wrapped as ordinary transactions. The migration file or driver policy
makes this visible.

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

Conceptual common calls:

```align
database := db.meta_database(exec, detail)?
schemas := db.meta_schemas(exec, detail)?
tables := db.meta_tables(exec, schema_filter, detail)?
table := db.meta_table(exec, table_ref, detail)?
columns := db.meta_columns(exec, table_ref, detail)?
keys := db.meta_keys(exec, table_ref, detail)?
indexes := db.meta_indexes(exec, table_ref, detail)?
query_meta := db.meta_query(exec, query(), detail)?
plan := db.explain(exec, query(), params, options)?
```

These are distinct calls. `meta_table(Full)` does not automatically fetch columns, keys, indexes, or
plans.

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
SQL hash
driver restriction
verification state
parameter names/order/common types/native types/native ids
result column names/order/common types/native types/native ids/nullability
origin table/column when the database reports it
prepare/schema/server identity
```

### 18.10 Query plans

Plan retrieval is explicit because it may be expensive and may execute the Query under ANALYZE.

```text
EXPLAIN only        plan without running user statement where supported
EXPLAIN ANALYZE     explicitly executes; must be separately named/confirmed in API
```

The API must make “executes the statement” visible. It may return text, JSON/native structured plan,
or both.

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
rows := db.dynamic_rows(exec, sql_text, params, out)?
```

Dynamic results use:

```text
db.row
db.value
```

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
- A timeout is an explicit option/deadline, not an implicit default.
- Cancellation maps to a stable error category and retains native detail.
- Cancelling a PostgreSQL Query may use libpq cancellation; SQLite may use interrupt support.
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
- PostgreSQL parameter bind + one-row typed decode;
- prepared repeated execution;
- large streamed flat result;
- one-to-many one-pass shaping;
- segmented parent/child output;
- text vs PostgreSQL binary result where supported;
- direct SoA/batch decode when implemented;
- metadata request granularity;
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
  SQL hash changed for db/queries/user_by_id.sql
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

### L1a–L6 — mandatory Align library-boundary prerequisites

Land the seven PRs specified in
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md), in order:

```text
L1a recursive DropPlan + Option<Move> fields
L1b Move sum/Option/Result payload completion
L2  borrow / borrow mut parameters + interface return-borrow summaries
L3  package-defined opaque/dependent Move resource + resource_ref/native views + exactly-once Drop
L4  named arena binding + region + clone_in
L5  deterministic static inputs + StaticQueryArtifact + descriptor skeleton
L6  region-backed PlainStruct array_builder
```

All focused tests and benchmarks in that plan are gates. No SQLite or PostgreSQL safe public
connection type lands before L3; no Query file support lands outside L5; no compound-output private
vector lands before L6.

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

### D1 — generated Query plan over a fake driver

Before a native database:

- construct `db.query<P,R>` from inline and sibling SQL;
- emit/load `StaticQueryArtifact`;
- generate binder/decoder thunks;
- exercise named-parameter occurrence tables;
- decode a fake flat scalar row;
- prove interface/implementation/cache invalidation boundaries;
- prove no runtime reflection or per-row name lookup.

Tests mutate SQL-only, private Query, public Params/Row, driver restriction, and metadata digest
independently. Benchmark generated binder/decoder calls and warm-cache behavior.

### D2 — minimal SQLite Query vertical

The first native vertical is deliberately exact:

- in-memory SQLite connection as `db.conn`;
- one `db.command<Params>` inserting an `i64`;
- one sibling-file `db.query<Params,Row>` selecting one `i64`;
- Params and Row contain only self-contained scalar fields;
- `execute` and `one`;
- zero/one/more-than-one cardinality behavior;
- structured SQLite primary/extended errors;
- one execution-count hook;
- close/finalize exactly once on success and every error exit.

It does not include text views, `all`, streaming, transactions, migrations, dynamic rows, metadata
catalogs, or native options. The benchmark compares prepared bind + one-row decode with an equivalent
direct libsqlite3 loop.

### D3 — checked Query metadata core + SQLite

- canonical `.align-db/sqlite` artifact;
- `alignc db prepare` and `--check`;
- explicit temporary/in-memory schema setup;
- Declared/CheckedOptional/CheckedRequired;
- stale SQL/options/type/schema diagnostics;
- normal-build no-network test;
- runtime SQLite storage-class and NULL validation.

This milestone lands before promising that a typed Query is database-checked.

### D4 — minimal PostgreSQL Query vertical

- PostgreSQL `db.conn` over libpq;
- the same common Query module shape as D2;
- dialect-aware named-source scan and `$n` rewrite;
- repeated source name reuses one ordinal;
- scalar bind/decode;
- SQLSTATE and owned native error detail;
- driver restriction before SQL send;
- portable `CAST(:value AS BIGINT)` Query exercised on both drivers;
- execution-count hook and cleanup tests.

Integration tests use an explicitly configured local/ephemeral PostgreSQL instance and skip with a
reported reason when it is unavailable. The direct-libpq comparison benchmark is environment-gated.

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
- explicit prepare options;
- `rows_stmt(borrow mut stmt, ...)` and repeated sequential execution after each rows Drop;
- finalize/close on Drop and errors;
- no implicit global statement cache.

Benchmark direct prepared execution, common-layer execution, and re-prepare cost separately.

### D7 — transaction and common execution view

- `db.begin` consumes `db.conn`;
- `db.exec_conn` and `db.exec_tx` return the same borrowed type;
- `db.commit`/`db.rollback` consume `db.tx` and return `db.conn`;
- Drop rollback closes the connection;
- SQLite begin modes;
- PostgreSQL isolation/read-only/deferrable combinations;
- use-after-end and conn/tx alias rejection.

### D8 — typed row streaming

- dependent `db.rows<R>` resource;
- `next(borrow mut rows)` generation invalidation;
- owner-tied `resource.view_from_raw` with invalid pointer/length/UTF-8 rejection;
- SQLite current-row views;
- PostgreSQL full-result path first, single-row mode only when explicitly selected;
- early Drop/finalize;
- one-million-row scalar and borrowed-text benchmarks;
- compile-fail tests for use-after-next, storage in a longer-lived builder, return, branch, and loop.

### D9 — scoped native options, timeout, and cancellation

- common versus driver-qualified option entry points;
- connection/Query/prepare/execute/transaction scopes;
- applied/unsupported/conflicting outcomes;
- no-silent-ignore tests;
- SQLite busy/locking controls;
- PostgreSQL timeout/cancel path;
- statement/result cleanup under cancellation.

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
- transaction policy and partial-failure reporting;
- explicit `alignc db migrate/status/check`.

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

Native detail:

- PostgreSQL OIDs/types/opclasses/index details/JSON plans;
- SQLite STRICT/WITHOUT ROWID/hidden columns/index origin/query-plan details.

Tests prove that one requested category does not fetch unrelated categories. `EXPLAIN ANALYZE`
remains a visibly executing operation.

### D13 — batch, SoA, and high-value native paths

- bounded `next_batch` and batch generations;
- PostgreSQL binary parameter/result formats;
- segmented child buffers and nullable validity bitmaps;
- direct eligible `soa<Row>` decode with no intermediate AoS;
- PostgreSQL COPY/pipeline/single-row/LISTEN-NOTIFY;
- SQLite backup/incremental blob/FTS helpers;
- explicit pool package;
- additional drivers only after the common contracts are proven.

No roadmap item may add hidden relationship loading or a Query-builder DSL.

### D14 — dynamic SQL and native callbacks

- settle and implement the minimal owned `db.value` sum and indexed `db.row`;
- require a visible dynamic SQL string, explicit value slice, and explicit driver restriction;
- keep dynamic execution separate from typed Query descriptors and checked artifacts;
- decode dynamic rows by indexed access only, with no struct reflection or name-based field writes;
- add SQLite function/collation and PostgreSQL notice/COPY callback surfaces only after the
  callback-capture, abort, reentrancy, and thread rules are proved;
- pin statement count, value allocation, callback lifetime, and cleanup behavior.

This slice is committed after the typed Query product. It is not permission to route typed Query
execution through a reflective dynamic engine.

### Initial release gate

The first release presented as Align database support requires L1a–L6 and D1–D10 for both SQLite and
PostgreSQL where the milestone is driver-relevant. D11–D14 remain committed roadmap work, not
architectural deferrals, but migrations/catalog breadth/batch/native extensions do not block the
first Query product.

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
29. Canonical compound shaping uses a Pure `borrow mut` state transition, so transitive database
    I/O is rejected; its Query-local orchestrator contains one visible rows loop.
30. Region builder allocation and its single compacting pass are measured and visible.
31. Structured owned `db.Error` and Move Output use ordinary recursive `Option`/`Result`/sum Drop;
    the successful path allocates no error storage.
32. `pkg.db`, `.sqlite`, and `.postgres` are acyclic modules in one `pkg/db` package subtree, not
    falsely independent package versions.

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
2. Implement L1a–L6 in order; do not start a safe driver API before their owning gate.
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
