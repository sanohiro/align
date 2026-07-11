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

## Per-profile sizes (M13 Slice 4)

```
bench/binary_size/profiles.sh [prog ...]
```

Builds each program at every `--profile` (`dev`/`release`/`fast`/`small`/`tiny`) with `alignc build`
and reports the file size, whether the image keeps a `.symtab` (stripped state), and the gated
`DT_NEEDED` set. The pipeline is the stock `default<O0|O2|O3|Os|Oz>`; `small`/`tiny` are additionally
stripped (`-Wl,--strip-all`).

Representative result (x86-64, glibc 2.41, release runtime):

| program | profile          | size    | symbols  |
|---------|------------------|--------:|----------|
| hello   | dev/release/fast | 4,274,568 | symbols  |
| hello   | small/tiny       |   324,496 | stripped |
| pipe    | dev              | 4,291,008 | symbols  |
| pipe    | release/fast     | 4,290,816 | symbols  |
| pipe    | small/tiny       |   336,784 | stripped |

- The **strip** step (size profiles) is the dominant lever here — it drops the runtime staticlib's
  symbol/debug info (~4.3 MB → ~0.32 MB).
- The **O-level** difference is negligible on these runtime-dominated programs (the Align code is
  tiny next to the linked runtime). LLVM does **not** guarantee `Oz ≤ Os ≤ O2` byte-for-byte, so the
  table reports the real numbers and asserts no ordering.
- Speed profiles keep symbols (useful backtrace / `perf`); size profiles strip.
