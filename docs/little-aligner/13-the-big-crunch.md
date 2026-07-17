# 13. The Big Crunch

> 🌐 **English** · [Japanese](./ja/13-the-big-crunch.md)

**Q1.** You are handed a 10-gigabyte log file. The goal: count how many times each `user_id` appears, but only for `status == 200`.

**A1.** We need to read it, parse it, filter it, group it, and count it.

---

**Q2.** Object-oriented instinct says: `file.read_to_string()`. 

**A2.** And your machine begs for mercy. You just allocated 10 gigabytes on the heap, copying the data from the OS buffer into your program's memory.

---

**Q3.** So we stream it line by line? `for line in file.lines() { ... }`

**A3.** Better. But parsing strings into `LogLine` objects one by one still creates thousands of small objects, thrashing the cache and breaking the pipeline.

---

**Q4.** Then how does Align want us to read 10 gigabytes?

**A4.** 
```align
view := fs.read_bytes_view("access.log")
```
This is an OS-level memory map (`mmap`). The file is mapped directly into your address space. No bytes are copied. We are simply pointing at the disk and calling it memory.

---

**Q5.** But it is just a slice of `u8`. How do we get columns from it?

**A5.** We decode it sideways, directly into an arena:
```align
arena {
    logs: soa<Log> := json.decode_many(view)?
    ...
}
```

---

**Q6.** Does `decode_many` allocate a string on the heap for every URL in the logs?

**A6.** No. Strings in Align are `slice<u8>`. The parser simply points those slices into the `view` we already have. Zero copies.

---

**Q7.** Now we have `logs`, which is `soa<Log>`. We only want `status == 200`.

**A7.** We use what we learned in Chapter 3:
```align
valid_logs := logs.where(fn l { l.status == 200 })
```

---

**Q8.** Is `valid_logs` a new SoA with copied data?

**A8.** No. It is a filtered view. No memory was moved.

---

**Q9.** Finally, count the occurrences of `user_id`.

**A9.** Chapter 10 gives us the answer:
```align
results := valid_logs.user_id.group_by().count()
```

---

**Q10.** Let's put it all together in one breath.

**A10.**
```align
view := fs.read_bytes_view("access.log")

arena {
    results := json.decode_many::<Log>(view)?
        .where(fn l { l.status == 200 })
        .user_id
        .group_by()
        .count()
        
    print(results)
}
```

---

**Q11.** How many heap allocations did this pipeline perform?

**A11.** Zero. The OS mapped the file. The arena held the SoA layout. The pipeline fused the filter and grouping into a single fast loop, processing columns directly from the memory map.

---

**Q12.** What happens when the `arena` block ends?

**A12.** The columns vanish instantly. No garbage collector scans them. No destructors walk a tree. The memory pointer simply resets. 

---

**Q13.** You have just processed 10 gigabytes in a fraction of a second, with zero allocations, using exactly one loop.

**A13.** That is The Big Crunch. That is why Align has no objects, no hidden allocations, and no classes. 

---

> **The Thirteenth Commandment**
>
> *Do not bring the data to the objects. Lay the data flat, map it from the earth, and let the pipeline flow over it.*
