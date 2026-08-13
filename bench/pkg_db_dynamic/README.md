# `pkg_db_dynamic` — dynamic SQL row materialization

This is the named, non-gating D14 measurement for `pkg.db.dynamic_rows`. It runs the same
three-column logical result over SQLite and PostgreSQL: one `I64`, one nonempty `Text`, and one
two-byte `Bytes` value per row. Each driver is measured once through early Drop after the first row
and once through a complete 10,000-row scan.

Run against a disposable PostgreSQL database:

```text
ALIGN_DB_POSTGRES_URL=postgresql://... bench/pkg_db_dynamic/run.sh
```

The four records are SQLite first-row, SQLite full-scan, PostgreSQL first-row, and PostgreSQL
full-scan. Each reports elapsed nanoseconds, Align-runtime allocation count, matching free count,
delivered rows, and checksum. Timing is a machine-local characterization, never a semantic gate.
The allocation/free equality is the cleanup check.

The fixed contract-level storage observations are independent of timing:

| Shape | Setup scratch | Retained native owners | Region payload per delivered row |
|---|---:|---:|---:|
| zero parameters | SQL C string only | at most one statement/result | none before `dynamic_next` |
| empty Text + Bytes | two non-null one-byte sentinels, recorded lengths zero | at most one | row array only; no view payload allocation |
| nonempty Text + Bytes | one copy of each input view during setup | at most one | one text copy + one byte copy + one final row-array compaction |
| one row / early Drop | setup scratch freed before stream publication | exactly one until Drop | one current-row value plane |
| 10,000-row scan | setup scratch remains zero after publication | exactly one at every instant | one current-row value plane because each loop-local arena ends before the next advance |

`dynamic_rows` retains no parameter scratch. SQLite retains one prepared statement and PostgreSQL
one complete `PGresult`; the latter therefore pays full server/result buffering before its
first-row record, as the public contract states. The payload record excludes allocator metadata and
opaque SQLite/libpq bookkeeping, which neither package nor benchmark can portably observe.

## Recorded result

Candidate run: 2026-08-13, Pengwin on WSL2 Linux 6.18.33.2 x86-64, PostgreSQL server 17.10,
libpq 17.10. These values are observations, not thresholds.

| Arm | Elapsed (ns) | Allocations / frees | Rows | Checksum |
|---|---:|---:|---:|---:|
| SQLite first row | 1,120,662 | 5 / 5 | 1 | 1 |
| SQLite full scan | 6,733,315 | 5 / 5 | 10,000 | 50,005,000 |
| PostgreSQL first row | 4,942,555 | 10 / 10 | 1 | 1 |
| PostgreSQL full scan | 7,943,151 | 10 / 10 | 10,000 | 50,005,000 |
