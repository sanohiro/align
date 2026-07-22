# Closures and parallelism

> 🌐 **English** · [Japanese](./ja/10-closures-and-parallelism.md)

Parallelism is where hidden things kill you: hidden shared state, hidden side effects, hidden threads. Align's parallel story is therefore built on two visible pieces — closures with value capture, and **inferred purity** — and two constructs: `par_map` for data parallelism, `task_group` for task parallelism. Nothing else spawns a thread.

## Lambdas

A lambda is `fn` + parameters + a block; you have been using them in pipelines since chapter [06](06-pipelines.md):

```align
[1, 2, 3].map(fn x { x * 2 }).sum()
[1, 2, 3, 4].reduce(0, fn acc, x { acc + x })
```

Lambdas **capture by value**: an enclosing binding used inside is copied in at creation. No shared mutable environment exists, which is precisely what makes the parallel constructs below safe:

```align
factor := 3
print([1, 2, 3].map(fn x { x * factor }).sum())     // 18
```

## Functions as values

A parameter (or binding) of type `fn(T) -> R` accepts a named function, a lambda, or a capturing closure:

```align
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)

fn double(x: i64) -> i64 = x * 2

fn main() -> i32 {
    print(apply(double, 21))            // 42 — named function
    print(apply(fn n: i64 { n + 1 }, 41))   // 42 — lambda
    k: i64 := 100
    print(apply(fn n: i64 { n + k }, 5))    // 105 — capturing closure
    twice := fn x: i64 { x * 2 }        // a lambda as a value (params must be typed)
    print(twice(6))                     // 12
    return 0
}
```

Current limits (implementation in progress): a lambda bound to a value needs typed parameters, and returning a bare function value is still deferred. Function values can be stored in structs and homogeneous arrays/slices; named and non-capturing values are freely reusable, while escape analysis rejects a frame-capturing value that would outlive its environment.

## Purity is inferred — and parallelism requires it

The compiler infers, for every function, whether it is **Pure** (no I/O, no rng, no FFI, no mutation of anything external). You never annotate it; you can't get it wrong. You only notice when it protects you:

```align
fn show(x: i64) -> i64 {
    print(x)        // I/O — show is Impure
    return x
}

ys := [1, 2].par_map(fn x { show(x) })
// error: 'par_map' requires a Pure function, but the lambda has a side
//        effect (it reads/writes I/O)
```

A data race needs shared mutable state or unordered side effects; a Pure function by-value-capturing its inputs has neither. So Align doesn't detect races — it makes them **unrepresentable** in the parallel constructs, at compile time, with no `Send`/`Sync` vocabulary to learn.

## `par_map` — data parallelism

```align
Emp { base: i64, bonus: i64 }

fn net(e: Emp) -> i64 = e.base + e.bonus

fn main() -> Result<(), Error> {
    pay := [
        Emp { base: 30, bonus: 12 },
        Emp { base: 18, bonus: 4 },
    ].par_map(net)          // fan out across a persistent worker pool
    print(pay.sum())        // 64
    return Ok(())
}
```

`par_map(f)` is `map` across a persistent worker-thread pool, materializing an owned `array<R>`. Semantically identical to `map` — purity guarantees it — so you can switch between them freely as data sizes change.

And you should: **`par_map` earns its keep only when `f` is expensive.** Every element crosses an indirect call, while sequential `map` fuses into a vectorized loop — for cheap arithmetic, plain `map().sum()` is typically *faster*. Measure before reaching for it. (A capturing closure currently falls back to sequential execution — implementation in progress.)

## `task_group` — task parallelism

For heterogeneous work — do these three things at once, then combine:

```align
fn main() -> Result<(), Error> {
    base: i64 := 100
    task_group {
        a := spawn(fn { base + 5 })     // runs on a real thread
        b := spawn(fn { base * 2 })
        wait()                          // join everything spawned in this group
        print(a.get() + b.get())        // 305
    }
    return Ok(())
}
```

`spawn(fn { ... })` starts a task and returns a handle; `wait()` joins all of them; `.get()` reads a result after the join. The block is the lifetime: tasks cannot outlive their `task_group`, structurally — no detached threads, no forgotten joins, because the scope won't let you write them.

Tasks that can fail return `Result`, and the join point wears the `?`:

```align
fn fetch(n: i64) -> Result<i64, Error> {
    if n < 0 { return Err(error(2)) }
    return Ok(n * 10)
}

fn main() -> Result<(), Error> {
    task_group {
        a := spawn(fn { fetch(3) })
        b := spawn(fn { fetch(-1) })
        wait()?                         // joins ALL tasks, then propagates the first error
        print(a.get() + b.get())        // not reached
    }
    return Ok(())
}
```

`wait()?` is the error boundary of the group: every task completes (no half-joined state), then the first failure propagates as an ordinary `Err`. Parallel error handling with the same one operator as everything else.

## Which one, when

- Same function over many elements → `par_map`, if the function is expensive enough to beat the vectorized sequential loop.
- A few different jobs at once → `task_group`.
- Everything else → sequential pipelines, which are already using SIMD lanes in parallel (chapter [12](12-simd.md)).

All of it visible in source: `par_map` and `spawn` are the only two words in the language that mean "another thread runs this."
