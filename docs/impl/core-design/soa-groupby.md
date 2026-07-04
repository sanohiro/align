This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — soa / group_by / dict_encode

> 🌐 **English** · [Japanese](./ja/soa-groupby.md)

## Overview

The columnar layer: `soa<T>` stores one contiguous column per struct field (draft §9); `group_by`
is the grouped-fold primitive; `dict_encode` interns a `str` key column once for reuse. This is
the layer that makes "decode → filter → aggregate" run at columnar-database speed from ordinary
code; the M6 benches pin ~8–10× over AoS scans for column-touch workloads.

## Signatures (verified)

```text
rows.to_soa()                    -> soa<T>       // AoS transpose; REQUIRES enclosing arena {}
json.decode(s)  (into soa<T>)    -> Result<soa<T>, Error>   // direct columnar decode, no transpose
s.len()                          -> i64
s.field                          -> slice<F>     // column projection; full pipeline applies
s.field[a..b]                    -> slice<F>     // column window
s[i]                             -> T            // gather one row (multi-column fetch)
s[i].field                       -> F            // one cell (IndexColumn)
s[i].field = v                                   // one-cell write (StoreColumn; needs mut binding)
s[i] = value                                     // whole-element scatter (gather+scatter)
s.where(.flag)                                   // column-predicate filter stage

s.group_by(.k).sum(.v)           -> (keys, sums)     // i64 key on soa, or str key on array<Struct>
s.group_by(.k).min(.v) / .max(.v) / .count()         // count takes NO field
xs.group_by(.name).agg(sum(.a), max(.b), count())    // fused multi-aggregate, ONE pass
xs.dict_encode(.name)            -> encoded          // intern str key column; reuse across group_bys
```

`group_by` results are a tuple of parallel columns: `g.0` = distinct keys, `g.1..` = one column
per aggregate, row-aligned with `g.0`.

## Type & ownership classification

- `soa<T>` is an **arena-resident view structure**: columns are allocated in the enclosing arena
  (`to_soa` outside an `arena {}` is a compile error — "'to_soa' allocates its column buffer in
  an arena"). It is not a Move type; it is region-bound data. Escape past the arena is rejected
  by the ordinary region rules.
- `T` must be a struct; `str` fields are allowed (column of views). `soa` of a non-struct is
  rejected in sema.
- Column projections (`s.field`) are plain slices carrying the soa's region.
- `group_by`/`agg` outputs are owned result columns (tuple of arrays) — usable after the pass.
- `dict_encode`'s `encoded` value borrows the source array; the follow-up `group_by` key **must
  match the encoded key** (mismatch rejected in sema).

## Effects

All pure computation — no I/O, no rng. `to_soa`/`group_by`/`agg`/`dict_encode` nodes are Pure;
they may appear anywhere purity is demanded, but note the arena requirement shapes *where* they
can run, not their effect class.

## Errors & aborts

No `Result` anywhere in this area: invalid shapes are **compile errors** (wrong source type,
non-i64 aggregate value, unknown aggregate name, empty `.agg()`, bare `group_by` with no
aggregate, `sum(.strfield)`), and there is no runtime failure mode (hash tables grow; empty
input yields empty key/value columns).

## Regions

`region_of(soa) = the enclosing arena`. `region_of(s.field) = region_of(s)`. `s[i]` gathers a
*value* (Copy struct) — no region unless `T` has `str` fields, in which case the gathered views
keep the column region (the storage-vs-element-region distinction from #297 applies: a `str`
column's *elements* may point at longer-lived text, e.g. the decoded JSON input, while the
*storage* is arena-bound).

## Spec'd / deferred, not implemented

- **soa-source `.agg(...)`** — first cut is str-key AoS `array<Struct>` only (`soa.rs`
  `group_by_agg_soa_source_is_rejected`). i64-key soa multi-aggregate is a recorded follow-up.
- `.agg` / `dict_encode` need a **dynamic** `array<Struct>` source (a fixed-size stack literal
  array is rejected; decode one or take an `array<T>` parameter).
- **Owned soa columns** (soa that outlives an arena), **`soa_slice<T>`** (windowed soa view —
  repr decided in #330, unification not a new type), **packed-bool columns** — post-M6 backlog,
  recorded in the roadmap and `open-questions.md`.
- `avg`/`median`/field-taking `count(.f)` inside aggregates — rejected by design (first cut);
  `avg` is a candidate follow-up, `median` needs a different algorithm class.

## Pitfalls

- P1 — **arena requirement is load-bearing**: columns are bump-allocated batch data; without the
  arena bound, drop tracking of per-column buffers would need a Move soa (deferred). Do not
  "fix" the compile error by making `to_soa` allocate on the heap silently.
- P2 — **`agg` reads AoS strided**: the fused pass gathers fields from row-major memory. It beats
  per-aggregate passes ~3×, but a pre-transposed soa source (when implemented) reads dense — do
  not benchmark-compare the two shapes as if equivalent.
- P3 — **dict_encode reuse discipline**: the win is hash-once; a second `dict_encode` of the same
  column costs the hash again. Bind the encoded value once, run all group_bys off that binding.
- P4 — **str columns are zero-copy views** into the decode input: keep the input (or the arena)
  alive as long as the soa; the region checker enforces this — expect the error at the escape
  point, not at decode.

## Test anchors

`crates/align_driver/tests/soa.rs` (columns, index/write, group_by forms, agg accept/reject
matrix, dict_encode reuse + key-mismatch rejection); `m5.rs` json→soa decode; guide examples
`examples/soa.align`, `examples/soa_json_str.align`. Perf pins: `bench/group_by_reuse/README.md`,
`bench/json_soa/README.md`.
