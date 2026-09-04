# 22. Building a system: ECS

> 🌐 **English** · [Japanese](./ja/22-building-a-system.md)

The previous chapters covered data and behavior (19), pools for long-lived data (20), and state machines (21).

We can combine these ideas in a small Entity-Component-System (ECS), a common data-oriented architecture.

## The Architecture

In OOP, a game entity is a class with fields and methods. In ECS:
- **Entities** are just IDs (e.g., `i64`). They contain no data.
- **Components** are plain data in parallel columns. Here the columns are arrays allocated once in `main`'s arena, which encloses the whole game loop. A `soa<T>` could also live in that arena; neither form belongs in an arena that ends after each frame. Systems below receive input slices and an `out` destination. The declaration fragments belong inside `main`, because top-level `:=` declares a compile-time constant.
- **Systems** are functions that iterate over components using pipelines.

Let's model a tiny 1D world where things have positions and velocities.

## The Components

Instead of a `GameObject` class, we define columns. The entity is nothing but the row index shared by all of them:

```align
// Row i across all columns = entity i.
mut xs  := [0.0, 10.0, 20.0].to_array()   // position component
vxs     := [1.0, 1.0, -1.0].to_array()    // velocity component
```

A real world would carry more columns — health, sprite ids — and, for components not every entity has, an `alive`-style `bool` column plus the generational tickets of Chapter 20. The shape stays the same: one column per field, one row per entity.

## The System

A System is a function that operates on components. It does not belong to any class. Let's write a Physics System that computes the next positions from the velocities.

In Align, a system is a pipeline over the component columns, writing into a caller-owned destination with `map_into` ([Little Aligner 05](../little-aligner/05-chains.md)) instead of allocating a fresh array every call:

```align
fn physics(xs: slice<f64>, vxs: slice<f64>, dt: f64, out next_xs: slice<f64>) {
    zip(xs, vxs).map(fn v { v.0 + v.1 * dt }).map_into(next_xs)
}
```

The inputs and destination are explicit. The pure closure and contiguous columns let the compiler analyze the whole pass for vectorization.

All three slices must have equal lengths, and `next_xs` must not overlap either input. Calling `physics(xs, vxs, dt, xs)` would violate the `out` contract. The two buffers below provide separate input and output storage while preserving each entity's index across columns.

## The Game Loop

Now put the update in a `loop` (Chapter 11 of [The Little Aligner](../little-aligner/11-do-it-until.md)). Repeatedly allocating in an arena that encloses the loop would retain every allocation until the loop ends. Instead, allocate two position buffers once and alternate their roles: the first frame reads A and writes B; the next reads B and writes A.

```align
fn main() -> i32 {
    arena {
        mut buf_a := [0.0, 10.0, 20.0].to_array()
        mut buf_b := [0.0, 0.0, 0.0].to_array()
        vxs := [1.0, 1.0, -1.0].to_array()

        mut frame := 0
        loop {
            if frame % 2 == 0 {
                physics(buf_a[..], vxs[..], 0.016, buf_b[..])
            } else {
                physics(buf_b[..], vxs[..], 0.016, buf_a[..])
            }
            // ...input system, render system: more functions over the same columns...
            frame = frame + 1
            if frame == 600 { break }
        }
        print(buf_a.len())
    }
    return 0
}
```

A real game would read elapsed time with `std.time` and window state through an FFI binding (Chapter 15); this example runs 600 fixed frames. The arena holds the columns for the whole run, and each system is a function called in the order shown. `physics` writes into the existing destination, so the position storage stays at two buffers regardless of the number of frames.

## Why this scales

1. **Decoupling:** `physics` does not care about sprites. A render system does not care about velocities. You can add a `health` column tomorrow without touching the physics code.
2. **Predictability:** Everything flows from top to bottom. There are no hidden `Update()` methods calling other methods implicitly.
3. **Performance:** Contiguous columns support sequential memory access and SIMD. Use `alignc explain-opt` or `alignc emit-llvm` to check whether `physics` vectorizes for your target.

The arrays hold the state; each system applies a transformation. Adding a system means adding another function and choosing where to call it in the loop.
