# Explicit SIMD: vecN, masks, alignment

> 🌐 **English** · [Japanese](./ja/12-simd.md)

Start with pipelines over slices and SoA columns. The compiler can vectorize them with a width suited to the target. Use `vecN<T>` when you need to specify a fixed-width kernel lane by lane.

## `vecN<T>` — a SIMD register as a value

```align
fn main() -> i32 {
    a: vec4<i32> := [1, 2, 3, 4]
    b: vec4<i32> := [10, 20, 30, 40]
    c := a + b          // lane-wise addition: [11, 22, 33, 44]
    d := c * 2          // scalar broadcasts across lanes
    print(d[2])         // 66 — lane read (constant index)
    print(c.max())      // 44 — horizontal reduction
    return 0
}
```

`vec2` / `vec4` / `vec8` / `vec16` of a numeric scalar. Arithmetic is lane-wise; the number of machine instructions depends on the operation and target. Scalars broadcast; `v[i]` reads a lane; `mut` vectors allow lane writes (`v[1] = 99`). Element-wise `a.min(b)` / `a.max(b)`, and float math — `sqrt`, `abs`, `floor`, `ceil`, `round`, `trunc` — apply per lane. Two free functions cover the classic kernels: `dot(a, b)` and `fma(a, b, c)` (fused multiply-add, one rounding).

Integer lane semantics match the scalar language exactly: overflow wraps, division by zero aborts — no lane is ever undefined behavior.

## Masks and `select` — choosing each lane

Comparing vectors yields a `maskN<T>`; you use it as data, not as control flow:

```align
fn main() -> i32 {
    scores: vec4<i32> := [90, 45, 82, 60]
    m := scores > 80                        // mask4<i32>: [t, f, t, f]
    picked := select(m, scores, scores * 0) // lane-wise blend, no branch
    print(picked.sum_where(m))              // 172 — masked reduction: 90 + 82
    return 0
}
```

`select(mask, a, b)` chooses each lane from `a` or `b` according to the mask. `v.sum_where(mask)` sums only the selected lanes. A pipeline such as `xs.where(p).sum()` can use the same technique: replace rejected values with the additive identity, zero, to sum without a branch.

Both value arguments are evaluated before `select` chooses the lanes. It is not a lane-wise `if`: `select(d != 0, n / d, zero)` still divides by zero in any zero-denominator lane. Make the operands safe first, then choose the requested fallback result:

```align
fn main() -> i32 {
    numerators: vec4<i32> := [12, 20, 30, 40]
    denominators: vec4<i32> := [3, 0, 5, 0]
    ones: vec4<i32> := [1, 1, 1, 1]
    zero: vec4<i32> := [0, 0, 0, 0]
    valid := denominators != 0
    safe := select(valid, denominators, ones)
    quotients := select(valid, numerators / safe, zero)
    print(quotients.sum())    // 10: 4 + 0 + 6 + 0
    return 0
}
```

The first selection prevents invalid division. The second implements the chosen policy that a zero denominator contributes zero. `select` requires two vectors of the same type; unlike arithmetic, it does not broadcast a scalar fallback.

## Bridging memory and registers: `load` / `store`

```align
fn scale_add(xs: slice<f64>, ys: slice<f64>, out dst: slice<f64>) {
    x: vec4<f64> := xs.load(0)      // 4 lanes from the slice, bounds-checked
    y: vec4<f64> := ys.load(0)
    dst.store(0, x * 2.0 + y)       // scale and add all 4 lanes
}

fn main() -> i32 {
    xs := [1.0, 2.0, 3.0, 4.0]
    ys := [10.0, 10.0, 10.0, 10.0]
    mut out := [0.0, 0.0, 0.0, 0.0]
    mut d: slice<f64> := out
    scale_add(xs, ys, d)
    print(out.sum())                // 60.0
    return 0
}
```

`s.load(i)` reads N consecutive elements into a register (N from the annotation); `s.store(i, v)` writes lanes back through an `out` slice. Both are bounds-checked. This pair is how a hand-written kernel walks an array — typically in `chunks(N)` with a scalar tail.

For ten elements and `vec4`, complete non-overlapping loads start at `0` and `4`. A load at `8` would read through index `11` and fail the bounds check. Process indices `8` and `9` as scalars; applying a mask after the load cannot make the out-of-bounds read valid.

## `align(N)` — when the loads should be aligned

`align(64) xs := [...]` over-aligns an array's storage; `align(64) CacheLine { ... }` over-aligns a struct (and pads its array stride to match). Vector loads at provably aligned offsets then use the wider alignment. It composes with `layout(C)` for FFI-shared buffers. This is a micro-optimization with a real but small payoff — measure first.

## The two-tier rule

- **Tier 1 (default): pipelines.** `xs.map(f).where(p).sum()`, soa columns, `group_by`. Width-agnostic — the compiler picks lane counts per target, and the same source can vectorize on AVX2, NEON, or a scalable ISA. This tier follows your data.
- **Tier 2: `vecN`/`maskN`.** Use these for kernels such as dot products, FIR filters, or blending when you need to specify a fixed vector width and per-lane operations.

> **Optimization:** Tier-1 vectorization is target- and profile-dependent, not a semantic guarantee or a fixed byte threshold. Loop legality and input shape also matter. Use `alignc explain-opt` for the decision and `emit-llvm --stage optimized` for the resulting vector width.

Keep explicit SIMD in a small function with slice parameters, like `scale_add` above. Callers then need not depend on its vector width. Before writing that function, inspect the pipeline's generated code with `alignc emit-llvm --stage optimized`; vector types such as `<4 x i64>` show where vectorization occurred. Compare performance before replacing the pipeline.
