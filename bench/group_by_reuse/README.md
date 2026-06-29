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

## Result (2026-06-29, native, 1M rows, 4 aggregates)

```
  groups  distinct      a1 ms      a2 ms   naive ms   smart ms     a1/a2   smart/a2
     100       100     69.693     29.549     32.558      9.057     2.36x      0.31x
   10000     10000    114.939     48.121     67.644     19.393     2.39x      0.40x
 1000000    632390   1190.985    341.011    729.070    238.758     3.49x      0.70x
```

**The honest verdict — the reuse helps, but the mechanism as built does not beat fast Rust:**

- **a2 beats a1 (Align naive) by 2.4–3.5×.** The reuse is real: paying the string interning once and
  running the four aggregates on the dense-id column is materially faster than four full str-key
  group_bys. The win widens with cardinality (more distinct keys → more interning to amortize).
- **a2 also beats the *naive* Rust baseline** (4× `HashMap<&str>`): ~1.1× (100 groups) to ~2.1× (632k).
- **But a2 *loses* to the *smart* single-pass Rust** — `smart/a2` is **0.31–0.70×**, i.e. one-pass Rust
  is **1.4–3.2× faster** than a2. The design mandate is explicit that only a win over the *fast*
  baseline is honest, so this is the number that counts: **A2, as a batch of separate group_bys, does
  not beat idiomatic fast Rust.**
- The earlier **~19–21× projection was wrong** — it over-counted the interning cost relative to the
  per-aggregate scan.

### Why, and what it changes

Smart Rust hashes each key **once** and updates all four accumulators in a **single pass**. A2 hashes
once too (`dict_encode`), but then makes **four more passes** — each aggregate gathers its value column
and runs a full dense-id hash-aggregate (plus a per-call malloc). Five passes vs one. Reuse removes the
*re-hashing*, not the *re-scanning*.

So the benchmark redirects the roadmap (exactly its job, per the json→soa lesson): **the real lever is
"multiple aggregates in one pass"** — fuse the K aggregates into a single scan of the encoded ids that
fills K result columns — not `dict_encode` reuse on its own. That deferred sub-item is now the
*primary* A2 work, not a nice-to-have.

A2's remaining honest niche is **sequential / interactive** reuse: when the aggregates arrive over time
(can't be fused into one pass), re-using the encoding beats re-interning per query (the 2.4–3.5× a1/a2
gap). For a known batch, single-pass wins.
