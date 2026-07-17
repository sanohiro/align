# 8. Whose Is It?

> 🌐 **English** · [Japanese](./ja/08-whose-is-it.md)

**Q1.**

```align
a := 42
b := a
```

How many forty-twos exist now?

**A1.** Two. Numbers are **Copy**: assignment duplicates. Each goes its own way.

---

**Q2.**

```align
a := "hello".clone()    // an owned string
b := a
```

How many owned strings exist now?

**A2.** One. A `string` owns a heap buffer, and owners are **Move**: the buffer *changed hands*. `b` has it.

---

**Q3.** Then what does `print(a.len())` do, right after?

**A3.** It does not compile: *use of moved value `a`*. The old name is dead. One buffer, one owner, no exceptions.

---

**Q4.** Why so strict?

**A4.** Because one owner means the compiler knows exactly who frees, and when. No garbage collector to wait for, no double-free to fear, no lifetime annotations to write. The strictness *is* the memory management.

---

**Q5.** But I truly want two strings.

**A5.** Then truly pay: `b := a.clone()`. A deep copy, spelled out where the cost is. In Align you may copy anything — you may just never copy *invisibly*.

---

**Q6.** Which types are Move?

**A6.** The owners: `string`, `array<T>`, `buffer`, `box`, file handles. Everything plain — numbers, `bool`, views, small structs of them — is Copy. Own a resource: Move. Just data: Copy. And a struct that *contains* a `string` becomes Move too — ownership soaks upward.

---

**Q7.** What is `"hello"` before any `.clone()` — who owns the literal?

**A7.** Nobody; it is a `str` — a **view**: a pointer and a length, looking at bytes that outlive it. Views are Copy — copying a *look* at data is free.

---

**Q8.** Now the arena. What does this print?

```align
fn shout(name: str) -> string {
    arena {
        s := template "hey, {name}!"
        return s.clone()
    }
}

print(shout("align"))
```

**A8.** `hey, align!`. Inside `arena { }`, the template allocates into the arena. At `}`, the whole arena frees in one motion. The `.clone()` copied the survivor out first. String `+` is a compile error; a builder is the one concatenation path.

---

**Q9.** What if we `return s` — without the clone?

**A9.** It does not compile: *cannot return a value allocated in an arena*. The compiler knows `s`'s region and the `}` where it dies. Escape is a copy, and copies are visible: `.clone()`.

---

**Q10.** When do I reach for an arena?

**A10.** When a *phase* allocates many things that die together — parse this file, handle this request, decode this batch. One `arena {}` around the phase; temporaries cost a pointer bump each; cleanup is one line long and impossible to forget.

---

**Q11.** In other languages, a garbage collector finds what I dropped and cleans it up. Why not use that instead of an `arena`?

**A11.** A garbage collector is a janitor that follows your program around, inspecting and picking up individual pieces of trash. An `arena` is a building. When the work is done, you demolish the building. It is not about cleaning up mistakes; it is about planning lifetimes in bulk from the start.

---

**Q12.** If I create 10,000 temporary strings in a loop and free them one by one, what is the cost?

**A12.** In other languages, 10,000 trips to the OS allocator, 10,000 thread locks, and 10,000 cache misses to track memory metadata. In an `arena`, 10,000 strings cost exactly one pointer addition each. You bypass the OS completely. The arena is not just about cleanup; it is about *blinding allocation speed*.

---

**Q13.** So the whole decision, for any new data?

**A13.** One question — *how long does it live?*

- this scope → a plain value, done
- this phase → the arena, `.clone()` the survivors
- longer, one owner → an owned type, moved along
- I'm only looking → a view, free

---

**Q14.** Whose is it, then — this very buffer, at this line?

**A14.** Read the source: the last binding it moved into. Ownership in Align is not a runtime mystery; it is written down, and the compiler already checked your reading.

---

> **The Eighth Commandment**
>
> *One owner at a time. Group the short-lived under an arena. And when you must have two — `.clone()`, where all can see.*
