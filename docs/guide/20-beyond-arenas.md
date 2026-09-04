# 20. Beyond arenas: pools and lifetimes

> 🌐 **English** · [Japanese](./ja/20-beyond-arenas.md)

Chapters 5 and 8 introduced `arena` for data that shares a lifetime. If data lives for one phase, such as a web request, game frame, or compilation pass, allocate it in an arena and release it together when that phase ends.

But what happens when data outlives a single phase, and its lifetime is unpredictable?

Imagine a multiplayer game server. Players log in and out at random times. An `arena` per frame drops the player data too early. A server-long arena that allocates a new record on every login retains those allocations after logout. A fixed pool avoids that growth by reusing the same slots.

In a garbage-collected language, you might allocate a `Player` object and let the collector reclaim it after disconnection. Here we use **data pools** and **generational indices** to reuse storage explicitly.

## The Pool

A pool is a set of pre-allocated columns with reusable slots — the same column-per-field shape [chapter 11](11-data-oriented.md) taught as `soa<T>`. Here, the columns are arrays owned by `main` for as long as the server runs. A long-lived arena could also hold a pool, including a `soa<T>`, if the arena outlives every use of it. The distinction is between reusing fixed storage and repeatedly allocating, not between pools and arenas.

We use one row per slot and a `bool` column for occupancy. Handlers can update the caller's columns through `borrow mut` parameters. The fragments below belong inside `main`; a top-level `:=` declares a compile-time constant.

```align
mut alive := [false, false, false, false].to_array()
mut hp    := [0, 0, 0, 0].to_array()
```

When a player joins, we find the first free slot — `alive[i] == false` — (or use a freelist to track empty slots in `O(1)`), write their data into row `i` of every column, and return the `i64` index. When they leave, we set `alive[i]` back to `false`.

After the pool is allocated, joining and leaving update its existing arrays without calling the OS allocator.

## The stale-index problem

Returning an `i64` index alone cannot distinguish successive occupants of a slot.
1. Alice joins and gets assigned `id = 2`.
2. Bob (Alice's friend) saves `target = 2` to heal her later.
3. Alice disconnects. Slot 2 is now free.
4. Charlie joins and is assigned the newly vacant `id = 2`.
5. Bob casts his heal on `target = 2`. Charlie gets healed instead of Alice!

This is a stale-handle form of the [ABA problem](https://en.wikipedia.org/wiki/ABA_problem): a saved index still points to the same slot, but the occupant has changed.

## Generational Indices

To solve this, we don't just hand out an `i64` index. We hand out a ticket that includes both the index and a **generation counter**.

```align
Entity { index: i64, generation: i64 }
```

We upgrade our Pool with one more column that tracks the generation of each slot, and a check that a ticket is still current:

```align
mut generation := [1, 1, 1, 1].to_array()
```

```align
fn is_live(alive: slice<bool>, generation: slice<i64>, e: Entity) -> bool {
    if e.index < 0 { return false }
    if e.index >= alive.len() { return false }
    if e.index >= generation.len() { return false }
    return alive[e.index] && generation[e.index] == e.generation
}
```

Check the index before reading either column. An out-of-range ticket is invalid input to the pool; it should return `false`, not trigger an array-bounds abort. All pool columns must still have the same capacity, and the ticket belongs to this pool only.

Now, the timeline looks like this:
1. Alice joins. Slot 2 is at generation 1. Alice is given `Entity { index: 2, generation: 1 }`.
2. Bob saves `target = Entity { index: 2, generation: 1 }`.
3. Alice disconnects. `alive[2]` becomes `false`, and **we increment `generation[2]` to 2**.
4. Charlie joins. He is placed in slot 2, and is given `Entity { index: 2, generation: 2 }`.
5. Bob tries to heal `Entity { index: 2, generation: 1 }`. `is_live(alive, generation, ticket)` checks slot 2, sees that its current generation is `2`, which does not match Bob's ticket (`1`), and returns `false` — the heal is safely rejected.

## Capacity and generation limits

Keep these rules when implementing insertion and removal:

- If no reusable slot remains, report absence with `Option` or a domain-specific `Result`; do not reuse an occupied slot.
- Check the ticket before removal. A stale ticket must not clear a newer occupant's `alive` flag or advance its generation.
- Never wrap a generation counter. Align integer addition wraps, so incrementing `i64` indefinitely would eventually make an old ticket match again. When a slot reaches the maximum generation, its final removal must retire it permanently instead of incrementing it. Track retirement separately from `alive`, and exclude retired slots from insertion.

For example, in a pool with one live slot, removing generation `1` allows the slot to be issued as generation `2`. The generation-`1` ticket then fails validation. Removing a maximum-generation occupant leaves no reusable slot, so the next insertion reports that the pool is full. The counter is finite; retirement is the policy that preserves the stale-ticket guarantee.

## What the pool provides

The design has three useful properties:
1. **Storage reuse:** Joining and leaving reuse the allocated slots.
2. **Cache locality:** Each field occupies a contiguous column. Bulk updates, such as applying poison damage, can process `hp` directly through a pipeline.
3. **Stale-handle checks:** Access checks both whether the slot is occupied and whether its generation matches the ticket.

A fixed pool suits data with independent lifetimes when you can set a capacity in advance. Use generation-bearing tickets to identify the occupants of reusable slots.
