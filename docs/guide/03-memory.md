# Memory: value, arena, heap

> 🌐 **English** · [日本語](./ja/03-memory.md)

Align has no garbage collector and no manual `free`. Instead, where data lives is a property you choose, and the compiler inserts the cleanup. There are three places.

## Values (the default)

Most data is a plain value — a number, a small struct, a fixed array. Values live on the stack, are copied when small, and need no thought. Primitives and small structs are **Copy**: passing them around duplicates them. You never free a value; it's gone when its scope ends.

```align
Point { x: f64, y: f64 }

fn main() -> i32 {
    p := Point { x: 1.0, y: 2.0 }   // a value, lives here, no cleanup needed
    return 0
}
```

## Arenas (bulk, scoped allocation)

When you need to allocate many things with the same lifetime — parse a document, build a graph, process a request — reach for an `arena`. Everything allocated inside is freed together, in one cheap operation, when the arena block ends. No per-object bookkeeping.

```align
arena {
    // allocations in here draw from the arena
    b := heap.new(42)      // a box, allocated in this arena
    x := b.get()           // read the value back
    // ... build more ...
}   // the whole arena is freed here, all at once
```

The arena is the answer to "I have a phase of work that allocates a lot, then throws it all away." It is fast (a bump pointer), it has no fragmentation, and the free is O(1) regardless of how much you allocated. This is the data-oriented way to manage memory: batch it by lifetime.

## The heap box

`box<T>` is an explicit single heap allocation, created with `heap.new(x)`. In the current design a box lives in an enclosing arena (so its lifetime is the arena's). `.get()` copies the value out; `.clone()` deep-copies the box.

```align
arena {
    b := heap.new(100)
    v := b.get()           // v == 100
}
```

## Move types and escape

Types that own a heap resource — `string`, `array`, `buffer`, `box` — are **Move**, not Copy. Assigning one moves ownership; the old binding can't be used again. This is how Align guarantees no double-free and no use-after-free without a garbage collector or visible lifetimes:

```align
a := some_string()
b := a          // ownership moves to b
// using `a` here is a compile error — it was moved
```

And a value allocated in an arena cannot **escape** that arena — returning it, or storing it somewhere longer-lived, is a compile error. The compiler tracks the region each value belongs to. You never write a lifetime annotation; the compiler infers it and simply rejects the programs that would dangle.

## The habit

Ask "what is the lifetime of this data?" A single value with local lifetime → just a value. A batch with a shared, scoped lifetime → an arena. That decision, made once per phase of work, is the whole of memory management in Align.
