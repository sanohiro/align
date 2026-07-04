This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — vecN / maskN / align(N)

> 🌐 **English** · [Japanese](./ja/vec-mask.md)

## Overview

The explicit fixed-width SIMD tier (draft §9). Policy first, because it shapes every API choice:
the **pipeline is the width-agnostic main path** (auto-vectorized, scalable-ISA-ready); `vecN<T>`
/ `maskN<T>` are the **fixed-width kernel escape hatch**. MIR carries vectorization-enabling
*properties*, never a baked vector width — vector width is a permanent backend decision (settled,
2026-07-02 internal review). Nothing in this area may leak a width assumption into the main path.

## Signatures (verified)

```text
v: vecN<T> := [a, b, ...]        // N ∈ {2,4,8,16}; T numeric; literal under annotation
v + w, v - w, v * w, v / w, v % w    // lane-wise, one instruction each
v + s / s + v                        // scalar literal broadcasts (either side)
v == w, v > w, v < w, ...        -> maskN<T>
v[i]                             -> T            // lane read, constant index
v[i] = x                                          // lane write (mut binding)
v.min() / v.max()                -> T            // horizontal reduce
a.min(b) / a.max(b)              -> vecN<T>      // element-wise
v.sqrt()/abs()/floor()/ceil()/round()/trunc()    // per-lane float math
dot(a, b)                        -> T
fma(a, b, c)                     -> vecN<T>      // one rounding
select(m, a, b)                  -> vecN<T>      // lane blend
v.sum_where(m)                   -> T            // masked reduction

s.load(i)                        -> vecN<T>      // N consecutive slice elems; bounds-checked
s.store(i, v)                                     // through an out/mut slice; bounds-checked

align(N) xs := [...]                              // over-align array storage (power of two)
align(N) Struct { ... }                           // over-align struct; stride padded to N
```

## Type & ownership classification

`vecN<T>` and `maskN<T>` are **Copy scalar-class values** (register-sized aggregates): pass,
return, store freely; never on the move/drop/escape path. `maskN<T>` is nameable (annotation,
param, return). `align(N)` is an attribute, not a type — it composes with `layout(C)` in either
order.

## Effects

Pure, all of it. Vec kernels are `par_map`-eligible and pipeline-lambda-eligible.

## Errors & aborts

Lane semantics are **identical to scalar semantics** — this is a hard invariant, not an
optimization detail: integer lanes wrap on overflow; lane division by zero **aborts** (via the
same `align_rt_div_fail` guard, lane-checked); `INT_MIN / -1` wraps; float lanes are IEEE.
`load`/`store` out of bounds aborts. No UB in any lane, ever (#294/#318 closed the vec-div
residual).

## Regions

None — Copy values. `load` borrows the slice momentarily; `store` requires a writable (`mut`/
`out`) slice. The only region interaction is via the slices at the boundary.

## Spec'd but not implemented

- **`bitset`** (§18.1 catalog) — no implementation, no tests. Design open: relationship to
  packed-bool soa columns (post-M6 backlog) should be settled together.
- Scalar-**variable** broadcast is narrower than spec prose suggests: literal broadcast (`v * 2`,
  `10 + v`) is verified; broadcasting a scalar *binding* into a lane op was observed rejected
  ("type mismatch: f64 vs vec4<f64>") — spell it as an explicit splat vector or a literal until
  a splat form ships. (Guide ch12 was written within the verified subset.)
- Aligned-load propagation across function boundaries (cross-function provably-aligned slices) —
  deferred; only locally provable alignment upgrades the load today (#320).

## Pitfalls

- P1 — **do not add a width-generic `vec<T>`**: two-tier is settled. Anything width-agnostic
  belongs to the pipeline, where the backend picks lanes.
- P2 — **audit before hand-vectorizing**: `emit-llvm` on the pipeline version first; the usual
  outcome is that the fused loop already vectorized. Keep kernels behind slice-boundary
  functions (`fn kernel(src: slice<T>, out dst: slice<T>)`), scalar tail handled by the caller
  via `chunks(N)`.
- P3 — `align(N)` only ever *over*-aligns, and dynamic `array<align(N) S>` stays rejected until
  aligned heap allocation lands (#319) — the attribute is not a general allocator directive.
- P4 — mask element type must match the compared vectors (`mask4<i32>` from `vec4<i32>`
  comparisons); there is no cross-width or cross-type mask reuse.

## Test anchors

`examples/vec_simd.align`, `vec_mask.align`, `vec_mask_annot.align`, `vec_broadcast.align`,
`vec_sum_where.align`, `vec_minmax.align`, `vec_math.align`, `vec_fma.align`, `vec_dot.align`,
`vec_load_store.align`, `vec_lane_set.align`, `aligned_load.align`, `align_attr.align`; vec
lane-`%`/div-guard tests around #318; differential fuzzer lane-arith extension (#326). M6
completion pins: real `<N x T>` IR + branchless `where` for every reducer (#303, #327).
