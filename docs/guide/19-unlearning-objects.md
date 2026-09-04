# 19 — Unlearning objects

> 🌐 **English** · [Japanese](./ja/19-unlearning-objects.md)

If you are coming to Align from Java, C#, or Python, you may be used to modeling a program as interacting objects. Align has no classes or inheritance. It separates data from the functions that process it.

This chapter shows how to adapt common object-oriented patterns to Align.

## 1. Updating fields across many entities

**The OOP Way:** 
You have a `Player` class that holds its own `health`, `x`, `y`, and an `update()` method that modifies its own state.

**The Align Way:**
Data and behavior are separate. Furthermore, an individual `Player` is rarely the right unit of abstraction. You do not update *a* player; you update *the* positions.

```align
Player { x: f64, velocity_x: f64, health: i64 }

// Instead of updating Player objects one by one:
arena {
    rows := [
        Player { x: 0.0, velocity_x: 1.0, health: 100 },
        Player { x: 5.0, velocity_x: -1.0, health: 80 },
    ]
    players := rows.to_soa()

    // Compute every next position in one bulk, cache-friendly pass
    xs := players.x
    vxs := players.velocity_x
    next_x := zip(xs, vxs).map(fn v { v.0 + v.1 }).to_array()
}
```

## 2. Processing several variants

**The OOP Way:** 
A list of `Shape` interfaces contains `Circle`, `Rectangle`, and `Triangle` objects. Calling `shape.area()` on each element requires virtual dispatch; if the objects are scattered in memory, traversal can also incur cache misses.

**The Align Way:**
Use a sum type if the collection is small and mixed, or separate arrays if processing speed is paramount.

If you must mix them:
```align
Shape { Circle(f64), Rect(f64, f64) }

shapes := [Shape.Circle(2.0), Shape.Rect(3.0, 4.0)]
areas := shapes.map(fn s {
    match s {
        Circle(r) => 3.14159 * r * r,
        Rect(w, h) => w * h,
    }
}).to_array()
```
When the variants can be processed separately, store Circles in `soa<Circle>` and Rects in `soa<Rect>`. Each pipeline then processes one shape without a branch to select its variant.

## 3. Allocating collections

**The OOP Way:** 
You append to a list inside a loop. The list resizes itself automatically, allocating heap memory unpredictably. 

**The Align Way:**
Use an `arena` when allocations share a lifetime. Its memory is released together when the block ends. For a bulk transformation, let the pipeline accumulate the result and make its allocation explicit at the end:

```align
threshold := 100
arena {
    readings := [42, 150, 88, 203]
    // One visible allocation, at the end of the pipeline — no hidden growth
    spikes := readings.where(fn r { r > threshold }).to_array()
} // Boom. Gone.
```

When results arrive through sequential I/O of unknown length, use `array_builder<T>` and `.push()` to accumulate them (chapter [18](18-std-services.md)). Use a pipeline when the transformation can be expressed as one.

## 4. Accessing and transforming fields

**The OOP Way:** 
You hide fields behind `get_health()` and `set_health()` to encapsulate state and inject behavior.

**The Align Way:**
Data is just data. Structs have public fields. If you need to transform the data, you write a pure function that takes the struct and returns a new value. "Nothing hidden" means you never execute arbitrary code just to read a memory address.

## Summary

In Align, start by identifying the data a task needs and the transformations to apply. Grouping fields into contiguous columns lets each pipeline process the data it uses.
