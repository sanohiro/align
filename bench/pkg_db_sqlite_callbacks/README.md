# `pkg_db_sqlite_callbacks` — SQLite scalar callbacks

This is the named, non-gating D14 measurement for `pkg.db.sqlite` scalar callbacks. It records
same-connection replacement registration and scalar invocation at arity 0, 1, and 127:

```text
bench/pkg_db_sqlite_callbacks/run.sh
```

The scalar arms each scan 10,000 SQLite rows. The arity-127 arm therefore converts and consumes
1,270,000 callback arguments. The checksums prove that every callback ran and returned the expected
value. Timing is a machine-local characterization with no semantic threshold.

## Recorded result

Candidate run: 2026-08-13, AMD Ryzen 9 5950X, WSL2 Linux 6.18.33.2 x86-64, SQLite
3.46.1. These values are observations, not thresholds.

| Arm | Elapsed (ns) | Operations | Checksum |
|---|---:|---:|---:|
| Same-identity registration | 8,636,156 | 10,000 | 10,000 |
| Scalar arity 0 | 218,751 | 10,000 | 10,000 |
| Scalar arity 1 | 422,372 | 10,000 | 50,005,000 |
| Scalar arity 127 | 14,103,933 | 1,270,000 arguments | 6,350,635,000 |
