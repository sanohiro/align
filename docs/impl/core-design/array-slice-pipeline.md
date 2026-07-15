This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — array / slice / the pipeline

> 🌐 **English** · [Japanese](./ja/array-slice-pipeline.md)

## Overview

The data-oriented center (draft §7–§8): three collection forms and one processing vocabulary.
Every stage below fuses into a single counted loop; a pipeline **must terminate** in a reduction
or a materialization (a held middle is a compile error — no lazy value escapes). This file is the
surface + shape rules; the columnar layer is [soa-groupby.md](soa-groupby.md).

## The three collection forms

```text
[a, b, c]        fixed array [T; N] — stack slot, compile-time length, Copy
array<T>         dynamic array — heap/arena {ptr,len}, Move (deep-dropped for str elements);
                 produced by .to_array(), chunks, json.decode, partition, sort
slice<T>         borrowed view {ptr,len}, Copy, region = the data it points into
```

`bytes` is prose shorthand for `slice<u8>`, not a distinct type.

## Signatures (verified)

```text
xs.len()   -> i64        // direct length: str/string, slice, array (fixed = const), soa, buffer
xs[i]                    // index (bounds-checked abort): scalar elem / chunk slice / struct gather / vec lane
xs[a..b]   -> slice<T>   // range view; scalar elements only; either bound omittable
xs[i] = v                // scalar element write — needs mut local or out slice param
arr[i] = structval       // whole-struct element write (POD; Move structs into FIXED arrays only)
arr[i].f = v             // element-field write, nested paths ok; dynamic arrays: primitive leaf only
fn f(out dst: slice<T>)  // writable-slice param; caller passes a mut binding; no-alias enforced

// stages                          // terminals
xs.map(f)                          xs.sum() / .count() / .min() / .max()
xs.where(p) / .where(.flag)        xs.any(p) / .all(p)
xs.field                           xs.reduce(init, f)      // init FIRST
xs.scan(init, f)                   xs.to_array()           // materialize -> array<T>
xs.chunks(n)                       xs.map_into(dst)        // write into caller slice
                                   xs.sort() / .sort_by_key(f)   // materializing
                                   (evens, odds) := xs.partition(p)
```

Function arguments to stages: named `fn`, lambda `fn x { … }` / `fn acc, x { … }`, or the
`.field` projection forms. `reduce`/`scan` are **init-first** — the old trailing-init order was
removed outright (no alias survives, per the no-backward-compat rule).

## Type & ownership classification

- Fixed arrays are Copy values; **Move-element** fixed arrays (`[User{name}]` with owned fields)
  are rejected pending per-element drop.
- Dynamic `array<T>` is a Move type with recursive Drop (str-element arrays deep-free, #339
  precedent).
- Slices are Copy views; a `mut slice<T>` binding (or `out` param) is the one writable-view form.
- `.count()` is the *pipeline* length (composes with `where`); `.len()` is the direct read. Both
  exist on purpose — do not merge them.

## Effects

Stages and terminals are Pure given pure function arguments; purity of the argument is inferred
and demanded where it matters (`par_map`; and pipeline lambdas reject allocation-leaking forms —
`str + str`, `template` — as compile errors, see [string.md](string.md)).

## Errors & aborts

No `Result` in this area. Shape mistakes are compile errors (unterminated pipeline, arity
mismatch in a stage lambda, Move-element slicing/indexing, aliasing `out` args, `map_into`
source/dst overlap). Runtime aborts: index/range out of bounds, `map_into` length mismatch.
Empty input is an answer, never an error: `sum` 0, `count` 0, `any` false, `all` true; `min`/
`max` on a provably-empty filter yield the sentinel identity (branchless `where` reducers, #303).

## Regions

`region_of(xs[a..b]) = region_of(xs)`; `region_of(chunks elem) = region_of(source storage)` —
the storage-vs-element distinction from #297 (a str-array's *elements* may outlive the array
*storage*). `to_array`/`sort`/`partition` results are owned (no region). `map_into` writes
through the caller's region and **proves no-alias**: the caller-side out-disjointness check is
deliberately conservative after the #328 call-laundered-aliasing fix — do not loosen it without
re-running that adversarial case.

## Spec'd but not implemented

- Slicing/indexing **Move-element** collections ("slicing a collection of the Move type … not
  supported yet"); arrays of Move structs (per-element drop pending).
- Dynamic `array<Struct>` element-field writes with a **non-primitive leaf** (str/owned/nested-
  Move) — `StoreElemFieldPtr` is primitive-leaf-only (#316).
- Nested element write `arr[i].a.x = v` works; nested **soa** columns and element write via
  chained projections beyond the tested forms — see `08-nested-structs.md` deferred list.
- `soa` columns are not range-slicable via the generic path (column windows go through
  `s.field[a..b]`, which IS implemented — the gap is only the generic `check_slice_range` arm).

## Pitfalls

- P1 — **termination rule is a language invariant**, not a style lint: a bound
  `xs.map(f)` value would be a hidden loop-in-waiting. Any new stage must either terminate or be
  statically required to feed a terminal.
- P2 — **init-first everywhere**: any new fold-shaped API (`reduce`, `scan`, future `fold_*`)
  takes the seed first. Mixed conventions are how AIs mis-generate code.
- P3 — the `out` no-alias check must consider **sub-slices** of the same local (#302) and
  call-laundered views (#328) — both were real soundness holes; new writable-view surfaces must
  route through the same check.
- P4 — fixed-array indexing requires literal-or-variable receiver (MIR addresses the slot);
  don't "fix" a failing expression-receiver case by copying the array silently.
- P5 — unbound owned temporaries have path-local synthetic owners as of 2026-07-15, including
  view-retention and per-iteration loop cleanup. `chunks(n)` still materializes its header array
  even for direct `.len()`, index, or pipeline consumers; that remaining cost is tracked in
  [audit 13 §8.2](../13-string-array-allocation-short-input-audit.md#82-confirmed-p1--virtualize-chunks-for-direct-consumers).

## Test anchors

`m4.rs` (count/min/max/any/all), `mmv2.rs` (scan/sort), `lambda.rs` (stage lambdas, arity,
purity rejections), `map_into.rs` (+#328 aliasing cases), `out_params.rs` (no-alias, bounds),
`struct_index.rs` (element/field writes, nested paths), `tuples.rs` (partition destructure),
examples `pipeline.align`, `chunks.align`, `partition.align`, `sort_by_key.align`,
`owned_array.align`. Differential fuzzer covers reducer terminals (#326).
