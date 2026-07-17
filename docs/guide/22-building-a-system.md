# 22. Building a system: ECS

> 🌐 **English** · [Japanese](./ja/22-building-a-system.md)

You have unlearned objects (Chapter 19). You know how to manage long-lived memory with pools (Chapter 20). You know how to model state with enums (Chapter 21).

How do you put this all together to build an entire application? Let's build a miniature Entity-Component-System (ECS) architecture, the quintessential Data-Oriented Design pattern.

## The Architecture

In OOP, a game entity is a class with fields and methods. In ECS:
- **Entities** are just IDs (e.g., `i64`). They contain no data.
- **Components** are plain data. They are stored in SoA (Struct of Arrays) columns.
- **Systems** are functions that iterate over components using pipelines.

Let's model a 2D world where things have Positions, Velocities, and Renderable sprites.

## The Components

Instead of a `GameObject` class, we define flat arrays of components.

```align
Position { x: f32, y: f32 }
Velocity { dx: f32, dy: f32 }

World {
    // Entities are implicit; the index in these arrays is the Entity ID.
    // Option allows us to have sparse components (not every entity has every component).
    positions: array<Option<Position>>,
    velocities: array<Option<Velocity>>,
    sprites: array<Option<string>>,
}
```

## The System

A System is a function that operates on components. It does not belong to any class. Let's write a Physics System that updates positions based on velocities.

In Align, we write this as a pipeline over the component arrays.

```align
fn physics_system(world: mut World, dt: f32) {
    // We only want entities that have BOTH a Position and a Velocity.
    // `zip` combines two arrays.
    world.positions.zip(world.velocities).map_in_place(
        fn (opt_pos, opt_vel) {
            match (opt_pos, opt_vel) {
                (Some(mut p), Some(v)) => {
                    p.x = p.x + v.dx * dt
                    p.y = p.y + v.dy * dt
                    Some(p)
                },
                _ => opt_pos // Leave unchanged
            }
        }
    )
}
```

## The Game Loop

Now we wrap it all in a `loop` (Chapter 11).

```align
fn main() -> i32 {
    mut world := spawn_initial_world()
    
    loop {
        dt := time.delta()
        
        // 1. Process Input (System)
        input_system(world)
        
        // 2. Update Physics (System)
        physics_system(world, dt)
        
        // 3. Render (System)
        render_system(world)
        
        if window.should_close() { break 0 }
    }
}
```

## Why this scales

1. **Decoupling:** `physics_system` does not care about sprites. `render_system` does not care about velocities. You can add a `Health` component tomorrow without touching the physics code.
2. **Predictability:** Everything flows from top to bottom. There are no hidden `Update()` methods calling other methods implicitly.
3. **Performance:** Because components are contiguous arrays, the CPU prefetcher streams them perfectly. When you run `alignc emit-llvm`, you will see that `physics_system` compiles to a tightly packed SIMD vector loop.

Data goes in. Data gets transformed. Data comes out. That is Align.
