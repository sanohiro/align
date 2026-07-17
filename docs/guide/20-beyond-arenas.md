# 20. Beyond arenas: pools and lifetimes

> 🌐 **English** · [Japanese](./ja/20-beyond-arenas.md)

Chapter 5 and Chapter 8 taught you that `arena` is the hammer for almost every memory nail. If data lives for a single "phase" (a web request, a frame of a game, a compilation pass), you allocate it in an arena and blow up the whole building when the phase ends.

But what happens when data outlives a single phase, and its lifetime is unpredictable?

Imagine a multiplayer game server. Players log in and out at random times. An `arena` per frame drops the player data too early. A single global `arena` never frees anything and eventually runs out of memory. 

In an Object-Oriented language, you would `new Player()` and let the Garbage Collector eventually clean it up when it disconnects. In Align, we use **Data Pools** and **Generational Indices**.

## The Pool

A Pool is just a pre-allocated block of parallel columns that reuses slots — the same column-per-field shape [chapter 11](11-data-oriented.md) taught as `soa<T>`. It cannot literally *be* a `soa<T>`, though: those columns are arena-resident (`to_soa` must be called inside an `arena`, and the columns die with it), while a Pool's whole point is data that outlives any single arena. So here the columns are bare top-level bindings instead, kept alive for as long as the server runs. Instead of asking the OS for memory every time a player joins, we hold contiguous columns — one row per slot — and manage the vacancy ourselves with a plain `bool` column:

```align
mut alive := [false, false, false, false].to_array()
mut hp    := [0, 0, 0, 0].to_array()
```

When a player joins, we find the first free slot — `alive[i] == false` — (or use a freelist to track empty slots in `O(1)`), write their data into row `i` of every column, and return the `i64` index. When they leave, we set `alive[i]` back to `false`.

No OS calls. No garbage collector pauses. Just changing bytes in an array.

## The stale-index problem

There is a fatal flaw with returning raw `i64` indices. 
1. Alice joins and gets assigned `id = 2`.
2. Bob (Alice's friend) saves `target = 2` to heal her later.
3. Alice disconnects. Slot 2 is now free.
4. Charlie joins and is assigned the newly vacant `id = 2`.
5. Bob casts his heal on `target = 2`. Charlie gets healed instead of Alice!

This is the classic stale-handle flavor of the [ABA problem](https://en.wikipedia.org/wiki/ABA_problem): the slot you point at has been reused behind your back. If we were using pointers in C++, this would be a Use-After-Free security vulnerability.

## Generational Indices

To solve this, we don't just hand out an `i64` index. We hand out a ticket that includes both the index and a **generation counter**.

```align
Entity { index: i64, generation: i64 }
```

We upgrade our Pool with one more column that tracks the generation of each slot, and a check that a ticket is still current:

```align
mut generation := [0, 0, 0, 0].to_array()
```

```align
fn is_live(alive: slice<bool>, generation: slice<i64>, e: Entity) -> bool =
    alive[e.index] && generation[e.index] == e.generation
```

Now, the timeline looks like this:
1. Alice joins. Slot 2 is at generation 1. Alice is given `Entity { index: 2, generation: 1 }`.
2. Bob saves `target = Entity { index: 2, generation: 1 }`.
3. Alice disconnects. `alive[2]` becomes `false`, and **we increment `generation[2]` to 2**.
4. Charlie joins. He is placed in slot 2, and is given `Entity { index: 2, generation: 2 }`.
5. Bob tries to heal `Entity { index: 2, generation: 1 }`. `is_live(alive, generation, ticket)` checks slot 2, sees that its current generation is `2`, which does not match Bob's ticket (`1`), and returns `false` — the heal is safely rejected.

## Why this is the Align way

Notice what we have achieved:
1. **Zero Allocations:** Players can join and leave millions of times without a single call to the OS allocator.
2. **Cache Locality:** All players live contiguously in memory, making bulk updates (like applying poison damage to everyone) incredibly fast via pipelines — `hp` is already a column.
3. **Absolute Safety:** No Use-After-Free bugs, no dangling pointers, and no garbage collection pauses.

When you need unpredictable lifetimes, do not look for a garbage collector. Build a Pool, and hand out tickets.
