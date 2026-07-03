# Data-oriented: structure-of-arrays

This chapter is why Align exists. How you lay out data in memory decides how fast you can process it — often by an order of magnitude — and Align gives you the fast layout as a first-class citizen.

## Array-of-structs vs structure-of-arrays

Suppose you have a million particles, each with a position and a velocity. The obvious layout is an array of structs (AoS):

```text
[ {x0,y0,vx0,vy0}, {x1,y1,vx1,vy1}, ... ]
```

Now you want to advance every position by its velocity — you touch `x`, `y`, `vx`, `vy` but if you only needed `x`, you'd still drag the whole struct through cache. Worse, to update just the `x` field across all particles, the values you want are scattered every few words, and SIMD can't load them contiguously.

The structure-of-arrays (SoA) layout stores each field in its own contiguous array:

```text
x:  [x0, x1, x2, ...]
y:  [y0, y1, y2, ...]
vx: [vx0, vx1, vx2, ...]
vy: [vy0, vy1, vy2, ...]
```

Now "add every velocity to every position" walks two dense arrays in lockstep — perfect cache behavior, and the vectorizer loads 8 or 16 lanes per instruction. This is the layout GPUs and ML engines demand, the layout signal processing wants (planar `RRR GGG BBB`, not interleaved `RGB RGB`).

## Align makes SoA a declaration, not a chore

In most languages, converting AoS to SoA is a manual, error-prone rewrite. In Align, `soa<T>` gives you the SoA layout from a struct definition — you declare the shape once and the compiler lays out the columns. You still think in terms of `T`; the memory is columnar underneath.

## Columnar reductions and group_by

Because the data is already columnar, aggregate operations are natural and fast. `group_by` partitions rows by a key and reduces each group — the database-style operation at the heart of analytics — and it runs over the columns with branchless inner loops.

## The habit

When data is processed in bulk — especially numeric or record data walked repeatedly — reach for SoA. The rule of thumb: if you loop over a collection touching one or two fields at a time, AoS is fighting you and SoA is your friend. Align rewards the choice with cache density and automatic vectorization, for free, from ordinary pipeline code.
