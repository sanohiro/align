# Align v0.5.0 Release Notes

Align v0.5.0 ships the complete committed `pkg.db` roadmap: deterministic static SQL, SQLite and PostgreSQL execution, checked metadata, reusable statements and transactions, resilient streaming, migrations and inspection, compound products, columnar batches, explicit pools, dynamic SQL, and proved SQLite scalar callbacks. The database work is backed by the language, ownership, ABI, build, and test infrastructure required to keep those surfaces explicit and separately compiled.

## `pkg.db` — typed SQL on SQLite and PostgreSQL

A query is an ordinary Align module paired with SQL. `Params` names the placeholders and `Row` names the result columns; the compiler produces deterministic artifacts and generated bind/decode code without reflection or a hidden statement cache.

```align
module db.queries.user_by_id

import pkg.db

pub Params { id: i64 }
pub Row { id: i64, name: str }

pub fn query() -> pkg.db.query<Params, Row> = pkg.db.query_file([])
```

The same common API works with SQLite and PostgreSQL:

- `execute`, `one`, and `rows`/`next` cover commands, exact-cardinality reads, and one-pass streams.
- `prepare`, `rows_stmt`, and explicit `begin`/`commit`/`rollback` make reuse and transaction ownership visible.
- `pkg.db.Error` is one closed package error model across both drivers; unsupported behavior is rejected rather than silently ignored.
- Move resources close, finalize, rollback, release, or poison exactly once on every success, error, early-exit, and Drop path.

SQLite supports explicit create/read-only/URI/pragma setup and native busy-timeout execution. PostgreSQL supports application/connect options, enforced deadlines and cancellation, buffered, `SingleRow`, and `PortalBatch` delivery, and text or binary parameter/result formats with checked protocol budgets.

## Checked SQL, migrations, and inspection

`alignc db prepare` is the explicit online step. It describes reachable queries against SQLite or PostgreSQL and writes canonical metadata under `.align-db/`; ordinary builds remain hermetic and read only committed artifacts. `DeclaredOnly`, `CheckedOptional`, and `CheckedRequired` select the exact per-query policy.

The release also adds deterministic migration tooling:

```text
alignc db migrate --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
alignc db status  --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
alignc db check   --entry main.align --migrations db/migrations --driver sqlite --sqlite-path dev.sqlite
```

Migration catalogs use contiguous versioned SQL files, canonical identities, explicit transaction policy, and fail-closed dirty-state repair. Read-only runtime inspection exposes tables, columns, keys, indexes, and query plans through region-owned records; it never migrates implicitly.

## Compound, columnar, and dynamic results

Typed streams can materialize one-pass nested products without reflection. Bounded `next_batch` calls produce independently owned batches, and `batch_soa` projects plain rows into the language's SoA form for column-shaped work while keeping the caller-selected memory bound visible.

`pkg.db.pool` is an eagerly opened, fixed-capacity, non-waiting pool for both drivers. Acquisition performs no hidden network or filesystem work, exhaustion is explicit, and a connection is returned only after its native idle state is proved.

When SQL cannot be static, the common dynamic rail accepts a closed `pkg.db.value` sum and provides `dynamic_execute`, `dynamic_rows`, and `dynamic_next`. Each returned row is copied into the caller's explicit region. PostgreSQL validates binary OIDs and formats; SQLite maps runtime storage classes without weakening ownership or cleanup guarantees.

SQLite scalar functions can be registered with exact noncapturing Align function targets. Argument and result values use the same closed dynamic value model, native callback state is allocation-accounted, and malformed native values, invalid callback values, and callback-returned errors cross the C boundary as deterministic SQLite errors. A language hard error still terminates the process; the callback boundary does not catch or downgrade it.

## Language and compiler foundations

The database surface drove general capabilities that are available to every package:

- borrowed parameters with caller ownership, replacement, and generation invalidation preserved across direct, imported, and function-value calls;
- recursively owned `Option`/`Result` and sum payloads with path-selected cleanup;
- package-defined opaque Move resources, dependent resources, checked native views, and producer-owned Drop thunks;
- named regions, explicit recursive cloning, and region-backed plain-struct array construction;
- deterministic static source inputs plus canonical interface and artifact codecs for separate compilation;
- canonical callable identities and an exhaustive checked-HIR validator that fails loudly instead of publishing an empty program;
- Copy-safe generic `json.scan` rows, including canonical generic identity and diagnostic ordering.

These are language-wide rules rather than `pkg.db` exceptions.

## Compiler, performance, and release engineering

- Primitive, captured, chunked, AoS, and selected invariant-string `par_map` paths gained real range kernels, stable filter compaction, and lower scheduler overhead.
- Repeated compilations reuse in-process results and a persistent per-unit frontend cache; ELF links use `ld.lld` when selected.
- `alignc` uses mimalloc. Release/fast programs enable runtime LTO by default, and versioned artifacts use a dedicated thin-LTO, single-codegen-unit `dist` profile with two-phase PGO over examples and the `pkg.db` corpus.
- CI now separates bounded PR owners from nightly full-suite detection, binds review and preflight evidence to the exact merge-base/HEAD pair, and runs all thirteen `pkg.db` owner suites against required PostgreSQL integration.
- Release artifacts target Linux x86-64, Linux AArch64, and macOS Apple Silicon. The macOS environment-clearing implementation no longer depends on unavailable `clearenv(3)`.

## Backward Compatibility Warning

**Align makes zero backward compatibility guarantees during the 0.x series.** v0.5.0 changes language ownership and borrow rules, compiler interfaces and artifact identities, runtime ABIs, generated database artifacts, package APIs, and diagnostics. Rebuild all Align code and regenerate checked database metadata with the v0.5.0 compiler; do not reuse v0.4.0 interface, object-cache, or `.align-db` artifacts.

## Known Intentional Limitations

- PostgreSQL COPY, LISTEN/NOTIFY, PostgreSQL native callbacks, SQLite collations, and additional logical database types remain consumer-gated. They are not partial v0.5.0 contracts.
- SQLite rejects query deadlines; use its explicit busy-timeout option for lock waits. PostgreSQL owns the deadline/cancellation path.
- Dynamic SQL is the explicit escape hatch. It does not infer result types, prepare statements implicitly, or provide a hidden cache.
- Windows remains unsupported. Distributed `alignc` binaries dynamically depend on LLVM 22 and use the platform toolchain and capability libraries documented in `docs/impl/11-release-distribution.md`.
- Fully escaping function values and the remaining owned-value closure-capture shapes remain deferred.
