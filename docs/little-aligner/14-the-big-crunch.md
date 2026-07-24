# 14. The Big Crunch

> 🌐 **English** · [Japanese](./ja/14-the-big-crunch.md)

**Q1.** You are handed a 10-gigabyte log file. The goal: count how many requests each `user_id` made — and, while we are at it, how many of the whole 10 gigabytes came back `status == 200`.

**A1.** We need to read it, parse it, filter it, group it, and count it.

---

**Q2.** Object-oriented instinct says: `file.read_to_string()`. 

**A2.** And your machine begs for mercy. You just allocated 10 gigabytes on the heap, copying the data from the OS buffer into your program's memory.

---

**Q3.** So we stream it line by line, the way another language would? `for line in file.lines() { ... }`

**A3.** Better. But parsing strings into `LogLine` objects one by one still creates millions of small objects, thrashing the cache and breaking the pipeline.

---

**Q4.** Then how does Align want us to read 10 gigabytes?

**A4.** 
```align
arena {
    view := fs.read_file_view("access.log")?
    ...
}
```
For a regular, non-empty file, this is an OS-level memory map (`mmap`). It avoids making a second user-space buffer up front; pages arrive through the OS page cache as they are touched. The view lives in the arena (chapter 8): a lifetime, stated up front. And the `?` is chapter 7: opening and validating a text file can fail, and Align will not let that stay invisible.

---

**Q5.** But it is just a window of text. How do we get columns from it?

**A5.** We decode it sideways, directly into the same arena:
```align
    logs: soa<Log> := json.decode(view)?
```

---

**Q6.** Does `json.decode` allocate a string on the heap for every URL in the logs?

**A6.** No. A decoded string column is a column of `str` **views** — each one simply points into the `view` we already have. Zero copies.

---

**Q7.** Now we have `logs`, which is `soa<Log>`. How many requests were `status == 200`?

**A7.** A column is a plain slice, so chapter 3 applies to it directly:
```align
    ok := logs.status.where(fn s { s == 200 }).count()
```

---

**Q8.** Did that `where` build a new, smaller array?

**A8.** No. `where` is a pipeline stage — it fuses with `count` into one pass over the column. No memory was moved.

---

**Q9.** And the request count per `user_id`?

**A9.** Chapter 10 gives us the answer:
```align
    g := logs.group_by(.user_id).count()
```
`g.0` is the column of distinct users; `g.1` is the count for each, row-aligned.

---

**Q10.** Let's put it all together in one breath.

**A10.**
```align
import std.fs
import core.json

Log { user_id: i64, status: i64 }

fn main() -> Result<(), Error> {
    arena {
        view := fs.read_file_view("access.log")?
        logs: soa<Log> := json.decode(view)?

        ok := logs.status.where(fn s { s == 200 }).count()
        g := logs.group_by(.user_id).count()

        print(ok)
        print(g.0.len())
    }
    return Ok(())
}
```

---

**Q11.** How much memory did this program churn through?

**A11.** Much less churn than a row-object design, but not zero memory. The file mapping occupies address space and resident pages as it is touched. The decoded columns take space proportional to the rows. Grouping needs state and result columns proportional to the number of distinct users. What disappeared were the copied input buffer, millions of little row objects, and filtered intermediate arrays.

---

**Q12.** What happens when the `arena` block ends?

**A12.** The mapped view is unmapped and the batch storage is released at the boundary. There is no tracing garbage collector searching for forgotten rows; the lifetime was the block we could point to from the start.

---

**Q13.** You have just processed 10 gigabytes with a handful of fused column passes and not one object.

**A13.** With no millions of per-row objects and no hidden intermediate arrays. That is The Big Crunch. The data stayed flat, the necessary batch and grouping memory stayed visible, and every pass had a reason.

---

**Q14.** So what does *zero-copy* mean — zero memory and zero work?

**A14.** Neither. It means a particular boundary did not duplicate the bytes. Mapping avoids an owned input copy; decoded `str` fields view those mapped bytes. Parsing still works, numeric columns still need storage, and the OS still moves pages through the memory hierarchy. Always name which copy disappeared.

---

**Q15.** What if we wanted only the global `status == 200` count and never needed grouping or a reusable batch?

**A15.** Then stream typed rows from the mapped text:

```align
rows: json.scanner<Log> := json.scan(view)
ok := rows.status.where(fn s { s == 200 }).count()?
```

Each row is decoded as it flows into the reducer; no `soa<Log>` batch is materialized. The reducer returns `Result` because malformed input may be discovered halfway through the stream.

---

**Q16.** Why not use that scanner for the original per-user `group_by` too?

**A16.** A scanner is deliberately for fused, non-materializing reductions; `group_by`, `sort`, and `to_array` are not scanner endings. If the job needs reusable columns or grouped materialization, build the batch and pay for it visibly. If one bounded answer can flow straight out, scan. “Zero-copy” is not one trick — it is refusing each unnecessary materialization in turn.

---

**Q17.** Choose the input shape: a 20-gigabyte NDJSON file, one final count, no sorting, no grouping, no reuse.

**A17.** Map it as a view and use `json.scan` into a fused reducer. The requested answer is bounded; a materialized batch is not.

---

**Q18.** Same file, but twenty reports repeatedly scan five hot fields.

**A18.** Decode those declared fields into `soa<T>` inside an arena, then reuse the columns. One batch allocation and parse can repay itself across many field-wise passes.

---

**Q19.** Same data, but the program needs one small record at a time and immediately sends it onward.

**A19.** Prefer the scanner. SoA wins on repeated column work; it is not a badge to attach to every large file.

---

**Q20.** A `str` field decoded from the mapped file must be returned after the arena ends. What is missing?

**A20.** Ownership. The field is only a view into the mapping. Clone the specific survivor into owned storage at the boundary, or redesign the caller to finish using it inside the arena.

---

**Q21.** The scanner reports malformed JSON after nine million good rows. May it return the partial count as if nothing happened?

**A21.** No. Its reducer returns `Result`; `?` passes the failure upward. Streaming saves materialization, not correctness. If an application wants partial answers, that policy needs an explicit interface and an explicit error decision.

---

> **The Fourteenth Commandment**
>
> *Do not bring the data to the objects. Lay the data flat, map it from the earth, and let the pipeline flow over it.*
