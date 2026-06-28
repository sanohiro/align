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

## Target (the rewrite)

A `work/` probe (a SIMD structural index + a projecting two-stage decode, validated for correctness
and benchmarked against `serde_json` on this exact data) reaches **~3.4–4.1×** over `serde_json`
(and **~3.2–3.9×** when the decode materializes into soa columns). The rewrite lands that here: the
`proj×` / `full×` columns should climb from ~1× toward those figures as each slice merges. Run this
before/after every parser change and watch the ratios.
