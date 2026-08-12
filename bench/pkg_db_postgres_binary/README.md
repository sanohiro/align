# `pkg_db_postgres_binary` — PostgreSQL Text versus Binary

This is the named, non-gating D13 measurement for PostgreSQL parameter and result formats. It runs
the same static Query over 100,000 `int8` rows in `SingleRow` delivery, first stopping after the
first row and then scanning the complete result. The Text arm uses both format defaults; the Binary
arm selects Binary independently for both parameters and the result.

Run against a disposable PostgreSQL database:

```text
ALIGN_DB_POSTGRES_URL=postgresql://... bench/pkg_db_postgres_binary/run.sh
```

The four records are Text first-row, Binary first-row, Text full-scan, and Binary full-scan. Each
record reports elapsed nanoseconds, Align-runtime allocation count, matching free count, and a
checksum. The first-row checksum is 1 and the full-scan checksum is 5,000,050,000. Allocation/free
deltas cover the complete package resource lifetime; equality is the cleanup check. The harness is
built with `alloc-count` and `--no-rt-lto` so those counters come from the instrumented runtime.

The exact transported/copy record for this fixed input is independent of timing noise:

| Arm | Parameter payload/copied bytes | First-row payload/copied bytes | Full row payload/copied bytes |
|---|---:|---:|---:|
| Text | 7 / 7 | 1 / 0 | 488,895 / 0 |
| Binary | 16 / 16 | 8 / 0 | 800,000 / 0 |

`copied` counts package-owned parameter transport and result decoding separately. Scalar result
decoding reads directly from the current `PGresult`, so it copies zero payload bytes. The row-cache
storage is allocated once per execution and reused across all `SingleRow` generations.

## Recorded result

Recorded on 2026-08-13 under WSL2 on an AMD Ryzen 9 5950X, against disposable PostgreSQL 16.4,
after warming every arm:

| Arm | Elapsed (ns) | Allocations / frees | Checksum |
|---|---:|---:|---:|
| Text first row | 14,143,816 | 10 / 10 | 1 |
| Binary first row | 15,268,222 | 11 / 11 | 1 |
| Text full scan | 1,181,213,953 | 10 / 10 | 5,000,050,000 |
| Binary full scan | 1,410,050,127 | 11 / 11 | 5,000,050,000 |

Allocation counts stay constant between first-row and 100,000-row scans for each arm. This closes
the row-cache reuse and complete cleanup checks; timings are a local characterization, not a gate.
