# `json_decode` — JSON decode-throughput duel (Align vs `serde_json`)

The regression tracker for the **JSON parser rewrite** (recursive-descent → simdjson-style two-stage
SIMD). Measures Align's `json.decode` against idiomatic Rust `serde_json` on a runtime-generated
array of 4-field records, both folding `where(.active).pay.sum()` so the time is parser-dominated.

Two shapes (each with a matching serde baseline):

- **full** — decode all 4 fields into `array<Full>` (a full parse). serde → `Vec<Full4>`.
- **proj** — decode only `active`+`pay` into `array<Proj>`; the decoder skips the undeclared
  `score`/`extra` (the projection rail — declare only what you read). serde → `Vec<Proj2>`
  (serde ignores unknown fields by default, the same projection).

```sh
bench/json_decode/run.sh [baseline|v3|native]   # default native
```

Same plumbing as `bench/json_soa/`: the kernel is built with `alignc emit-obj` and the runtime is
linked as a **cdylib** (dynamic, over the C-ABI). Standalone cargo project (own `[workspace]`).

## Baseline (2026-06-29, native, recursive-descent parser — the "before")

```
  records  json KB |    A-full   rs-full     full× |    A-proj   rs-proj     proj×
    10000      498 |    0.718    0.739     1.03x |    0.637    0.696     1.09x
   100000     5083 |    7.519    7.701     1.02x |    6.584    7.271     1.10x
  1000000    51814 |   90.405   92.940     1.03x |   68.015   73.967     1.09x
```

The current recursive-descent decoder is **parse-bound** and roughly ties `serde_json` (full ≈1.03×;
proj ≈1.09×, a slight edge from skipping the two unused fields).

## After Mison speculation (2026-06-29, native, 1M rows)

```
  records  json KB |    A-full   rs-full     full× |    A-proj   rs-proj     proj×
    10000      498 |    0.891    0.802     0.90x |    0.538    0.703     1.31x
   100000     5083 |    7.040    7.717     1.10x |    4.415    7.280     1.65x
  1000000    51814 |   97.160   92.301     0.95x |   63.759   73.907     1.16x
```

The speculative decoder (lean `{ } [ ] :` decode-index + a Mison pattern: learn each declared field's
colon ordinal from the first record, then per record jump to it and verify the key — no `find_field`
— falling back on a structure miss) **wins the projection rail: `proj×` ≈1.09× → 1.16–1.65×**, while
full-decode stays at parity (≈0.9–1.1×). It does *not* reach the `work/` probe's **~3.4–4.1×** (SIMD
index + projecting two-stage, soa columns) — an autopsy pinned the remaining cost to the **walk**
(index build + per-token `src[idx[k]]` gather + `rec_cols` + key scan-back + per-value parse), which
the general decoder pays and the probe's inlined positional sum did not. The lean index was the
autopsy's first fix (index build 47→18 ms vs the quote-heavy structural index). Run this before/after
every parser change and watch the ratios; when a result disappoints, autopsy — don't guess (see
`bench/README.md` methodology).

## Profile finding (2026-06-29, native, 1M rows)

`ALIGN_BENCH_PROFILE=1 bench/json_decode/run.sh native` adds decode-only entry points that return the
row count after `json.decode`. Measured:

```
full decode-only   91.376 ms; aggregate delta    0.684 ms
proj decode-only   60.780 ms; aggregate delta   -0.108 ms
```

The aggregate is below the noise floor at this scale; this benchmark is a parser/decoder benchmark.
The next parser work should target the stage-2 walk and value parsing, not the folded aggregate.
