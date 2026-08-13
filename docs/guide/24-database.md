# Databases: pkg.db in practice

> 🌐 **English** · [Japanese](./ja/24-database.md)

`pkg.db` keeps SQL as SQL. A query is a small module: a `.sql` file, a `Params` struct for its placeholders, and a `Row` struct for its result columns. That SQL can be checked against a real schema offline; at runtime the package binds parameters and decodes rows through generated code, with no reflection and no hidden statement cache.

Vendor it like any package (chapter [23](23-packages.md)): copy [apps/db/pkg](../../apps/db/pkg) into your project root for `pkg.db` plus `pkg.db.sqlite`, `pkg.db.postgres`, and `pkg.db.pool`. Each section below is one thing you will actually need to do.

## The first query

```text
main.align
db/queries/user_by_id.align
db/queries/user_by_id.sql
pkg/db.align
pkg/db/…
```

```sql
SELECT id, name FROM users WHERE id = :id
```

```align
module db.queries.user_by_id

import pkg.db

pub Params { id: i64 }
pub Row { id: i64, name: str }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query_file([])
```

`query_file([])` links the sibling `.sql` of the same basename — no path argument, no glob. Each distinct `:name` maps to exactly one `Params` field, and `Row` is the exact result-column contract in `SELECT` order. Short SQL may be inline with `pkg.db.query("…", [])`, but a file is easier to review, format, and `EXPLAIN`.

```align
module main

import pkg.db
import pkg.db.sqlite
import db.queries.user_by_id

fn lookup(borrow connection: pkg.db.conn, id: i64) -> Result<string, pkg.db.Error> {
  q := db.queries.user_by_id.query()
  p := db.queries.user_by_id.Params { id: id }
  arena out {
    found := pkg.db.one(pkg.db.exec_conn(connection), q, p, out, [])?
    return Ok(found.name.clone())
  }
}

fn main() -> i32 {
  connection := pkg.db.sqlite.connect("app.db", []) else { return 1 }
  name := lookup(connection, 1) else { return 2 }
  print(name)
  return 0
}
```

Three things stay visible at the call site:

- `pkg.db.exec_conn(connection)` is the execution target. `pkg.db.exec_tx(transaction)` produces the same `pkg.db.exec` type, so query code is identical inside and outside a transaction.
- `out` is where the row's string views are cloned. `one` materializes into that region and the row cannot outlive it, so `.clone()` whatever must escape. `conn` is likewise a Move resource that closes on drop (chapter [05](05-memory.md)) — no `close()` to forget.
- `[]` is the execution-option slice. Empty means the documented default, never an inferred one.

`one` requires exactly one row: zero or two both return `pkg.db.Error.Cardinality`, and `observed_at_least` says which. There is no `maybe_one` and no `all`, so everything else streams.

`pkg.db.Error` is its own sum type, not the builtin `Error`, so a fallible `main` cannot return it (chapter [04](04-errors.md)). Match it, or `map_err` it at the boundary.

## When the result is many rows

```align
fn total_ids(borrow connection: pkg.db.conn) -> Result<i64, pkg.db.Error> {
  q := db.queries.active_users.query()
  p := db.queries.active_users.Params { active: true }
  mut stream := pkg.db.rows(pkg.db.exec_conn(connection), q, p, [])?
  mut total := 0
  loop {
    row := pkg.db.next(stream)? else { break }
    total = total + row.id
  }
  return Ok(total)
}
```

`rows` is one pass with no implicit materialization: the stream is a Move resource holding the connection until it is exhausted or dropped. The `loop`/`else`-unwrap shape is the ordinary one from chapter [02](02-language-basics.md) — the package adds no callback ABI to hide it.

For column-shaped work, take bounded batches instead of single rows:

```align
fn total_in_batches(target: pkg.db.exec) -> Result<i64, pkg.db.Error> {
  q := db.queries.user_ids.query()
  mut stream := pkg.db.rows(target, q, db.queries.user_ids.Params { min_id: 0 }, [])?
  mut total := 0
  loop {
    chunk := pkg.db.next_batch(stream, 64)? else { break }
    columns := pkg.db.batch_soa(chunk)?
    total = total + columns.id.sum()
  }
  return Ok(total)
}
```

`next_batch` materializes at most `max_rows` rows into one independently owned columnar batch; `batch_len` and `batch_row` index it, and `batch_soa` projects it as the `soa<R>` of chapter [11](11-data-oriented.md). You pick the bound, so the memory cost stays visible.

Deadlines are the common option `pkg.db.ExecuteOption.TimeoutNs(ns)`, and they are honest about driver support. PostgreSQL enforces one with a nonblocking wait plus native cancellation, mapping expiry to `pkg.db.Error.Timeout` and an engine-side cancellation to `Cancelled`. SQLite rejects the option before any SQL is sent — `Unsupported`, naming `db.execute.timeout_ns` — rather than accepting and ignoring it. Its native `pkg.db.sqlite.ExecuteOption.BusyTimeoutNs(ns)`, passed through `pkg.db.sqlite.rows_native`, bounds lock waits only and is deliberately not sold as a query deadline.

## Writing safely

A non-row statement is a `pkg.db.command<P>`:

```align
pub fn command() -> pkg.db.command<Params> = pkg.db.command_file([])
```

```align
outcome := pkg.db.execute(target, command(), params, [])?
affected := outcome.rows_affected else { return Ok(-1) }
```

`rows_affected` is `Option<i64>` because not every statement reports a count. A DML statement with `RETURNING` is a `query`, not a `command`.

Transactions move the connection through three named steps:

```align
fn seed(connection: pkg.db.conn) -> Result<pkg.db.conn, pkg.db.Error> {
  transaction := pkg.db.begin(connection, [])?
  attempt := add(pkg.db.exec_tx(transaction), 1, "ada")
  match attempt {
    Ok(_) => {}
    Err(failure) => {
      _ := pkg.db.rollback(transaction)?
      return Err(failure)
    }
  }
  return pkg.db.commit(transaction)
}
```

`begin` consumes the `conn` and produces a `tx`; `commit` and `rollback` consume the `tx` and give the `conn` back. That is why the failing arm matches instead of using `?`. A `?` inside the transaction body is *safe* — dropping `tx` performs a fail-safe rollback — but the early return drops the connection with it. Match when the caller must get the connection back; use `?` freely when it must not.

`[]` transaction options mean `DEFERRED` on SQLite and `READ COMMITTED READ WRITE` on PostgreSQL. Stronger modes go through the driver's `begin_native`, which takes the common option slice plus a native one: `pkg.db.sqlite.TxOption.Immediate`, or `pkg.db.postgres.TxOption.Isolation(pkg.db.postgres.Isolation.Serializable)`. There is no transparent retry — match `Serialization` or `Deadlock` and retry visibly.

## Changing the schema

Migrations are plain SQL files named `NNNN_snake_name.sql`, with contiguous versions:

```text
db/migrations/0001_create_users.sql
db/migrations/0002_create_groups.sql
```

```text
alignc db migrate --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
alignc db status  --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
alignc db check   --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
```

For PostgreSQL, replace the target with `--postgres-url-env NAME`: the variable *name* is on the command line, the URL and its password never are. Each command prints one line per migration plus a summary (`applied=1 pending=0 dirty=0 mismatched=0 history_only=0`). `status` reports; `check` additionally fails when the live state is not exactly the catalog, which is the CI gate.

Each file gets one transaction policy. The default is all-or-nothing; `-- align:migration transaction=forbidden` on the first line marks the exceptional single statement that must run outside a transaction, such as a concurrent index build. A forbidden migration interrupted mid-flight leaves a **dirty** row that blocks every later migration, and clearing it is deliberately manual — `alignc db repair … --version N --accept-applied|--clear-dirty --expect-checksum HASH`. The tool never guesses whether the statement took effect. Migration files may not contain `BEGIN`/`COMMIT`/`ROLLBACK`/`SAVEPOINT`; the runner owns that boundary.

## Getting the schema checked at compile time

`alignc db prepare` is the one workflow that contacts a database. It asks the engine to describe every query reachable from the entry and writes deterministic metadata under `.align-db/`:

```text
alignc db prepare main.align --driver sqlite --database dev.sqlite --schema-id dev-v1
alignc db prepare main.align --driver sqlite --memory --migrations db/migrations
alignc db prepare main.align --driver postgres --url-env ALIGN_DB_URL --schema-id dev-v1
alignc db prepare main.align --driver postgres --url-env ALIGN_DB_URL --schema-id dev-v1 --check
```

The `--memory` form builds a throwaway SQLite database from the migration catalog, so a checked build needs no live server. `--check` regenerates nothing and exits nonzero when the committed metadata is stale — another CI gate. Normal `alignc build` never opens a database; it reads only the committed metadata.

Asking for the check is a per-query option:

```align
pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query_file(
  [pkg.db.QueryOption.Check(pkg.db.CheckPolicy.CheckedRequired)],
)
```

`DeclaredOnly` (the default) checks only that the SQL and the structs are well formed. `CheckedOptional` uses whatever current metadata exists, per driver. `CheckedRequired` makes missing or stale metadata a build error — for *every* driver the query permits, which by default is both, so preparing only SQLite fails with `checked metadata for PostgreSQL is stale: checked metadata is missing`. Pin the query with `pkg.db.sqlite.query_file(options, native_options)` or its PostgreSQL twin when only one engine is real for you.

The same read-only catalog surface exists at runtime — `pkg.db.meta_tables`, `meta_columns`, `meta_keys`, `meta_indexes`, and `pkg.db.explain` for a query plan. It inspects; it never migrates.

## Supporting SQLite and PostgreSQL at once

The common surface — `query`/`command`, `execute`, `one`, `rows`/`next`, `next_batch`, `prepare`/`rows_stmt`, `begin`/`commit`/`rollback`, `pkg.db.Error` — is identical on both engines, and portable SQL uses `:name` placeholders that each driver lowers to its own protocol form. Everything engine-specific is qualified, so it is obvious in review:

```align
sqlite := pkg.db.sqlite.connect("app.db", [
  pkg.db.sqlite.ConnectOption.Create,
  pkg.db.sqlite.ConnectOption.Pragma("journal_mode", "WAL"),
])?

postgres := pkg.db.postgres.connect(url, [
  pkg.db.postgres.ConnectOption.ApplicationName("align-guide"),
  pkg.db.postgres.ConnectOption.ConnectTimeoutNs(5000000000),
])?
```

The `*_native` execution functions take one extra slice of driver options; nothing else changes:

```align
mut stream := pkg.db.postgres.rows_native(
  pkg.db.exec_conn(connection),
  db.queries.user_ids.query(),
  db.queries.user_ids.Params { min_id: 0 },
  [],
  [pkg.db.postgres.ExecuteOption.Delivery(pkg.db.postgres.Delivery.PortalBatch(64))],
)?
```

`Delivery` chooses how libpq hands rows over: the default buffers the whole result, `SingleRow` streams one row at a time, and `PortalBatch(n)` pulls fixed chunks. It changes the memory profile, not the loop you write. No requested option is ever silently ignored — an unsupported one is an `Unsupported` error naming the exact item.

## Reusing connections

`pkg.db.pool` is a fixed-capacity, non-waiting pool. It opens every connection up front, so acquisition does no network, filesystem, or authentication work:

```align
owner := pkg.db.pool.open_sqlite("app.db", 8, [])?
connection := pkg.db.pool.try_acquire(owner)?
snapshot := pkg.db.pool.info(owner)?
```

`try_acquire` never blocks or sleeps: with no idle connection it returns `pkg.db.Error.PoolExhausted` immediately, and backpressure stays your decision. The acquired value is an ordinary `pkg.db.conn` — every query, transaction, and prepared statement works on it unchanged — and returning it to the pool is its Drop. Capacity is `1..=1024` and fixed; a connection that cannot be proved transaction-idle at Drop is closed and its slot retired rather than silently reused, which `info`'s `capacity`/`idle`/`checked_out` make visible. Sessions are not reset between users: a `PRAGMA` or `SET` you applied stays on that physical connection.

## When the SQL really must be dynamic

The escape hatch is explicit, weaker, and named:

```align
fn counted(borrow connection: pkg.db.conn, out: region) -> Result<i64, pkg.db.Error> {
  params := [pkg.db.value.Bool(true)]
  mut stream := pkg.db.dynamic_rows(
    pkg.db.exec_conn(connection), pkg.db.Driver.SQLite,
    "SELECT count(*) FROM users WHERE active = ?1", params[..], [],
  )?
  first := pkg.db.dynamic_next(stream, out)? else { return Ok(0) }
  return match first.values[0] {
    I64(total) => Ok(total)
    _ => Ok(-1)
  }
}
```

The driver is an argument, not an inference: it is compared with the execution handle, and a mismatch fails with `DriverMismatch` before any SQL is sent. There is no placeholder rewriting here, so the SQL is that engine's own dialect — `?1` for SQLite, `$1` for PostgreSQL. Values go in as `pkg.db.value` and come back as an indexed `array<pkg.db.value>` in server column order, copied into the region you name; there is no column-name table and no reflective decode into a struct. `pkg.db.dynamic_execute` is the non-row form.

Identifiers still cannot be bound. Branching between two named static queries is the supported answer; string-concatenating an identifier into SQL is not.

## What is not there yet

- No `maybe_one` and no `all` — use `one` or the stream.
- Deadlines cover execution only, and only on PostgreSQL. Neither driver accepts `PrepareOption.TimeoutNs` or `TxOption.BeginTimeoutNs` in v1; both reject them as `Unsupported` instead of pretending.
- Prepared statements belong to one connection and never migrate between pooled ones. There is no statement cache, global or otherwise.
- The pool never waits, reconnects, health-checks, or resets a session.
- Dynamic SQL has no prepared or eager-materializing form.
- Native callbacks — custom functions, busy handlers, extension loading — wait for the proved callback rail. Until it and the final cross-rail audit land, treat the public surface as still moving.

The contract of record is `docs/impl/pkg-design/db.md`. [apps/db](../../apps/db) is the package-author workspace, and its `app/user_groups.align` is a worked one-to-many shaping example: one query, one ordered pass, one `array_builder` per child collection.
