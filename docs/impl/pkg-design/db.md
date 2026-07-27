This directory holds the authoritative per-package design docs for **first-party `pkg` libraries**,
at the same depth as `../std-design/` (signatures, Move/effect classification, error policy, slice
plan, pitfalls, test anchors). First-party packages are developed in this repo and distributed as
ordinary vendorable `pkg` subtrees.

# pkg — db

> **Call convention:** public call sites are fully qualified — `pkg.db.*`, `pkg.db.sqlite.*`, and
> `pkg.db.postgres.*` — so package provenance stays visible. For readability, the shorter `db.*`,
> `sqlite.*`, and `postgres.*` spellings in examples are shorthand for those fully qualified names.

## Status

**DESIGN PROPOSAL — query-centric SQL/database package.**

Initial required drivers: **SQLite and PostgreSQL**.

The semantic decisions in this document are the proposed contract. Exact API spellings remain
provisional where the current parser, ownership model, FFI surface, or package conventions require a
small adjustment. Such an adjustment must preserve the visible SQL, execution-count, ownership, and
native-extension guarantees below.

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
- A normal `align build` is offline and never contacts a database.
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

`align build`, `alignc build`, and ordinary module checking MUST NOT connect to a live database or the
network.

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

### 4.1 Initial packages

```text
pkg.db
pkg.db.sqlite
pkg.db.postgres
```

Possible future packages:

```text
pkg.db.pool
pkg.db.odbc
pkg.db.mysql
pkg.db.duckdb
```

The common package owns the semantic contracts. Driver packages own connection construction, native
types/options, authoritative prepare/describe integration, and driver metadata.

### 4.2 Minimal language impact

Version 1 SHOULD be implemented as first-party package modules with narrowly scoped compiler/runtime
support. It SHOULD NOT require:

- a `database` keyword;
- annotations or decorators;
- reflection;
- user-defined traits;
- row polymorphism;
- structural-record types;
- operator overloading;
- macros;
- a second compile-time language.

The compiler may recognize static Query constructors such as `db.query_file()` because they need the
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

- `db.conn` is a Move handle owning one physical SQLite or PostgreSQL connection.
- `db.tx` is a Move transaction handle tied to one connection. Dropping an active transaction performs
  fail-safe rollback, never commit.
- `db.exec` is a short-lived borrowed execution view produced from either a connection or a
  transaction. This lets a Query be reused inside and outside a transaction without a public trait.
- `db.query<P, R>` is a Copy static descriptor containing SQL identity, parameter contract, row
  contract, driver restriction, SQL hash, and static options.
- `db.command<P>` is the non-row descriptor. A statement with `RETURNING` is a Query.
- `db.stmt<P, R>` is a prepared Move statement tied to the physical connection that prepared it.
- `db.rows<R>` is a Move, one-pass typed row stream for exactly one execution.
- `db.exec_result` contains affected-row and available command-status information.
- `db.row` and `db.value` are explicit dynamic escape hatches.

The exact spelling of `conn.exec()` / `tx.exec()` may change if package-defined methods are not
available. The semantic requirement is one concrete borrowed execution type with two visible
constructors, not a trait hierarchy or runtime interface boxing.

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

pub fn query() -> db.query<Params, Row> = db.query_file()
```

The no-argument `query_file()` resolves the same-basename sibling SQL file:

```text
user_with_groups.align -> user_with_groups.sql
```

The Query module, not the SQL pathname, is the application-facing identity. Callers import the module
and call `user_with_groups.query()` or its `run()` helper. They do not pass filesystem paths around.

### 5.2 Explicit path override

A Query may explicitly link another static SQL file:

```align
pub fn query() -> db.query<Params, Row> =
  db.query_file("legacy/user_lookup.sql")
```

Rules:

- the path is a compile-time string literal;
- it is relative to the project/query root;
- absolute paths are rejected;
- escaping the project root is rejected;
- the file content is a build input;
- a changed file changes the Query hash and invalidates checked metadata.

This path is a definition-time linkage mechanism, not an execution argument.

### 5.3 Inline SQL

Short SQL may be inline:

```align
pub fn query() -> db.query<Params, User> =
  db.query("SELECT id, name, email FROM users WHERE id = :id")
```

The expression must be static. A runtime `string` cannot become a typed Query.

Complex SQL SHOULD use a `.sql` file because it is easier to review, format, run in database tools,
EXPLAIN, and diff as SQL.

### 5.4 One descriptor, one statement

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

### 5.5 Params

`Params` is a named struct. Every SQL value placeholder maps to exactly one field unless the driver’s
native placeholder model explicitly allows positional reuse.

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

- SQLite binds by the native named parameter;
- PostgreSQL lowers deterministically to `$1`, `$2`, ... while retaining the source names in
  diagnostics and metadata.

The compiler/preparer reports:

- missing fields;
- unused fields;
- duplicate or ambiguous placeholders;
- incompatible types;
- a parameter requiring an explicit native type declaration.

Values are always bound parameters. Value interpolation into SQL text is not a typed-Query feature.

### 5.6 Row

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

A reusable struct may be used directly as `Row` when its exact fields match. A Query-specific `Row` is
normal for joins and projections.

### 5.7 Output

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

API spelling is provisional; the behavior is normative.

### 6.1 Commands

A non-row statement is a `db.command<P>`:

```align
pub Params {
  id: i64,
  name: str,
}

pub fn command() -> db.command<Params> = db.command_file()
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
execute(exec, command, params)  -> Result<db.exec_result, db.Error>
one(exec, query, params, arena) -> Result<R, db.Error>
maybe_one(...)                  -> Result<Option<R>, db.Error>
all(...)                        -> Result<array<R>, db.Error>
rows(...)                       -> Result<db.rows<R>, db.Error>
prepare(...)                    -> Result<db.stmt<P,R>, db.Error>
```

Semantics:

- `one`: zero rows -> `NotFound`; more than one -> `Cardinality`.
- `maybe_one`: zero -> `None`; one -> `Some`; more than one -> `Cardinality`.
- `all`: materializes every row in the supplied arena/region.
- `rows`: one-pass stream; no implicit materialization.
- `execute`: never decodes rows.

The package MUST NOT provide a convenience call whose name hides whether it materializes or streams.

### 6.3 Query-local run helpers

A Query module may expose its preferred operation:

```align
pub fn run(
  exec: db.exec,
  params: Params,
  arena: Arena,
) -> Result<Option<User>, db.Error> {
  return db.maybe_one(exec, query(), params, arena)
}
```

For a compound result:

```align
pub fn run(
  exec: db.exec,
  params: Params,
  arena: Arena,
) -> Result<Option<Output>, db.Error> {
  rows := db.rows(exec, query(), params)?
  return shape(rows, arena)
}
```

This keeps the SQL path and result mode inside the named Query module while preserving visible
execution and allocation.

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

A future cache is explicit and must expose its scope and capacity.

### 6.5 Connection and transaction reuse

A Query accepts `db.exec`, not a connection-specific public trait.

Conceptually:

```align
outside := conn.exec()
inside := tx.exec()

result1 := user_with_groups.run(outside, params, arena)?
result2 := user_with_groups.run(inside, params, arena)?
```

If package-defined methods are unavailable, two named constructors may produce the same borrowed
`db.exec` type. The important properties are:

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

The shaper performs one pass:

```align
pub fn shape(
  rows: db.rows<Row>,
  arena: Arena,
) -> Result<Option<Output>, db.Error> {
  first := rows.next()? else return None

  user := User {
    id: first.user_id,
    name: first.user_name,
  }

  mut groups: array_builder<Group> := array_builder(arena)
  append_group(groups, first)?

  loop {
    row := rows.next()? else break

    if row.user_id != user.id {
      return Error.Cardinality
    }

    if row.user_name != user.name {
      return Error.Decode
    }

    append_group(groups, row)?
  }

  return Some(Output {
    user: user,
    groups: groups.build(),
  })
}
```

The exact builder support is a prerequisite to settle. The semantics are fixed:

- no second SQL statement;
- no hidden lazy load;
- no hash map;
- no hidden sort;
- one row pass;
- child allocation goes to the supplied arena;
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

### 8.1 Shaping cannot access a database

A conventional shaper receives only data and allocation context:

```text
shape(rows: db.rows<Row>, arena: Arena) -> Result<Output, db.Error>
```

It does not receive:

- `db.conn`;
- `db.tx`;
- `db.exec`;
- a prepared statement;
- a pool.

A lint SHOULD reject calls to database execution APIs from a conventional Query `shape` function.
The semantic design does not depend on the lint: standard examples and generated helpers must never
provide an execution handle to shaping.

### 8.2 One-pass by default

A shaper SHOULD consume `db.rows<Row>` once. It may:

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

A streamed `Row` may contain borrowed `str`/`bytes` views into the current driver result buffer. Those
views are region-bound and cannot escape after the row/batch advances unless cloned into an explicit
region.

The driver must document its validity window:

```text
row-at-a-time  views valid until next()
batch          views valid while the batch lives
materialized   views/owned values valid for the destination arena
```

The compiler/runtime must reject or prevent a borrowed row view from escaping its window.

### 9.3 Materialized results

`all`, `one`, `maybe_one`, and shaping that builds arrays receive an explicit arena/region. They do not
silently choose the global heap.

```align
rows := db.all(exec, query(), params, arena)?
```

Owned escape requires explicit clone/copy according to the normal Align memory model.

### 9.4 Builder prerequisite

Compound output requires a settled way to append plain structs into an arena-backed builder and then
freeze without a second element copy.

Required capability:

```text
array_builder<PlainStruct>
  push(value)
  build() -> array<PlainStruct>
```

This is a shared language/core prerequisite, not an excuse for a private database-only container.
Until it exists, a database implementation must not hide an equivalent heap vector behind the API.

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
metadata, and basic plan access. COPY/pipeline/notify may follow.

---

## 13. Native options and portability

### 13.1 Option scopes

Options are scoped, not one untyped bag:

```text
ConnectOption
QueryOption
PrepareOption
ExecuteOption
TxOption
MetaOption
ExplainOption
```

A driver option passed at the wrong scope is a compile-time type error.

### 13.2 No silent ignore

Every option has one of these outcomes:

```text
applied
rejected as unsupported
rejected as conflicting
```

“Ignored for portability” is forbidden.

### 13.3 Precedence

Each driver documents whether an operation-scoped option may override a connection default. The
recommended rule:

- static Query options describe Query semantics and cannot be overridden incompatibly;
- prepare options affect statement preparation only;
- execution options affect one execution only;
- connection defaults are fallback values only for explicitly overrideable properties;
- duplicate/conflicting non-overrideable options are errors.

### 13.4 Driver restriction

A Query descriptor records:

```text
AnySupportedDriver
SQLiteOnly
PostgreSQLOnly
```

`db.query_file()` starts portable in API terms, though its SQL may still fail driver preparation.
`sqlite.query_file()` and `postgres.query_file()` explicitly pin the descriptor. Native options also
pin it.

Executing a pinned Query with the wrong driver fails before SQL is sent (`DriverMismatch`).

Portability means “same common Align interface and successfully prepared SQL,” not automatic dialect
translation.

---

## 14. Transactions

### 14.1 Explicit lifecycle

```align
tx := db.begin(conn, common_tx_options)?

result := update_account.run(tx.exec(), params, arena)?

tx.commit()?
```

Rollback is explicit:

```align
tx.rollback()?
```

Dropping an active transaction performs fail-safe rollback. It MUST NOT commit.

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

Database operations return a structured database error type rather than aborting for recoverable
failures.

Proposed stable categories:

```align
db.Error {
  Connection,
  Timeout,
  Cancelled,
  NotFound,
  Cardinality,
  Constraint,
  Serialization,
  Deadlock,
  SchemaMismatch,
  DriverMismatch,
  Decode,
  InvalidQuery,
  Unsupported,
  Native(db.NativeError),
}
```

Exact payload shapes depend on current sum-type/owned-string capability, but the semantics are:

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

A Query may have these verification states:

```text
Declared
  static SQL source exists; Params/Row are valid Align types; hash is known

DatabaseChecked
  the selected database engine prepared/described the SQL and emitted metadata

RequiredChecked
  build/CI requires current matching checked metadata
```

The package/compiler must not describe a merely Declared Query as fully database-type-checked.

### 16.2 Explicit preparation command

```sh
align db prepare
```

The command:

1. loads project database configuration;
2. applies/selects the intended schema environment;
3. asks SQLite or PostgreSQL to prepare/describe every selected static Query;
4. records parameter/result/native type information and nullability where available;
5. records driver/version/schema identity;
6. records the SQL hash and relevant option hash;
7. writes deterministic repository metadata.

A normal build reads this metadata but never contacts the database.

### 16.3 Metadata location

A possible layout:

```text
.align-db/
  sqlite/
    <query-id>.json
  postgres/
    <query-id>.json
```

The exact encoding may be binary or JSON. It must be deterministic, reviewable through a tool, and
keyed by Query identity plus SQL/options/schema hash.

Secrets and connection URLs are never stored.

### 16.4 Stale metadata

If SQL, Params, Row, static options, driver, or declared schema identity changes, existing checked
metadata becomes stale.

Modes:

```text
optional   warn or fall back to Declared
required   compilation/CI error
```

No stale metadata may be silently treated as current.

### 16.5 No full custom SQL engine in v1

Version 1 SHOULD NOT implement a complete PostgreSQL/SQLite parser, resolver, function catalog,
implicit-cast engine, or nullability analyzer.

The database engine is authoritative during `align db prepare`. Align may perform lightweight static
work for file validation, placeholder scanning, statement count, hashing, diagnostics, and obvious
contract errors.

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
align db migrate
align db status
align db check
align db prepare
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
rows := db.dynamic_rows(exec, sql_text, params, arena)?
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
  run: align db prepare
```

```text
PostgreSQL Query cannot execute on SQLite connection:
  Query requires PostgreSQL (uses native Query options)
```

Unsupported/unknown native options must identify the option and driver rather than disappearing.

---

## 23. Roadmap

The implementation follows vertical slices. A milestone is complete only when an `.align` program
runs end to end and tests cover execution count, decoded values, ownership/drop behavior, and errors.

### M0 — design closure and driver spikes

Settle before broad coding:

- package placement and full-qualified public names;
- static `query<P,R>` descriptor representation;
- same-basename SQL build dependency;
- Params/Row static mapping representation;
- `db.exec` ownership/lifetime shape;
- row-view validity windows;
- libsqlite3 integration spike;
- libpq integration spike;
- authoritative prepare/describe metadata available from each driver;
- compound-output builder prerequisite;
- error payload feasibility;
- exact v1 type mappings.

Deliverable: a feasibility report with rejected alternatives and a vertical PR sequence. Do not start
with a broad SQL parser or ORM layer.

### M1 — SQLite walking skeleton

End-to-end SQLite slice:

- `import pkg.db` / `pkg.db.sqlite`;
- connect/close;
- sibling `.sql` Query descriptor;
- one named Params struct;
- scalar parameter binding;
- one typed flat Row;
- `one`, `maybe_one`, `all`, `execute`;
- explicit arena materialization;
- structured SQLite errors;
- execution-count test;
- direct-libsqlite3 comparison benchmark.

This milestone proves the Query model, not only the driver wrapper.

### M2 — PostgreSQL parity

End-to-end PostgreSQL slice with the same common Query module shape:

- libpq/native client integration;
- connect/close;
- named source parameter lowering to protocol order;
- common primitive types;
- one/maybe-one/all/execute;
- SQLSTATE/native error retention;
- Query driver restriction;
- common Query code exercised against both drivers where SQL is portable;
- direct-libpq comparison benchmark.

SQLite and PostgreSQL are both required before declaring the initial API shape stable.

### M3 — streaming, prepare, transaction, native options

- `db.rows<R>` one-pass stream;
- row view/lifetime enforcement;
- prepared Move statement;
- connection and transaction -> common `db.exec` view;
- begin/commit/rollback and rollback-on-Drop;
- SQLite begin modes;
- PostgreSQL isolation/read-only/deferrable options;
- connection/Query/prepare/execution option scopes;
- no-silent-ignore tests;
- cancellation/timeout basics;
- statement/result cleanup tests.

### M4 — migrations and database-checked Queries

- SQL migration runner;
- migration history/checksums/status;
- `align db migrate`;
- `align db prepare`;
- deterministic Query metadata;
- SQL/options/type/schema hashes;
- optional vs required checked mode;
- stale metadata diagnostics;
- SQLite temporary-schema preparation path;
- PostgreSQL development/ephemeral preparation path;
- no live database access during normal build test.

### M5 — compound Output

- one parent plus one-to-many output in one SQL execution;
- Query-local one-pass shaper;
- arena-backed plain-struct builder prerequisite shipped or explicitly landed first;
- repeated-parent consistency checks;
- null-child handling;
- no-DB-access shaper lint/test;
- transaction + master projection example;
- User + Groups example;
- multiple-child tagged/native-aggregate examples;
- execution-count and memory benchmarks.

Compound Query support is part of the first product contract, not a future ORM enhancement.

### M6 — fine-grained metadata

Common metadata:

- database;
- schema/attached database;
- table/view;
- columns;
- primary/unique/foreign keys and constraints;
- indexes;
- Query parameters/results;
- explicit EXPLAIN.

Native detail:

- PostgreSQL OIDs/types/opclasses/index details/JSON plans;
- SQLite STRICT/WITHOUT ROWID/hidden columns/index origin/query-plan details.

Tests verify that requesting table metadata does not implicitly fetch every column/key/index category.

### M7 — batch and data-oriented decode

- bounded `next_batch`;
- row-batch validity regions;
- segmented many-parent outputs;
- general adjacent grouping only if it has a non-DB consumer and settled core design;
- PostgreSQL binary parameter/result path;
- direct `soa<Row>` decode for eligible plain structs;
- nullable validity bitmap;
- batch/SoA benchmarks and no-intermediate-AoS verification.

### M8 — high-value native extensions

Consumer-driven additions:

- PostgreSQL COPY;
- PostgreSQL pipeline/single-row modes;
- LISTEN/NOTIFY;
- richer native/custom-type mapping;
- SQLite backup/incremental blob;
- FTS/virtual-table helpers where they remain thin;
- explicit pool package;
- additional drivers only after the common contracts are proven.

No roadmap item may add hidden relationship loading or a Query-builder DSL.

### Initial release gate

The first release presented as Align database support requires M0–M6 for both SQLite and PostgreSQL.
M7/M8 are performance/native extensions, but the design and APIs before them must leave those paths
open.

A release that handles only single-table model loading is incomplete even if CRUD works. At least one
many-to-one/master projection and one one-to-many compound Output must be end-to-end, execution-count
pinned, and documented.

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

---

## 25. Open decisions before implementation

These are implementation-shape decisions, not permission to weaken the settled semantics:

1. Exact package-call spelling for operations on opaque Move handles, given current method support.
2. Representation of static `db.query<P,R>` descriptors in module interfaces and incremental caches.
3. Whether `query_file()` is a compiler intrinsic, sema-known package call, or generated static table
   entry.
4. Exact row-stream borrow representation and the compiler check that invalidates row views on
   `next()`.
5. Arena-backed `array_builder<PlainStruct>` design and its shared non-database uses.
6. Final stable database error payloads under current owned-string/sum-type limits.
7. Decimal, UUID, temporal, JSON/JSONB, and native array representations.
8. How much nullability/origin information PostgreSQL and SQLite reliably expose during preparation.
9. Whether adjacent grouping starts as Query-local code or a settled core stream primitive.
10. Checked-metadata encoding and project configuration location.
11. The minimal safe dynamic-row value set.
12. Native callback safety for SQLite functions/collations, deferred unless a real consumer requires it.

The implementer must report these before writing the broad feature. “The driver library made the
choice for us” is not a design rationale.

---

## 26. Instructions for Codex/implementation agents

Before coding, produce a review with:

1. conflicts with current `draft.md`, ownership, generics, module, FFI, and package rules;
2. required compiler changes versus package/runtime-only work;
3. the smallest SQLite and PostgreSQL vertical slices;
4. static descriptor and checked-metadata representation;
5. row-view lifetime strategy;
6. compound-output builder prerequisites;
7. error and native-option representation;
8. proposed PR sequence with end-to-end tests and benchmarks;
9. features from this document that should be deferred only because a stated prerequisite is missing;
10. any proposal that would accidentally introduce an ORM, hidden SQL, reflection, a trait hierarchy,
    or a second query language.

Do not implement a large horizontal SQL subsystem first. Do not build single-table CRUD and call the
Query model complete. The first implementation path must preserve the final Query-centric shape from
the first SQLite and PostgreSQL programs onward.
