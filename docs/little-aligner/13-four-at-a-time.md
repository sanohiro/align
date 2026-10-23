# 13. Four at a Time

> 🌐 **English** · [Japanese](./ja/13-four-at-a-time.md)

**Q1.** You have two arrays: `[1, 2, 3, 4]` and `[10, 20, 30, 40]`. Add them pairwise.

**A1.** `zip(a, b).map(fn v { v.0 + v.1 }).to_array()`. `zip` is a pipeline source that walks both arrays in lockstep, handing each stage one pair.

---

**Q2.** How many element additions are required?

**A2.** Four. One for each pair. 

---

**Q3.** Can SIMD express them as one vector operation?

**A3.** Yes. We can pack four `i32`s into a `vec4<i32>` and add them with one `+`. The target, lane type, and width determine whether a vector operation becomes one or several machine instructions.

---

**Q4.** How do we ask for that?

**A4.** 
```align
v1: vec4<i32> := [1, 2, 3, 4]
v2: vec4<i32> := [10, 0, 30, 40]
v3 := v1 + v2
```
The annotation turns the literal into a vector — there is no separate constructor. `v3` now holds `[11, 2, 33, 44]`. One vector addition, four results. (`v2` carries a zero on purpose — we will need it.)

---

**Q5.** What if we want to divide by `v2`, but only if the denominator is not zero?

**A5.** In scalar code, we can test one denominator with `if`. Here we need a separate decision for each lane. We use a **mask**.

---

**Q6.** What is a mask?

**A6.** A vector of lane-wise booleans. `m := v2 != 0` compares every lane against the broadcast scalar `0` at once, and gives us a `mask4<i32>` — true in every lane except the second, where `v2` is `0`. A mask is only ever born from a comparison — you never write one by hand.

---

**Q7.** How do we use it to avoid dividing by zero?

**A7.** We `select`.
```align
ones: vec4<i32> := [1, 1, 1, 1]
safe_v2 := select(m, v2, ones)
ans := v1 / safe_v2
```
Where the mask is true, it picks the lane from `v2`; where false, from `ones` — so `safe_v2` is `[10, 1, 30, 40]`, and `ans` is `[0, 2, 0, 0]`. The second lane divides by the substituted `1`. Every denominator is now nonzero. Signed overflow still wraps, as in scalar arithmetic: the minimum value divided by `-1` yields that minimum value, and its remainder is `0`.

---

**Q8.** Is it tedious to write this by hand for large arrays?

**A8.** Yes. For suitable pipeline operations, the compiler can generate vector instructions automatically. We can write the element transformation with `map` and leave the vector width and remaining elements to the compiler.

---

**Q9.** Then why learn `vec` and `mask`?

**A9.** Auto-vectorization is not guaranteed. Some algorithms call for specific lane operations, and some kernels benefit from explicit SIMD after measurement. Vectors and masks let us express those operations ourselves.

---

**Q10.** What is `v1 * 2` when `v1` is a vector?

**A10.** The scalar `2` broadcasts to every lane. One vector operation multiplies all four values; you do not need to write `[2, 2, 2, 2]`.

---

**Q11.** What is the difference between `a.max(b)` and `a.max()`?

**A11.** With an argument, `max` is lane-wise and returns a vector. With no argument, it reduces across the lanes and returns one scalar. Punctuation is small; the shape of the answer is not.

---

**Q12.** Our vectors began as literals. How does a real kernel bring four adjacent array elements into a register?

**A12.** Through a slice:

```align
fn double4(xs: slice<i32>, out dst: slice<i32>, i: i64) {
    v: vec4<i32> := xs.load(i)
    dst.store(i, v * 2)
}
```

`load` reads four consecutive elements; `store` writes four. Both are bounds-checked. The destination belongs to the caller and arrives as `out dst`, just as it did for `map_into` in chapter 5.

---

**Q13.** The slice has ten elements. May a `vec4` kernel load at `i == 8`?

**A13.** No — that would ask for elements 8 through 11 and abort on the bounds check. Process two full groups of four, then handle the final two elements as a scalar tail. SIMD never grants permission to read past the data.

---

**Q14.** Does a mask by itself make a dangerous operation safe?

```align
select(v2 != 0, v1 / v2, v1 * 0)
```

**A14.** No. The division is an argument to `select`; it is computed before the lanes are blended, including the zero lane. First use the mask to select a safe denominator, as Q7 did, then divide. A mask chooses results. It does not erase work already requested.

---

**Q15.** What price do we pay for choosing `vec4` ourselves?

**A15.** Fixed width becomes part of the kernel. The best width may differ on another target, and tails are now our responsibility. Keep explicit SIMD small and behind slice-shaped functions; let ordinary callers and most loops remain width-agnostic pipelines.

---

**Q16.** Compute lane by lane:

```align
v: vec4<i32> := [1, 2, 3, 4]
w := v * 3 + 1
```

**A16.** `[4, 7, 10, 13]`. The two scalars each broadcast; multiplication happens before addition in every lane.

---

**Q17.** `m := w > 8`. Which lanes are true, and how do we zero the rest?

**A17.** The last two lanes are true, and the false side needs a real vector:

```align
zero: vec4<i32> := [0, 0, 0, 0]
select(m, w, zero)
```

`[0, 0, 10, 13]`. Q16's arithmetic broadcasts a scalar (`w * 3 + 1`), but `select` needs **two vectors**. `select(m, w, 0)` is a type error; give it vectors of the same type to choose between.

---

**Q18.** Reduce only the selected lanes.

**A18.** `w.sum_where(m)` is `23`. A masked reduction collapses lanes without first storing a smaller vector.

---

**Q19.** A ten-element slice feeds `vec4`. Which full-width starting indices are safe if we walk without overlap?

**A19.** `0` and `4`. The elements from index `8` onward form the scalar tail. Check the indices before writing the load.

---

**Q20.** Choose pipeline or explicit vector:

- add one to ten million ordinary integers
- implement one fixed four-lane mixing round required by a hash design
- sum one SoA column

**A20.** Pipeline; explicit vector; pipeline. Start width-agnostic, and spend fixed-width syntax only where the algorithm itself speaks in lanes.

---

**Q21.** How should the rest of the program call a hand-vectorized kernel?

**A21.** Through ordinary slices and values. Keep `vec4` inside a small function so callers can use it without depending on the vector width.

---

**Q22.** A float comparison chose the best scores. Can its mask also choose integer indices?

**A22.** Yes, when lane count and bit width agree. A `mask4<f32>` may select `vec4<i32>` indices or gate their `sum_where`, as well as selecting `vec4<f32>` scores. It cannot gate `vec4<i64>`: those lanes are twice as wide.

---

**Q23.** Will a floating-point sum automatically reorder itself to make a faster SIMD reduction?

**A23.** The default keeps its evaluation order. If different rounding is acceptable, write the permission:

```align
fn total(xs: slice<f64>) -> f64 = float(reassoc) { xs.sum() }
```

This permits regrouping; it does not guarantee vectorization. `float(contract)` separately permits an eligible multiply and add/subtract to fuse. Functions and lambdas begin strict, so a caller's permission does not silently change a callee. The [SIMD guide](../guide/12-simd.md) explains the numerical limits and checked byte views.

---

> **The Thirteenth Commandment**
>
> *Start with a pipeline. Use explicit vectors when the algorithm needs lane operations, and check bounds and operands before using them.*
