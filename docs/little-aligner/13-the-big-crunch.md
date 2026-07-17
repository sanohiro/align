# 13. The Big Crunch

> 🌐 **English** · [Japanese](./ja/13-the-big-crunch.md)

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
This is an OS-level memory map (`mmap`). The file is mapped directly into your address space — no bytes are copied. We are simply pointing at the disk and calling it memory. The view lives in the arena (chapter 8): a lifetime, stated up front. And the `?` is chapter 7: touching a disk can fail, and Align will not let that stay invisible.

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

**A11.** Almost none of its own. The OS mapped the file — zero copies. The columns live in the arena — bump allocations, no per-row objects, no bookkeeping. The only owned results are `ok`, one integer, and `g`, two short columns. No garbage collector ever ran, because there was never any garbage.

---

**Q12.** What happens when the `arena` block ends?

**A12.** The columns vanish instantly. No garbage collector scans them. No destructors walk a tree. The memory pointer simply resets. 

---

**Q13.** You have just processed 10 gigabytes with a handful of fused column passes and not one object.

**A13.** That is The Big Crunch. That is why Align has no objects, no hidden allocations, and no classes. 

---

> **The Thirteenth Commandment**
>
> *Do not bring the data to the objects. Lay the data flat, map it from the earth, and let the pipeline flow over it.*
