# Closures and parallelism

> 🌐 **English** · [Japanese](./ja/10-closures-and-parallelism.md)

Align uses closures with value capture and **inferred purity** to control shared state and side effects in parallel code. Use `par_map` to apply one function to many elements, and `task_group` to run separate tasks.

## Lambdas

A lambda consists of `fn`, parameters and a block. The pipelines in chapter [06](06-pipelines.md) use this form:

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

## Purity is inferred — and `par_map` requires it

The compiler infers whether each function is **Pure**: no I/O, RNG, FFI or mutation of external state it does not own. Updating an explicit `borrow mut` parameter still counts as Pure. No purity annotation is written. This example fails because the callable passed to `par_map` performs I/O:

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

`par_map(f)` produces an owned `array<R>`. For an accepted Pure callable, its values and result order match `map(f).to_array()`. This is the complete sequential equivalent: a bare `map(f)` is an unfinished pipeline. Supported range kernels choose between caller-only execution and a persistent worker pool at runtime. Worker completion order does not change the array order; materialization and scheduling still cost work. A supported direct integer `par_map(f).sum()` can instead select `range-reduce`, avoiding a full intermediate result array. Check the form with `explain-opt`; a range-kernel report does not prove workers actually ran.

Measure `par_map` against the complete sequential equivalent. Input size, work per element, allocation, memory bandwidth and available workers all affect the result. Setup and scheduling can dominate short or cheap work; a fused sequential pipeline may vectorize and be faster. Direct-source `par_map` with Copy-capturing closures, supported scalar maps and filters, and supported field/projection forms use range kernels. Filtered forms may evaluate a Pure predicate again during count-and-fill processing. Unsupported forms fall back to sequential execution; Move captures remain rejected. Use `alignc explain-opt` to inspect the selected path and chapter [26](26-performance.md) for the measurement procedure.

## `task_group` — task parallelism

For heterogeneous work — do these three things at once, then combine:

```align
fn main() -> Result<(), Error> {
    base: i64 := 100
    task_group {
        a := spawn(fn { base + 5 })     // becomes eligible for worker execution
        b := spawn(fn { base * 2 })
        wait()                          // join everything spawned in this group
        print(a.get() + b.get())        // 305
    }
    return Ok(())
}
```

`spawn(fn { ... })` starts a task and returns a handle; `wait()` joins the group; `.get()` reads a result after a successful join. Tasks cannot outlive their `task_group`; leaving the scope, including an early exit, joins remaining tasks.

The task is eligible to run before `spawn` returns, even when it is the only task. A busy worker
pool can delay its start. A captured view shares its original bytes: keep that backing unchanged
and alive until `wait`. Dropping or reassigning the task handle does not end this protection.
Tasks may read shared views and modify storage they create themselves.

Unlike `par_map`, spawned tasks may perform I/O and other impure work. `wait()` also helps execute its group's pending tasks, allowing nested groups to progress even when every worker is occupied. It cannot resolve a blocking cycle created by the program. Task results may be self-contained primitive values, strings, buffers, ordinary dynamic arrays, records, tuples or sums, optionally through `Result<R, Error>`. They may contain owned data but cannot contain views, pointers, callables or native/resource handles. After a successful wait, Copy `get()` may repeat; owned `get()` consumes the handle and transfers the result once. Results can outlive the group. Group exit joins all tasks and drops untaken results, including after a failed wait. Shared captures remain read-only. For contiguous output, create one owned band per task and collect the named handles in band order.

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

`wait()?` joins every task, then propagates the failure with the lowest spawn index as an ordinary `Err`. It uses the same propagation operator as sequential code.

### Keeping an owned result after the group

Create the output inside the task and take it after `wait`. This function returns
the task's string without cloning it again:

```align
fn make_label(name: str) -> string {
    task_group {
        task := spawn(fn {
            text := builder()
            text.write("hello, ")
            text.write(name)
            text.to_string()
        })
        wait()
        task.get()
    }
}

fn main() -> i32 {
    label := make_label("Ada")
    print(label)    // hello, Ada
    return 0
}
```

The captured `name` is borrowed until the join. The builder creates independent
text, and `task.get()` transfers that text to the caller. A second `task.get()`
would reuse a moved handle and is rejected. Returning `name` itself would leave a
borrowed view in the task result, which is outside the supported result types.

## Which one, when

- Same function over many elements → `par_map`, when measurement shows that the work per element outweighs the scheduling overhead and improves on sequential execution.
- A few different jobs at once → `task_group`.
- Everything else → sequential pipelines, which may use SIMD without additional worker threads (chapter [12](12-simd.md)).

`par_map` and `spawn` mark explicit parallel work. Actual worker use depends on the operation and runtime scheduling.
