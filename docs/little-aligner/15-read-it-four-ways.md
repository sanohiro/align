# 15. Read It Four Ways

> 🌐 **English** · [Japanese](./ja/15-read-it-four-ways.md)

**Q1.** Can one small program bring together what we have learned?

**A1.** Enough of it to learn how to read:

```align
Record { score: i64, valid: bool }

fn total(data: str) -> Result<i64, Error> {
    arena {
        rows: soa<Record> := json.decode(data)?
        return Ok(rows.where(.valid).score.sum())
    }
}
```

Read it once. Do not hurry.

---

**Q2.** Here are three decoded records: `(10, true)`, `(20, false)`, `(30, true)`. What does `total` answer?

**A2.** `Ok(40)`. Keep the valid rows, take their scores, add them. This is the **answer reading**: before asking how a program works, say what it means.

---

**Q3.** Why begin with the answer? Surely the machinery is the interesting part.

**A3.** Because an optimization that changes `40` is a bug, and a beautiful lifetime for the wrong result is still wrong. Meaning comes first. The other readings must preserve it.

---

**Q4.** Read it a second time. What path does the data take?

**A4.** Text enters. `json.decode` turns it into columns. `where(.valid)` keeps positions. `.score` projects one column. `sum()` collapses many scores to one number. This is the **flow reading**: name each transformation from left to right.

---

**Q5.** How many loops are hidden in `where(.valid).score.sum()`?

**A5.** One fused loop. But do not say the whole function is one pass: decoding the text is work of its own, followed by the fused pass over the columns. Count the work that is really there, neither more nor less.

---

**Q6.** Suppose `Record` also had `name`, `email`, and `notes`. Which columns would the fused pass read?

**A6.** `valid` and `score`. The other columns exist, but this question never touches them. Chapter 9's sideways layout and chapter 5's fusion are the same story now.

---

**Q7.** Read it a third time. Who owns each thing, and where does it die?

**A7.**

- `data` is a `str`, a borrowed view supplied by the caller.
- `rows` and its columns live in the arena and die at the closing `}`.
- the final `i64` is Copy and leaves the arena inside `Ok`.

This is the **lifetime reading**. Do not merely find values; find their homes and their last lines.

---

**Q8.** Why may the number leave the arena, while `rows` may not?

**A8.** The number contains its own bits. It points at nothing in the arena. `rows` refers to arena memory; returning it would leave a view of storage that has been freed. The type checker rejects that return.

---

**Q9.** What if the JSON is malformed?

**A9.** `?` returns `Err` from `total`. On the way out, the arena still ends and its memory is reclaimed. Error flow and lifetime agree; the early door does not become a leak.

---

**Q10.** Read it a fourth time. Where does the machine do work?

**A10.** It parses the input, allocates the columns by bumping the arena, then scans the two needed columns in one fused loop. It builds no filtered array and clones no string. This is the **work reading**: count passes, touched columns, allocations, copies, and exits.

---

**Q11.** How can we tell that no filtered array was built?

**A11.** There is no materializing ending between `where` and `sum`. If we wanted an array, we would have to say `.to_array()`. If we wanted a deep copy, we would have to say `.clone()`. Align makes those changes to the cost story pronounceable.

---

**Q12.** What happens if we write this instead?

```align
rows.where(.valid).score.to_array().sum()
```

**A12.** The answer stays the same. The work does not. `to_array()` visibly materializes the scores, and `sum()` then makes another pass over that array. Sometimes you need the array; here you do not. The answer reading agrees while the work reading catches the difference.

---

**Q13.** Could we replace `?` with `else`?

**A13.** Yes. An `else` branch can produce a fallback of the payload's type or exit the function, for example `else { return Err(Error.Invalid) }`. On a `Result`, `else` discards the original error. Use `?` to pass that error unchanged; use `match` to inspect it before deciding. Choose which failure or fallback the caller should receive.

---

**Q14.** Where would `loop` enter this program?

**A14.** Not in the column scan. The rows are already a known collection, and one act applies to many elements — a pipeline. A `loop` would belong at a control-shaped boundary: read another block until EOF, retry until ready, converge until stable.

---

**Q15.** And where would `match` enter?

**A15.** Where the data is one of many: inspect a detailed decode error, or process a sum-type field with several variants. `match` follows the shape of the value; `loop` follows the shape of control; a pipeline follows the flow of many values.

---

**Q16.** Four readings, then. Say them without looking back.

**A16.**

1. **Answer** — What value or failure does this mean?
2. **Flow** — How does data change, and where does control go?
3. **Lifetime** — Who owns each value, and where does it die?
4. **Work** — Which data does the machine read, how many passes, allocations, and copies does it make, and where does execution exit?

If one answer is foggy, you have found the part of the program you do not understand yet.

---

**Q17.** Why does Align make these four readings fit the same source?

**A17.** Because its unusual rules were never separate tricks. A pipeline makes flow plain and work fusible. `soa` makes touched columns plain. Move and arenas make lifetimes plain. `?`, `.clone()`, and `.to_array()` make exits, copies, and allocations plain. The language asks the human, compiler, and hardware to read one honest story.

---

**Q18.** Close the book for this one. The requirement changes: “return the number of valid scores over 100.” Change only the final expression.

**A18.**

```align
return Ok(rows
    .where(.valid)
    .score
    .where(fn s { s > 100 })
    .count())
```

If you wrote that from the sentence without copying an earlier chain, the pipeline chapters have become a tool.

---

**Q19.** New requirement: “return every valid score over 100.” What changes?

**A19.** The result type becomes `Result<array<i64>, Error>`. Merely writing `.to_array()` inside the original arena is not enough: that array would belong to the arena. Using the same `Record` declaration and `import core.json`, make the result in a helper without a local arena:

```align
fn collect_scores(rows: soa<Record>) -> array<i64> =
    rows.where(.valid).score.where(fn score { score > 100 }).to_array()

fn scores(data: str) -> Result<array<i64>, Error> {
    arena {
        rows: soa<Record> := json.decode(data)?
        return Ok(collect_scores(rows))
    }
}
```

`rows` is a borrowed SoA view for the helper's call. The helper's `to_array()` allocates an independent result: a callee does not inherit its caller's arena implicitly. Its elements are `i64` values, so they borrow nothing from the input columns. The caller receives the array after the temporary columns are released. An array of borrowed `str` elements would still depend on their text; owning an array does not make all its elements independent.

---

**Q20.** New requirement: “if decoding fails, return its error unchanged; otherwise return the count.” Which old choice must not change?

**A20.** Keep `Result` and `?`. An `else` fallback would discard the requested error. This decoder returns `Error`; it does not provide a row number or a detailed parse message merely because we propagate it.

---

**Q21.** New requirement: “process blocks until EOF; within each block, total valid scores.” Which two control shapes appear?

**A21.** A `loop` around the blocks and a pipeline inside each block. One problem may contain several shapes; mastery is giving each boundary its one fitting tool.

---

**Q22.** New requirement: “the input is huge and only one count is needed.” `soa` or scanner?

**A22.** Scanner. Do not preserve columns that no later question will reuse. The answer reading stayed small; the work and lifetime readings should become small with it.

---

**Q23.** New requirement: “twenty analyses reuse the same five columns.” Now?

**A23.** `soa` in an arena. Decode once, keep the batch for the phase, and let the analyses stream columns. The same language offers both answers because the workloads are genuinely different.

---

**Q24.** When have you learned an Align feature?

**A24.** Not when you can define it. When a new problem arrives and you can choose it, reject it, combine it with an older tool, and state its answer, lifetime, and cost before running the code.

---

> **The Fifteenth Commandment**
>
> *Read once for the answer, once for the flow, once for the lifetime, and once for the work. When all four stories agree, you understand the program.*

Now try a program of your own. Choose the data's shape, trace its flow, check its lifetime, and account for its work. [The guide](../guide/README.md) covers strings, modules, the standard library, and the toolchain as you need them.

*What will you make?*
