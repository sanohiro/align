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

**Direct SoA decode implemented (2026-06-29).** `json.decode → soa<Struct>` no longer decodes into an
AoS `array<Struct>` and then transposes. The runtime (`align_rt_json_decode_soa`) does a **two-pass
count-then-direct-column-fill**: pass 1 counts records (over the SIMD structural index) so the
column offsets — which depend on the row count — can be computed, then pass 2 parses each record's
values straight into arena-allocated columns (sharing the AoS path's Mison speculation via a generic
`FieldDst`). No AoS intermediate buffer, no heap materialize-copy, no transpose loop.

Result at 1M rows (native), SoA full decode + `where(.active).pay.sum()`:

```
            soa ms    aos ms   rust ms   soa/rust
before       ~104       ~89      ~86       ~0.82x
after        ~83.5      ~94      ~86       ~1.03x
```

So the SoA path went **≈0.82× → ≈1.03× of `serde_json`** — it now beats serde on the end-to-end
column-analytics workload at 1M. The profile decomposition shows the direct fill is even ~8–9 ms
*faster* than the AoS decode-only path, because AoS still pays a heap materialize-copy of the whole
array while the direct fill bump-writes columns in the arena. (The earlier 10–25 ms transpose penalty
this benchmark measured is now gone — it drove this change, exactly the benchmark's job.)

**Remaining headroom to widen the lead over `serde_json`** (recorded in `docs/open-questions.md`):

- **A SIMD / structural JSON parser** — the bigger lever (runtime CPU-dispatch / simdjson-class); the
  decode is still value-parse-bound.
- **More scalar-parser tuning** — tighten `peek`/whitespace/string scanning; the count pass adds one
  cheap structural walk that could be folded into the index build.
- **Field-skip / narrow struct** — don't parse unread columns; already available.

Honest takeaway: Align beats Rust on the *aggregation* layout (flat `bench/` `col_sum` ~8–10×), and
now also on the *decode → soa → aggregate* pipeline (≈1.03×) after dropping the AoS materialization.
