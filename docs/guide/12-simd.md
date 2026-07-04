# Explicit SIMD: vecN, masks, alignment

> 🌐 **English** · [Japanese](./ja/12-simd.md)

Read this chapter last among the performance chapters, because its first lesson is: **you usually don't need it.** Pipelines over slices and soa columns auto-vectorize — that is the main path, it is width-agnostic, and it follows your data to whatever hardware runs it. `vecN<T>` is the escape hatch for the remaining case: a fixed-width kernel you want to spell out lane by lane.

## `vecN<T>` — a SIMD register as a value

```align
fn main() -> i32 {
    a: vec4<i32> := [1, 2, 3, 4]
    b: vec4<i32> := [10, 20, 30, 40]
    c := a + b          // one instruction: [11, 22, 33, 44]
    d := c * 2          // scalar broadcasts across lanes
    print(d[2])         // 66 — lane read (constant index)
    print(c.max())      // 44 — horizontal reduction
    return 0
}
```

`vec2` / `vec4` / `vec8` / `vec16` of a numeric scalar. Arithmetic is lane-wise, one instruction per op; scalars broadcast; `v[i]` reads a lane; `mut` vectors allow lane writes (`v[1] = 99`). Element-wise `a.min(b)` / `a.max(b)`, and float math — `sqrt`, `abs`, `floor`, `ceil`, `round`, `trunc` — apply per lane. Two free functions cover the classic kernels: `dot(a, b)` and `fma(a, b, c)` (fused multiply-add, one rounding).

Integer lane semantics match the scalar language exactly: overflow wraps, division by zero aborts — no lane is ever undefined behavior.

## Masks and `select` — branchless by construction

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

`select(mask, a, b)` blends lanes; `v.sum_where(mask)` reduces only the lanes the mask keeps. This is the explicit spelling of what `where(p)` compiles to in pipelines — the same identity-select trick that makes `xs.where(p).sum()` branch-free.

## Bridging memory and registers: `load` / `store`

```align
fn scale_add(xs: slice<f64>, ys: slice<f64>, out dst: slice<f64>) {
    x: vec4<f64> := xs.load(0)      // 4 lanes from the slice, bounds-checked
    y: vec4<f64> := ys.load(0)
    dst.store(0, x * 2.0 + y)       // two instructions, 4 lanes each
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

## `align(N)` — when the loads should be aligned

`align(64) xs := [...]` over-aligns an array's storage; `align(64) CacheLine { ... }` over-aligns a struct (and pads its array stride to match). Vector loads at provably aligned offsets then use the wider alignment. It composes with `layout(C)` for FFI-shared buffers. This is a micro-optimization with a real but small payoff — measure first.

## The two-tier rule

- **Tier 1 (default): pipelines.** `xs.map(f).where(p).sum()`, soa columns, `group_by`. Width-agnostic — the compiler picks lane counts per target, and the same source vectorizes on AVX2, NEON, or a scalable ISA. This tier follows your data.
- **Tier 2 (escape hatch): `vecN`/`maskN`.** For the dot-product / FIR / blend kernel where you want to dictate the exact register dance at a fixed width you chose.

If you reach for tier 2, keep it in one small function with slices at the boundary, like `scale_add` above — callers shouldn't know a kernel is hand-vectorized. And audit tier 1 before abandoning it: `alignc emit-llvm` shows you the `<4 x i64>` types in the IR the pipeline already produced. The most common outcome of that audit is deleting the hand-written kernel.
