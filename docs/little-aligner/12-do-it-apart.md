# 12. Do It Apart

> 🌐 **English** · [Japanese](./ja/12-do-it-apart.md)

**Q1.** We know how to do one thing to many values:

```align
ys := xs.map(expensive).to_array()
```

What if the values may be worked on at the same time?

**A1.**

```align
ys := xs.par_map(expensive)
```

`par_map` says the parallelism out loud.

---

**Q2.** Does `par_map` need `.to_array()`?

**A2.** No. It materializes an owned result array itself. Workers need places to put their answers, and the visible parallel boundary is also the visible materialization boundary.

---

**Q3.** Given a Pure function, may `map(f).to_array()` and `par_map(f)` answer differently?

**A3.** No. Same input positions, same output positions, same values. Parallelism changes how the work is scheduled, not what the program means.

---

**Q4.** Then what is `[3, 1, 2].par_map(fn x { x * x })`?

**A4.** `[9, 1, 4]`. Workers may finish in another order, but results return to input order. Completion order is not data order.

---

**Q5.** May the callable print each value?

**A5.** No. Which worker would print first? An observable answer would depend on scheduling. `par_map` requires a Pure callable: no I/O and no mutation of outside state.

---

**Q6.** Must we write `pure fn`?

**A6.** Never. The compiler infers purity from what the function does. If a call reaches I/O, rng, FFI, `unsafe`, or outside mutation, the compiler can show the path that made it Impure.

---

**Q7.** May a parallel lambda use an outside `factor`?

```align
factor := 3
ys := xs.par_map(fn x { x * factor })
```

**A7.** Yes. The closure captures `factor` by value. Every task receives a fact, not a shared box that another task may change.

---

**Q8.** What if `factor` was a `mut` binding and changes later?

**A8.** The closure still holds the value captured when it was made. Chapter 2's little rule has become a parallel safety rule: no shared mutable environment exists.

---

**Q9.** A million integers, and `f(x) = x + 1`. `par_map`?

**A9.** Usually no. Sequential `map` fuses, vectorizes, and avoids worker dispatch and result materialization. Many elements do not by themselves make expensive work.

---

**Q10.** A thousand independent images, each needing a costly transform. `par_map`?

**A10.** A good candidate, if measurement agrees. Parallelism has an entrance fee; expensive Pure work gives it time to pay the fee back.

---

**Q11.** Six numbers. Sum them in three independent hands of two:

```align
fn chunk_sum(c: slice<i64>) -> i64 = c.sum()
```

Finish the expression.

**A11.**

```align
total := [1, 2, 3, 4, 5, 6]
    .chunks(2)
    .par_map(chunk_sum)
    .sum()
```

The three partial answers are `3`, `7`, and `11`; the final answer is `21`.

---

**Q12.** Why make chunks instead of one task per number?

**A12.** A task needs enough work to repay scheduling. A chunk also keeps nearby data together. Grain size is the bridge between the algorithm's many elements and the machine's few workers.

---

**Q13.** If the chunks finish as third, first, second, may the final sum change?

**A13.** Not for this integer sum. `par_map` restores result order, and the final reduction is sequential. More generally, never use hidden completion order as an input; if order matters, keep it in the data.

---

**Q14.** Now the jobs are different: fetch a profile, load a model, and read a configuration. `par_map`?

**A14.** No common element function is being mapped. These are a few heterogeneous tasks — use a `task_group`.

---

**Q15.** What does this print?

```align
base := 100
task_group {
    a := spawn(fn { base + 5 })
    b := spawn(fn { base * 2 })
    wait()
    print(a.get() + b.get())
}
```

**A15.** `305`. The tasks may run in either order; `wait()` joins both; `.get()` reads their completed values.

---

**Q16.** May either task keep running after the `task_group` ends?

**A16.** No. The block is their lifetime. No detached task and no forgotten join can escape the source shape.

---

**Q17.** One task returns `Err`. Does `wait()?` abandon the others?

**A17.** No. It joins every task first, then propagates the first failure through the ordinary `?` door. Structured concurrency means cleanup and failure agree on the same boundary.

---

**Q18.** Choose:

- cheap arithmetic over a slice
- the same expensive Pure function over many independent items
- three different independent operations

**A18.** Sequential pipeline; `par_map`; `task_group`. SIMD lanes, data-parallel workers, task-parallel workers — three scales, three visible choices.

---

**Q19.** Which words in Align mean “another thread runs this”?

**A19.** `par_map` and `spawn`. Nothing else. If you cannot point to one of those words, the program did not secretly create parallel work.

---

> **The Twelfth Commandment**
>
> *Give expensive Pure work to `par_map`, and different independent work to a `task_group`. Give every task a scope, and every failure a join.*
