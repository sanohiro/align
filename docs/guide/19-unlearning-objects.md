# 19 — Unlearning objects

> 🌐 **English** · [Japanese](./ja/19-unlearning-objects.md)

If you are coming to Align from Java, C#, or Python, your first instinct is to model the world as a graph of interacting objects. In Align, that instinct will fight the language at every turn. Align does not have objects, classes, or inheritance. 

This chapter is a cookbook of paradigm shifts: how to solve common OOP problems the Align way.

## 1. The "Stateful Entity" Anti-Pattern

**The OOP Way:** 
You have a `Player` class that holds its own `health`, `x`, `y`, and an `update()` method that modifies its own state.

**The Align Way:**
Data and behavior are separate. Furthermore, an individual `Player` is rarely the right unit of abstraction. You do not update *a* player; you update *the* positions.

```align
// Instead of Player { health, x, y }
arena {
    players: soa<Player> = load_players()
    
    // Update all positions in one bulk, cache-friendly pass
    players.x.zip(players.velocity_x).map(fn (x, v) { x + v }).to_array()
}
```

## 2. The "Polymorphic List" Anti-Pattern

**The OOP Way:** 
A list of `Shape` interfaces, containing `Circle`, `Rectangle`, and `Triangle` objects. You loop over them and call `shape.draw()`. This causes virtual method dispatch (cache misses) on every iteration.

**The Align Way:**
Use sum types (`enum`) if the collection is small and mixed, or separate arrays if processing speed is paramount.

If you must mix them:
```align
enum Shape {
    Circle { radius: f32 },
    Rect { w: f32, h: f32 },
}

shapes.map(fn s {
    match s {
        Circle { radius } => draw_circle(radius),
        Rect { w, h } => draw_rect(w, h),
    }
})
```
However, the true data-oriented approach is to store all Circles in one `soa<Circle>` and all Rects in a `soa<Rect>`, and process them in two separate, blazing-fast pipelines with no branching at all.

## 3. The "Hidden Allocator" Anti-Pattern

**The OOP Way:** 
You append to a list inside a loop. The list resizes itself automatically, allocating heap memory unpredictably. 

**The Align Way:**
Align has `heap.alloc`, but its idiomatic use is rare. If you need dynamic memory, you use an `arena`. When the arena block ends, all memory is freed instantly. You never `new` or `delete` individual objects inside a hot loop. 

```align
arena {
    // Accumulate results into the arena without individual frees
    mut results := []
    lines.where(.is_error).map(fn l { results.push(l.msg) })
} // Boom. Gone.
```

## 4. The "Getter/Setter" Anti-Pattern

**The OOP Way:** 
You hide fields behind `get_health()` and `set_health()` to encapsulate state and inject behavior.

**The Align Way:**
Data is just data. Structs have public fields. If you need to transform the data, you write a pure function that takes the struct and returns a new value. "Nothing hidden" means you never execute arbitrary code just to read a memory address.

## Summary

Unlearning objects means stopping the search for "the thing that *does* the action" and starting the search for "how the *data* flows". When you lay the data flat and push it through pipelines, the machine will run it faster than you ever thought possible.
