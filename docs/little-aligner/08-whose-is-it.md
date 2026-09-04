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

**A4.** With one owner, the compiler can track responsibility for freeing the resource. We need neither a garbage collector nor lifetime annotations in the source.

---

**Q5.** But I truly want two strings.

**A5.** Write `b := a.clone()`. This makes a deep copy of the string, so each binding owns its own buffer. The allocation and copy are explicit.

---

**Q6.** Which types are Move?

**A6.** Resource owners such as `string`, `array<T>`, `buffer`, and file handles. Numbers, `bool`, views, and structs of Copy fields are Copy. A struct that contains a `string` becomes Move too: it owns the string as part of its value.

---

**Q7.** What is `"hello"` before any `.clone()` — who owns the literal?

**A7.** The literal's bytes live for the whole program. Its type is `str`, a **view** consisting of a pointer and a length. Views are Copy: copying one copies the pointer and length, without duplicating the text.

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

**A10.** When a *phase* allocates many things that die together — parse this file, handle this request, decode this batch. Put one `arena {}` around the phase. It provides storage for the temporaries and releases that storage when the phase ends.

---

**Q11.** How does this differ from letting a garbage collector reclaim the memory?

**A11.** An arena fits the case where we already know that many values have the same lifetime. We put them in one region and release it when the phase ends. The program states that boundary instead of asking a garbage collector to determine which individual values are still reachable.

---

**Q12.** If I create 10,000 temporary strings in a loop and free them one by one, what is the cost?

**A12.** Separate allocations and frees each require allocator bookkeeping. An arena normally allocates by advancing a pointer within a block, obtaining another block when needed. It releases the storage together at the end. The strings still have to be built; the arena reduces allocation and cleanup overhead.

---

**Q13.** How should we choose when creating new data?

**A13.** One question — *how long does it live?*

- this scope → a plain value, done
- this phase → the arena, `.clone()` the survivors
- longer, one owner → an owned type, moved along
- I'm only looking → a view, with no copy of the underlying data

---

**Q14.** Whose is it, then — this very buffer, at this line?

**A14.** Read the source: the last binding it moved into. Ownership in Align is not a runtime mystery; it is written down, and the compiler already checked your reading.

---

**Q15.** One array, two slices:

```align
xs := [10, 20, 30]
a := xs[0..2]
b := a
```

How many arrays now exist?

**A15.** One. `a` and `b` are Copy views of the same two elements. Copying a view copies only its pointer and length, never the elements it sees.

---

**Q16.** May either view outlive `xs`?

**A16.** No. A view is cheap because it owns nothing, and safe because the compiler remembers what it borrows. Move answers *who will free this?* A view answers *whose lifetime am I inside?* You need both answers to read memory correctly.

---

**Q17.** Follow the owner:

```align
a := "red".clone()
b := a
c := b.clone()
```

Which names may still be used?

**A17.** `b` and `c`. Ownership moved from `a` to `b`, so `a` can no longer be used. `clone` made a separate buffer for `c` while leaving `b` intact.

---

**Q18.** Now `d := b`. Which names own strings?

**A18.** `c` and `d`. The original buffer moved from `b` to `d`; the cloned buffer stayed with `c`. Draw arrows if you must, but never draw two arrows to one owned buffer.

---

**Q19.** A function takes `string` by value. What happens when we call it with `d`?

**A19.** Ownership moves into the function, so the caller can no longer use `d`. The callee drops the string unless it moves ownership onward, for example by returning it. A returned string needs a new binding; it does not restore `d`.

---

**Q20.** The function only needs to read the text. Better parameter?

**A20.** `str`. Pass a view and keep the owner:

```align
fn count_bytes(s: str) -> i64 = s.len()
```

Pass a view while retaining the owner. Passing a `string` by value transfers ownership; passing a `str` lets the function borrow the text.

---

**Q21.** We build ten temporary strings and return one. Where should the copy appear?

**A21.** At the survivor:

```align
arena {
    chosen := build_choice(...)
    return chosen.clone()
}
```

Do not heap-own all ten in case one survives. Group the phase; copy the one value that crosses its boundary.

---

**Q22.** Which is the useful first question: “stack or heap?” or “how long does it live?”

**A22.** “How long will we use it?” A borrowed view for reading, an arena for values released together, or an owning value for data needed longer. Choose the storage from the lifetime you need.

---

**Q23.** A record owns a name. Can a function inspect the record without taking it?

**A23.** Yes, with a shared `borrow` parameter:

```align
Profile { name: string, visits: i64 }

fn name_size(borrow p: Profile) -> i64 = p.name.len()

fn main() -> i32 {
    p := Profile { name: "Ada".clone(), visits: 0 }
    print(name_size(p))    // 3
    print(p.name.len())    // 3 — the caller still owns p
    return 0
}
```

Without `borrow`, passing this Move record would transfer ownership. The call still says `name_size(p)`; the parameter declaration states how it is passed.

---

**Q24.** May `name_size` update `p.visits`?

**A24.** No. A shared borrow is read-only. To update the caller's record, use a different parameter mode:

```align
fn visit(borrow mut p: Profile) {
    p.visits = p.visits + 1
}
```

Declare the caller's record as `mut p`, then call `visit(p)`. The caller still owns the record, and its `visits` becomes `1`. The callee has exclusive access for the call. If all it needs is the name's text, a `str` parameter remains enough; borrowing the record is useful when the operation needs the record itself.

---

To see the ownership changes as diagrams, read the guide's [memory chapter](../guide/05-memory.md). It follows Move, borrowing, and a string copied out of an arena.

> **The Eighth Commandment**
>
> *One owner at a time. Group values with the same lifetime in an arena. When you need another copy of a string, write `.clone()`.*
