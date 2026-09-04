# Closures and parallelism

> 🌐 **English** · [Japanese](./ja/10-closures-and-parallelism.md)

Align uses closures with value capture and **inferred purity** to control shared state and side effects in parallel code. Use `par_map` to apply one function to many elements, and `task_group` to run separate tasks.

## Lambdas

A lambda is `fn` + parameters + a block; you have been using them in pipelines since chapter [06](06-pipelines.md):

```align
[1, 2, 3].map(fn x { x * 2 }).sum()
[1, 2, 3, 4].reduce(0, fn acc, x { acc + x })
```

Lambdas **capture by value**: an enclosing Copy binding used inside is copied in at creation. The closure retains that value even if the original `mut` binding changes later. It does not share a writable environment:

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

The compiler infers, for every function, whether it is **Pure** (no I/O, no rng, no FFI, and no mutation of external state it does not own — updating a value passed as an explicit `borrow mut` parameter still counts as Pure, since the caller handed it over for exactly that). You never annotate it; you can't get it wrong. You only notice when it protects you:

```align
fn show(x: i64) -> i64 {
    print(x)        // I/O — show is Impure
    return x
}

ys := [1, 2].par_map(fn x { show(x) })
// error: 'par_map' requires a Pure function, but 'main$lambda0' has an
//        observable side effect (I/O or a caller-view write); use `reduce`
//        for an accumulation
```

Sequential `map` may call `show`: it prints in input order. Changing that call to `par_map` is not valid, because the observable order would depend on worker scheduling. Parallel callables must satisfy both purity and capture restrictions. Move captures are rejected, and a `region` allocation capability cannot be sent to a worker even though its type is Copy. These checks are inferred; no `Send` or `Sync` annotation is written.

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

`par_map(f)` runs across a persistent worker-thread pool and returns an owned `array<R>`. For an accepted Pure callable, its values and result order match `map(f).to_array()`. This is the complete sequential equivalent: a bare `map(f)` is an unfinished pipeline. Worker completion order does not change the array order, but scheduling overhead and result allocation remain part of the parallel operation.

And you should: **`par_map` earns its keep only when `f` is expensive.** The range kernel still has setup and scheduling overhead, while sequential `map` fuses into a vectorized loop — for cheap arithmetic, plain `map().sum()` is typically *faster*. Measure before reaching for it. Direct-source `par_map` with Copy-capturing closures, and primitive-scalar length-preserving `map` stages before it, use the same range kernel with one immutable call-scoped context; filtered and unsupported forms remain sequential, and Move captures are rejected.

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

Tasks that can fail return `Result`. Propagate a group failure with `wait()?`:

```align
fn fetch(n: i64) -> Result<i64, Error> {
    if n < 0 { return Err(error(2)) }
    return Ok(n * 10)
}

fn main() -> Result<(), Error> {
    task_group {
        a := spawn(fn { fetch(3) })
        b := spawn(fn { fetch(-1) })
        wait()?                         // joins ALL tasks, then propagates the lowest-index error
        print(a.get() + b.get())        // not reached
    }
    return Ok(())
}
```

`wait()?` is the error boundary of the group: every task completes (no half-joined state), then the failure from the lowest spawn index propagates as an ordinary `Err`. Parallel error handling uses the same one operator as everything else.

## Which one, when

- Same function over many elements → `par_map`, when measurement shows that the work per element outweighs the scheduling overhead and improves on sequential execution.
- A few different jobs at once → `task_group`.
- Everything else → sequential pipelines, which may use SIMD without additional worker threads (chapter [12](12-simd.md)).

All of it visible in source: `par_map` and `spawn` are the only two words in the language that mean "another thread runs this."
