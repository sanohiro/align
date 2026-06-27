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

## Result (2026-06-27, native)

The harness times three pipelines on the same JSON: Align `→ soa` (with transpose), Align `→ array`
(AoS, no transpose), and Rust `serde_json → Vec`. Two snapshots:

**First measurement — Align ≈0.6× (LOSES):**

```
  records    soa ms    aos ms   rust ms   soa/rust   aos/rust
   100000     23.3      22.8     14.3       0.62x      0.63x
```

**Decomposition finding:** `soa` and `aos` are nearly identical → **the transpose is NOT the
bottleneck**; the gap is the **parser** (Align's `aos` parse, directly comparable to `serde → Vec`,
was itself only ~0.63×). So the dominant lever is parser speed, not dropping the transpose.

**After hand-rolling integer parsing** (`integer()` was `str::from_utf8(..).parse::<i64>()` — UTF-8
validation + a generic parse + a second pass over the digits; replaced with a single-pass
`checked` digit accumulation, the JSON hot path for int fields):

```
  records    soa ms    aos ms   rust ms   soa/rust   aos/rust
    10000      1.77      1.70     1.45      0.82x      0.85x
   100000     17.1      17.4     14.4      0.85x      0.83x
  1000000    230       192      188        0.82x      0.98x
```

≈0.61× → **≈0.82–0.85×** (AoS approaches parity at 1M). One clean scalar change closed most of the
gap, confirming the parser is where the analytics workload is won or lost.

**Remaining headroom to reach/beat `serde_json`** (recorded in `docs/open-questions.md`):

- **More scalar-parser tuning** — avoid the per-element zeroing memset (all declared fields are
  required, so the AoS buffer is fully overwritten), tighten `peek`/whitespace/string scanning.
- **A SIMD / structural JSON parser** — the bigger lever (runtime CPU-dispatch / simdjson-class).
- **Two-pass count-then-direct-column-fill** — drops the AoS intermediate + transpose; small (the
  transpose is cheap per the decomposition) but removes an alloc + pass at large N.
- **Field-skip / narrow struct** — don't parse unread columns; already available.

Honest takeaway: Align beats Rust on the *aggregation* layout (flat `bench/` `col_sum` ~8–10×); the
*decode* was parse-bound and is now ≈0.82× after the integer-parse fix — close, not yet ahead.
