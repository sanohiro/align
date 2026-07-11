# Binary size & dynamic-dependency benchmark (M13 Slice 2)

Measures the effect of **capability-based linking + link hygiene** on the produced executable: file
size and the gated `DT_NEEDED` set (`libz` / `libzstd` / `libcrypto` / `libssl`).

```
bench/binary_size/run.sh
```

For each program under `progs/` the script links the same object + release runtime **two ways**:

- **BEFORE** — the pre-Slice-2 driver: every gated library linked unconditionally
  (`-lz -lzstd -lcrypto -lssl`), no `--gc-sections`.
- **AFTER** — the current driver (`alignc build`): only the used capabilities' libraries
  (`align_mir::Capability`) plus `--gc-sections` / `--as-needed`.

## Representative result (x86-64, glibc 2.41, release runtime)

| program | before size | before gated deps | after size | after gated deps |
|---------|------------:|-------------------|-----------:|------------------|
| empty   |      15,824 | — (nothing pulled) |     15,648 | — |
| hello   |   5,519,840 | z, zstd, crypto, ssl | 4,274,568 | — |
| gzip    |   5,519,840 | z, zstd, crypto, ssl | 4,291,248 | z |
| crypto  |   5,519,840 | z, zstd, crypto, ssl | 4,290,904 | crypto, z, zstd |
| https   |   5,519,840 | z, zstd, crypto, ssl | 4,359,176 | z, zstd, crypto, ssl |

- The **size win** (~−22 % on any real program) comes from `--gc-sections` dropping the runtime's
  dead code; every `before` binary is the same 5.5 MB because nothing is garbage-collected.
- The **dependency-hygiene win** comes from capability collection: `hello` links **none** of the four
  gated libraries (was four); `gzip` links only `libz`; `https` legitimately links all four.
- `crypto` retains `z`/`zstd` because the single-member runtime co-locates crypto with compress and
  GNU ld stops garbage-collecting the member's compress references once `libcrypto` resolves some of
  its symbols. Fine-grained isolation (crypto → `libcrypto` alone) needs a runtime-crate split — a
  recorded follow-up. Passing the superset is always correct (`--as-needed` drops any truly-unused
  library from `DT_NEEDED`).

`https` is built, not run (no network).
