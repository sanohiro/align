# 20. Beyond arenas: pools and lifetimes

> 🌐 **English** · [Japanese](./ja/20-beyond-arenas.md)

Chapter 5 and Chapter 8 taught you that `arena` is the hammer for almost every memory nail. If data lives for a single "phase" (a web request, a frame of a game, a compilation pass), you allocate it in an arena and blow up the whole building when the phase ends.

But what happens when data outlives a single phase, and its lifetime is unpredictable?

Imagine a multiplayer game server. Players log in and out at random times. An `arena` per frame drops the player data too early. A single global `arena` never frees anything and eventually runs out of memory. 

In an Object-Oriented language, you would `new Player()` and let the Garbage Collector eventually clean it up when it disconnects. In Align, we use **Data Pools** and **Generational Indices**.

## The Pool

A Pool is just a pre-allocated array (or SoA) that reuses slots. Instead of asking the OS for memory every time a player joins, we hold a contiguous block of memory and manage the vacancy ourselves.

```align
Player { name: string, hp: i64 }

Pool {
    slots: array<Option<Player>>,
    next_free: i64,
}
```

When a player joins, we find the first `None` slot (or use a freelist to track empty slots in `O(1)`), put the `Player` there, and return the `i64` index. When they leave, we set the slot back to `None`.

No OS calls. No garbage collector pauses. Just changing bytes in an array.

## The ABA Problem

There is a fatal flaw with returning raw `i64` indices. 
1. Alice joins and gets assigned `id = 4`.
2. Bob (Alice's friend) saves `target = 4` to heal her later.
3. Alice disconnects. Slot 4 is now `None`.
4. Charlie joins and is assigned the newly vacant `id = 4`.
5. Bob casts his heal on `target = 4`. Charlie gets healed instead of Alice!

This is the classic [ABA problem](https://en.wikipedia.org/wiki/ABA_problem) of resource reuse. If we were using pointers in C++, this would be a Use-After-Free security vulnerability.

## Generational Indices

To solve this, we don't just hand out an `i64` index. We hand out a ticket that includes both the index and a **generation counter**.

```align
Entity {
    index: i32,
    generation: i32,
}
```

We upgrade our Pool to track the generation of each slot:

```align
Slot {
    value: Option<Player>,
    generation: i32,
}

Pool {
    slots: array<Slot>,
}
```

Now, the timeline looks like this:
1. Alice joins. Slot 4 is at generation 1. Alice is given `Entity { index: 4, generation: 1 }`.
2. Bob saves `target = Entity { index: 4, generation: 1 }`.
3. Alice disconnects. Slot 4's value becomes `None`, and **we increment Slot 4's generation to 2**.
4. Charlie joins. He is placed in Slot 4, and is given `Entity { index: 4, generation: 2 }`.
5. Bob tries to heal `Entity { index: 4, generation: 1 }`. The Pool looks at Slot 4, sees that its current generation is `2`, which does not match Bob's ticket (`1`). The Pool safely rejects the heal.

## Why this is the Align way

Notice what we have achieved:
1. **Zero Allocations:** Players can join and leave millions of times without a single call to the OS allocator.
2. **Cache Locality:** All players live contiguously in memory, making bulk updates (like applying poison damage to everyone) incredibly fast via pipelines.
3. **Absolute Safety:** No Use-After-Free bugs, no dangling pointers, and no garbage collection pauses.

When you need unpredictable lifetimes, do not look for a garbage collector. Build a Pool, and hand out tickets.
