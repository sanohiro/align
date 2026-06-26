# `json_soa` — JSON → SoA analytics duel (Align vs Rust `serde_json`)

Measures the headline "analytics win": Align decoding a JSON array of records straight into a
column-major `soa<Row>` and running `where(.active).pay.sum()`, vs idiomatic Rust
(`serde_json::from_str::<Vec<Row>>` → `.filter().map().sum()`). The records have 4 fields; the
aggregate touches 2.

```sh
bench/json_soa/run.sh [baseline|v3|native]   # default native
```

Unlike the flat `bench/`, the kernel pulls in the Align runtime (the JSON parser + arena), so the
harness links `libalign_runtime.so` (a **cdylib** — dynamic, over the C-ABI, so its bundled std
doesn't collide with the harness's std the way the `.a` staticlib would). `serde`/`serde_json` are
ordinary cargo deps; this is a standalone cargo project (its own `[workspace]`), detached from the
compiler workspace.

## Result (2026-06-27, native) — Align currently **LOSES** ≈0.6×

```
   records     json KB      align ms       rust ms  speedup
     10000         498         2.20          1.36     0.62x
    100000        5083        22.6          13.8      0.61x
   1000000       51814       251           149        0.59x
```

**The workload is parse-bound, and Align's parser is the bottleneck.** Two compounding costs:

1. **Scalar JSON parser.** `align_rt_json_decode_struct_array` is a straightforward byte-at-a-time
   parser; `serde_json` is heavily optimized. Parsing dominates total time, so a slower parser loses
   regardless of layout.
2. **decode-to-AoS-then-transpose.** Align decodes into a heap AoS buffer, then transposes to the
   arena soa (an extra full pass + an extra allocation), where Rust does a single `Vec` parse.

The SoA column-scan advantage (reading 2 of 4 columns) is real — see the flat `bench/` `col_sum`
(~8–10× vs `Vec<Struct>`) — but here it is **swamped by the parse**, which both sides pay in full.

**So the json→soa "analytics win" is not real yet.** To make it real (recorded in
`docs/open-questions.md`):

- **A faster (SIMD / structural) JSON parser** — the runtime CPU-dispatch / simdjson-class work. This
  is the dominant lever; without it the layout win can't surface.
- **Two-pass count-then-direct-column-fill** — drop the AoS intermediate + transpose (Codex's
  refinement): a structural count pass for N, then parse values straight into the columns.
- **Field-skip / narrow struct** — don't parse columns the pipeline never reads (here `score`,
  `extra`); already available by declaring a narrower struct, and it cuts parse work on both sides.

Honest takeaway: Align beats Rust on the *aggregation* layout, but the *decode* is currently a net
loss — the analytics headline depends on parser work that isn't done.
