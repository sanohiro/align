# `group_by_reuse` — A2 dictionary-reuse duel (Align vs Rust `HashMap`)

Measures the **A2 dictionary-reuse rail**: `e := us.dict_encode(.name)` interns a `str` key column
**once** into a dense-id column + dictionary, then several `group_by(.name)` aggregates reuse the
encoding (integer-column work) instead of re-interning the strings per group-by. The workload is four
aggregates over the same key — `sum(.a)`, `sum(.b)`, `max(.c)`, `min(.d)` — over 1M rows.

```sh
bench/group_by_reuse/run.sh [baseline|v3|native]   # default native
```

Four contenders:
- **a1** — Align naive: four independent str-key `group_by`s (re-interns the key column 4×).
- **a2** — Align reuse: `dict_encode(.name)` once, then four group_bys on the dense-id column.
- **naive** — Rust: four separate `HashMap<&str, i64>` (re-hashes the keys 4×), `ahash`.
- **smart** — Rust: one pass, one `HashMap<&str, [i64; 4]>` — hashes each key **once** and updates all
  four accumulators in that probe (the fast idiomatic baseline; the honest competition for A2).

Same plumbing as `bench/group_by/`: the kernel is built with `alignc emit-obj` and the runtime is
linked as a **cdylib**. Each kernel function returns the input array so the C-ABI harness can thread
the same buffer across rounds (an `array<Struct>` is a Move type — a callee would otherwise drop it).
Value correctness is covered by the `dict_encode_reuse_matches_a1_string_group_by` unit test; this
harness measures time and asserts the array threads back unchanged.

## Result (2026-06-29, native, 1M rows, 4 aggregates) — with the fused `a3`

```
  groups  distinct      a1 ms      a2 ms      a3 ms   naive ms   smart ms     a1/a3   smart/a3
     100       100     70.13      30.14      21.83      33.41       9.23      3.21x      0.42x
   10000     10000    112.21      47.29      39.75      65.92      19.29      2.82x      0.49x
 1000000    632390   1174.81     339.76     316.36     712.76     243.81      3.71x      0.77x
```

`a3 = us.group_by(.name).agg(sum(.a), sum(.b), max(.c), min(.d))` — the **fused one-pass** rail (the
"multiple aggregates in one pass" lever the earlier benches called for): one scan interns each key once
(a fast FxHash-class hasher, not SipHash) and updates all four accumulators, exactly the
`HashMap<&str,[i64;4]>` shape smart Rust uses. No `dict_encode`, no re-scan.

**The honest verdict — fusion is the right lever; a3 is now the best Align path, but still trails fast Rust:**

- **a3 beats a1 (Align naive 4× group_by) by 3.2–3.7×** — the headline. Replacing four full str-key
  group_bys with one fused pass (+ a fast hash) is the structural win.
- **a3 also beats a2 (`dict_encode` reuse) everywhere** (~1.0–1.4×): one fused pass over the str key
  beats encode-once-then-four-id-passes for a known batch — and skips the encode/gather entirely.
- **But a3 still *loses* to smart single-pass Rust** — `smart/a3` is **0.42–0.77×** (Rust 1.3–2.4×
  faster). Per the mandate (only a win over the *fast* baseline is honest), a3 does not yet beat
  idiomatic fast Rust — but it is materially closer than a2 and is the right shape.

### Why a3 still trails smart Rust — measured (corrects an earlier guess)

Smart Rust hashes each key **once** and updates all four accumulators in a **single pass**. a1 makes
four full passes; a2 hashes once (`dict_encode`) but then makes four more id-passes. **a3 collapses
that to one pass** — the structural fix (cause 1: N passes → 1). The earlier note blamed the remaining
gap on the `n`-sized output `malloc`; two probes show otherwise:

- **Right-sizing the output buffers is a no-op.** A prototype that allocates the K+1 output columns at
  the exact group count (not the row count) left the benchmark unchanged — the over-allocated buffers
  are lazily paged, so only the `count` written entries ever fault in. Not the lever.
- **The hasher is the real lever.** Swapping the dependency-free FxHash for `ahash` (AES) moved
  `smart/a3` 0.77× → 0.92× at 632k groups and 0.41× → 0.61× at 100 — but even then a3 doesn't fully
  win at low cardinality, and `ahash` is a new dependency on the minimal runtime.
- **The smart baseline reads pre-extracted columns** (`gidx[i]` + contiguous `cols[j][i]`), while a3
  reads the AoS struct array strided — part of the low-cardinality gap is columnar-vs-AoS, not the
  aggregation.

Beating smart Rust is a cross-cutting "smart" pass (we trail it in other benches too), deferred to be
decided once: the hash strategy (`ahash` dep vs hand-rolled AES, across all str group paths), an
inline-value accumulator, and possibly an AoS-reading (fair) smart baseline. See
`docs/open-questions.md`.

So the benchmark redirects the roadmap (exactly its job, per the json→soa lesson): **the real lever is
"multiple aggregates in one pass"** — fuse the K aggregates into a single scan of the encoded ids that
fills K result columns — not `dict_encode` reuse on its own. That deferred sub-item is now the
*primary* A2 work, not a nice-to-have.

### Profile finding (2026-06-29, native, 1M groups)

`ALIGN_BENCH_PROFILE=1 bench/group_by_reuse/run.sh native` adds A2 decomposition entry points:

```
encode only        260.349 ms
+ 1 aggregate      274.017 ms  delta  13.669 ms
+ 2 aggregates     301.142 ms  delta  40.793 ms
+ 3 aggregates     320.630 ms  delta  60.281 ms
+ 4 aggregates     335.050 ms  delta  74.702 ms
```

At high cardinality, string `dict_encode` is the dominant cost; the four reused dense-id aggregates
are the secondary cost. So the roadmap has two measured levers: speed up string dictionary encoding,
and fuse the K aggregate scans into one pass. The latter is still required to compete with the smart
Rust baseline for a known batch.

A2's remaining honest niche is **sequential / interactive** reuse: when the aggregates arrive over time
(can't be fused into one pass), re-using the encoding beats re-interning per query (the 2.4–3.5× a1/a2
gap). For a known batch, single-pass wins.

## Direct-output follow-up (2026-07-16)

Single str-key aggregates now seed and update the caller's existing result columns directly;
`dict_encode` writes vacant representatives directly into its existing dictionary. This removes two
staging Vecs + two final copies from each single aggregate and one Vec + one copy from dictionary
encoding. The benchmark kernel's `Row` was also made public to satisfy the current exported-interface
check; that is a harness-only visibility repair.

Consecutive same-host native min-of-20 runs before and after the runtime change measured:

```text
 groups  distinct      a1 before/after       a2 before/after
    100       100       41.056 / 40.712 ms    19.872 / 20.386 ms
  10000     10000       94.913 / 93.658 ms    36.494 / 36.750 ms
1000000    632390      690.010 / 630.425 ms   200.940 / 194.669 ms
```

The short/low-cardinality cases stayed within 3%; at 632,390 distinct keys A1 improved 1.09x and A2
1.03x. Treat these consecutive runs as directional evidence, not a balanced AB/BA claim. The
structural result is exact: three internal Vec allocations and three final copies are gone across
the single-aggregate and dictionary shapes, while fused `a3` deliberately keeps its row-major
multi-aggregate accumulator.
