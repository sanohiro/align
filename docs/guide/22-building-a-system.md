# 22. Building a system: ECS

> 🌐 **English** · [Japanese](./ja/22-building-a-system.md)

You have unlearned objects (Chapter 19). You know how to manage long-lived memory with pools (Chapter 20). You know how to model state with sum types (Chapter 21).

How do you put this all together to build an entire application? Let's build a miniature Entity-Component-System (ECS) architecture, the quintessential Data-Oriented Design pattern.

## The Architecture

In OOP, a game entity is a class with fields and methods. In ECS:
- **Entities** are just IDs (e.g., `i64`). They contain no data.
- **Components** are plain data, stored as flat, parallel columns — the same field-per-column shape [chapter 11](11-data-oriented.md) taught as `soa<T>`, though here they are bare top-level arrays rather than an actual `soa<T>` value: `soa<T>` is arena-bound (chapter 20), while these components must outlive any single frame's arena.
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

Data in, data out. No hidden state, and — because the closure is pure — nothing stops the compiler from vectorizing the whole pass.

## The Game Loop

Now we wrap it all in a `loop` (Chapter 11 of [The Little Aligner](../little-aligner/11-do-it-until.md)). A real game loop runs for as long as the process is alive, so it cannot spend a fresh allocation every frame the way a one-shot pipeline can — that would grow the arena forever. Instead we allocate two column buffers once, outside the loop, and each frame writes into whichever buffer the previous frame did *not* just read from:

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

(A real game would ask the OS for the elapsed time and the window state — that is `std.time` and, for the window, an FFI binding (Chapter 15); here we run 600 fixed frames.) The world is not an object: it is the arena plus its columns, and every system is just a function you call on them, in an order you can read top to bottom — and because `physics` writes in place, running it for a million frames costs the same two buffers as running it for one.

## Why this scales

1. **Decoupling:** `physics` does not care about sprites. A render system does not care about velocities. You can add a `health` column tomorrow without touching the physics code.
2. **Predictability:** Everything flows from top to bottom. There are no hidden `Update()` methods calling other methods implicitly.
3. **Performance:** Because components are contiguous columns, the CPU prefetcher streams them perfectly. Run `alignc emit-llvm` and you will see `physics` compile to a tightly packed SIMD vector loop.

Data goes in. Data gets transformed. Data comes out. That is Align.
