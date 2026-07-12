# Memory: value, arena, heap

> 🌐 **English** · [Japanese](./ja/05-memory.md)

Align has no garbage collector, no manual `free`, and no lifetime annotations. Instead: **where data lives is a decision you make**, ownership is **a property of the type**, and the compiler infers every lifetime and rejects the programs that would dangle. This chapter is the whole model — it is small.

## Values (the default)

Most data is a plain value: a number, a `bool`, a small struct, a tuple of scalars. Values live on the stack and are **Copy** — assigning or passing one duplicates it, both copies are independent, nothing is ever freed because nothing was allocated.

```align
Point { x: f64, y: f64 }

fn main() -> i32 {
    p := Point { x: 1.0, y: 2.0 }
    q := p              // a copy; p and q are independent
    return 0
}                       // scope ends, values are simply gone
```

When a struct gets big, passing it by value starts to cost — the compiler warns (`huge struct copy`) past two cache lines. That is your cue to reach for a slice, an arena, or SoA (chapter [11](11-data-oriented.md)) instead of copying.

## Move types — ownership in the type

Types that own heap resources — `string`, `array<T>`, `buffer`, `box`, I/O handles — are **Move**, not Copy. Assigning one transfers ownership; the source binding is dead afterwards, and the compiler enforces it:

```align
fn main() -> i32 {
    a := "hi".clone()   // a `string` — an owned heap buffer
    b := a              // ownership moves to b
    print(a.len())      // error: use of moved value 'a'
    return 0
}
```

That compile error is the entire mechanism that replaces both the garbage collector and Rust's visible lifetimes: exactly one owner at any time, so the compiler knows exactly when to free — no double-free, no use-after-free, no annotation. When the owner goes out of scope, the buffer is dropped; when you reassign a `mut` owner, the old value is dropped first (no leak). When you genuinely want two, say so: `.clone()` is a visible deep copy.

A struct with an owning field (`name: string`) becomes a Move type itself, dropped recursively — ownership composes through structure. Reading such a field (`u.name.len()`) borrows it as a `str` view without consuming anything.

## Arenas — batch allocation by lifetime

When a *phase* of work allocates many things that all die together — parse a file, build a request, decode a batch — wrap the phase in `arena {}`:

```align
fn join(a: str, b: str) -> string {
    arena {
        c := template "{a}{b}" // arena-backed temporary
        return c.clone()    // copy the result out — visible escape
    }                       // everything else freed here, O(1)
}

fn main() -> i32 {
    s := join("fu", "sion")
    print(s)                // fusion
    return 0
}
```

An arena is a bump allocator: allocation is a pointer increment, and the free is one operation for the whole block regardless of how much you allocated. No per-object bookkeeping, no fragmentation. This is data-oriented memory management: group allocations by lifetime, not by object.

The compiler tracks the **region** of every value. A value allocated in an arena cannot leave it:

```align
fn leak(a: str, b: str) -> str {
    arena {
        s := a + b
        return s        // error: cannot return a value allocated in an arena
    }                   //        (it is freed at block end)
}
```

You never annotate a region. You only see them when you try to write a dangling reference — and the program doesn't compile. The escape hatch is always the same, and always visible: `.clone()` copies the data out into an owned value.

## The heap, explicitly

`heap.new(x)` makes a single explicit allocation — a `box` — inside the enclosing arena; `.get()` reads the value back out:

```align
fn main() -> i32 {
    arena {
        b := heap.new(42)
        print(b.get())      // 42
    }
    return 0
}
```

You will rarely write this — and the compiler will tell you when you didn't need it. The example above actually earns a lint: *"unnecessary heap allocation: this box never escapes — use the value directly."* `heap.new` exists for the case where a value must outlive the stack frame that computed it but live inside a chosen arena; it is deliberately not the everyday tool. In Align, "allocate every object on the heap" is not a style — the language steers you to values and arenas, and it says so when you stray.

## Views: `str` and `slice<T>`

A `str` is a borrowed view of string data; a `slice<T>` is a borrowed view of an array. Views are cheap Copy values (a pointer and a length), and they carry the **region of the data they point into** — a view of arena data can't escape the arena, a view of a struct's field can't outlive the struct. Same inference, same rule, no annotation.

```align
fn main() -> i32 {
    xs := [10, 20, 30, 40]
    s := xs[1..3]           // slice view: elements 1 and 2, no copy
    print(s.sum())          // 50
    return 0
}
```

## The decision procedure

That's all of it. When you create data, ask one question — *what is its lifetime?*

- Dies in this expression or scope → **a value**. Do nothing.
- A batch that dies together at the end of a phase → **arena**, and `.clone()` the survivors out.
- One value that must outlive the frame → **`heap.new`** in the arena that matches its lifetime.
- Just looking at someone else's data → **a view** (`str`, `slice`), free of charge.

Everything else — when to free, whether it escapes, who owns what — is the compiler's job, checked at compile time, invisible in the source except at the two visible points: `arena {}` where a lifetime begins and ends, and `.clone()` where you pay for a copy.
