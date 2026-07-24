# 13. Four at a Time

> 🌐 **English** · [Japanese](./ja/13-four-at-a-time.md)

**Q1.** You have two arrays: `[1, 2, 3, 4]` and `[10, 20, 30, 40]`. Add them pairwise.

**A1.** `zip(a, b).map(fn v { v.0 + v.1 }).to_array()`. `zip` is a pipeline source that walks both arrays in lockstep, handing each stage one pair.

---

**Q2.** How many additions does the CPU perform?

**A2.** Four. One for each pair. 

---

**Q3.** Can we do it in one?

**A3.** Yes, if the CPU has wide registers. We can pack four `i32`s into a single `vec4<i32>` and add them with a single `+`.

---

**Q4.** How do we ask for that?

**A4.** 
```align
v1: vec4<i32> := [1, 2, 3, 4]
v2: vec4<i32> := [10, 0, 30, 40]
v3 := v1 + v2
```
The annotation turns the literal into a vector — there is no separate constructor. `v3` now holds `[11, 2, 33, 44]`. One instruction, four results. (`v2` carries a zero on purpose — we will need it.)

---

**Q5.** What if we want to divide by `v2`, but only if the denominator is not zero?

**A5.** In scalar code, we use `if`. But vectors don't branch—they compute everything at once. We use a **mask**.

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
Where the mask is true, it picks the lane from `v2`; where false, from `ones` — so `safe_v2` is `[10, 1, 30, 40]`, and `ans` is `[0, 2, 0, 0]`. The second lane divided by the substituted `1` instead of trapping on `v2`'s `0`. The division never traps, and the CPU never branched.

---

**Q8.** Is it tedious to write this by hand for large arrays?

**A8.** Yes. That is why Align auto-vectorizes pipelines. The compiler writes the `vec` and `mask` for you when you use `.map()`. 

---

**Q9.** Then why learn `vec` and `mask`?

**A9.** Because auto-vectorization is a heuristic, not a guarantee. When you write a crypto algorithm, a custom hash, or a novel compression scheme, you must speak to the silicon directly. Align does not hide the machine.

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
v: vec4<i32> := xs.load(i)
dst.store(i, v * 2)
```

`load` reads four consecutive elements; `store` writes four. Both are bounds-checked. The register is a value, but memory remains explicit at the boundary.

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

**Q17.** `m := w > 8`. Which lanes are true, and what is `select(m, w, 0)`?

**A17.** The last two lanes are true. The selected vector is `[0, 0, 10, 13]`; scalar `0` broadcasts for the false side.

---

**Q18.** Reduce only the selected lanes.

**A18.** `w.sum_where(m)` is `23`. A masked reduction collapses lanes without first storing a smaller vector.

---

**Q19.** A ten-element slice feeds `vec4`. Which full-width starting indices are safe if we walk without overlap?

**A19.** `0` and `4`. Index `8` belongs to the scalar tail. Say the indices before writing the load; off-by-one errors are faster in SIMD too.

---

**Q20.** Choose pipeline or explicit vector:

- add one to ten million ordinary integers
- implement one fixed four-lane mixing round required by a hash design
- sum one SoA column

**A20.** Pipeline; explicit vector; pipeline. Start width-agnostic, and spend fixed-width syntax only where the algorithm itself speaks in lanes.

---

**Q21.** How should the rest of the program call a hand-vectorized kernel?

**A21.** Through ordinary slices and values. Keep `vec4` inside a small function so the caller describes data, not one machine's register plan. An escape hatch is healthiest when it has a narrow door.

---

> **The Thirteenth Commandment**
>
> *Trust the pipeline to vectorize the bulk. But when the silicon matters, speak its language: vectors and masks, never branches.*
