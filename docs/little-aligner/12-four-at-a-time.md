# 12. Four at a Time

> 🌐 **English** · [Japanese](./ja/12-four-at-a-time.md)

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

> **The Twelfth Commandment**
>
> *Trust the pipeline to vectorize the bulk. But when the silicon matters, speak its language: vectors and masks, never branches.*
