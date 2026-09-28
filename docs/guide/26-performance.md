# Writing fast Align

> 🌐 **English** · [Japanese](./ja/26-performance.md)

This chapter is for a program that already produces the right result and now needs less time or memory. Start with the workload, count its passes and allocations, and measure one change at a time. Pipelines, explicit ownership, and contiguous storage give the compiler useful facts; they do not guarantee that every operation vectorizes or that more threads make it faster.

## Establish a useful baseline

Build once, then time the resulting executable on representative input:

```text
alignc build app.align --profile release --target-cpu baseline
./app
```

The second command runs the program; use your timing or profiling tool around that executable. Timing `alignc run` includes compiler, cache, and linker work. Record the compiler version, profile, CPU target, input size, and machine alongside the result. Separate startup, parsing, computation, and output when the question concerns only one of them; also retain an end-to-end measurement.

Use input loaded or generated at runtime. The small literal arrays in this guide explain behavior, but a compiler can calculate their results before the program runs. Consume the result, for example by printing a checksum after the timed computation. Compare equivalent results, repeat measurements, alternate the old and new versions, and report the spread as well as the chosen statistic. Include small, large, empty, and relevant skewed inputs; a change that wins on one size may lose on another.

`release` uses O2 and is the default; `fast` uses O3. Try `fast` on the same workload rather than assuming its name promises a win. `--target-cpu native` targets the current machine; use it when that matches deployment. Keep `baseline` for the portable target, or select an explicit LLVM CPU supported by your deployment machines. Compare versions with the same target settings.

## Count passes before counting instructions

Save this complete example as `energy.align`:

```align
fn positive_energy(xs: slice<i64>) -> i64 {
    return xs.where(fn x { x > 0 }).map(fn x { x * x }).sum()
}

fn main() -> i32 {
    xs := [1, -2, 3, 4]
    print(positive_energy(xs))    // 26
    return 0
}
```

The filter, multiplication, and sum form one pass with no intermediate array. Writing `.to_array()` before `.sum()` would request storage for the selected squares and a second pass to sum them. Materialization is useful when later work needs the collection or reuses an expensive transformation; it is a cost to choose deliberately.

| Shape | Work and storage to account for |
| --- | --- |
| `xs.map(f).where(p).sum()` | One fused traversal; no intermediate collection. |
| `xs.map(f).to_array()` | One traversal and storage for the result. |
| `xs.map(f).map_into(dst)` | Writes existing storage; no result allocation. |
| `xs.sort()` | Materializes an owned sorted result and may use working storage. |
| `xs.par_map(f)` | Materializes an owned result and includes parallel setup and scheduling. |

These are the costs of the collection operations. Work inside `f` or `p`, including an explicit allocation, still counts. Integer arithmetic wraps on overflow; a faster implementation must preserve that result too. See [pipelines](06-pipelines.md) for the full operation contracts.

## Reuse storage when the output shape is stable

An `out` slice makes the destination part of the function contract:

```align
fn double_into(xs: slice<i64>, out dst: slice<i64>) {
    xs.map(fn x { x * 2 }).map_into(dst)
}

fn main() -> i32 {
    xs := [1, 2, 3, 4]
    mut ys := [0, 0, 0, 0]
    mut dst: slice<i64> := ys
    double_into(xs, dst)
    print(ys.sum())    // 20
    return 0
}
```

The destination must have the required length and must not overlap the source. `map_into` accepts length-preserving map/projection chains, not filtered pipelines. It is not an in-place update of `xs`. For repeated state updates, two buffers can alternate input and output roles; [the ECS example](22-building-a-system.md) shows this for an entire frame loop.

An arena releases its allocations together when its scope ends. An arena enclosing a long loop therefore retains each iteration's allocations until the loop ends. Use reusable destinations, or a per-batch arena when no result needs to outlive the batch. A Move result does not by itself make arena-backed storage independent of that arena.

Choose the construction surface that matches the output:

| Need | Surface and boundary |
| --- | --- |
| Unknown number of typed elements | `array_builder<T>`; reserve with `array_builder(capacity)` when a useful bound is known. Capacity is not initialized length. |
| Incremental text | `builder(capacity)` and writes, then `to_string()`; avoid cloning each fragment. |
| Initialized repeated bytes | `buffer.filled(length, value)`; `buffer(capacity)` starts with an empty readable window. |
| More repeated bytes at the end | `append_filled(length, value)` on a mutable buffer. |
| Update an existing byte window | `slice<u8>.fill`, `copy_from`, or endian-explicit `set_*` / `fill_*`; these do not grow it. |
| Keep a shorter owned array | `array.truncate(len)` drops the removed suffix in place; it does not compact the backing capacity. |
| A small, fixed-size numeric table in a record | `[T; N]` stores elements inline; it is not a SIMD register. Copying the record also copies those elements. |

Reservation can reduce growth and copying; it does not eliminate the writes or every allocator call. Oversized reservations and large inline records can increase memory traffic. See [memory](05-memory.md) and [strings and text](07-strings-and-text.md) for ownership and bounds rules.

## Put the frequently scanned fields together

Use `soa<T>` or separate columns when repeated passes read a few fields of many rows. A column scan can then read adjacent values without fetching every other field. Keep rows together when the common operation needs the whole row. Account for the conversion: doing `to_soa()` before every small operation can cost more than the scan saves. For suitable JSON workloads, decoding directly into SoA avoids a separate transpose.

Use `zip` for same-index work across columns. Keep row order consistent and filter the paired values together. For repeated grouped reports, consider `dict_encode` to reuse string-key ids; use `group_by(...).agg(...)` when the supported source/key shape lets several aggregates share a pass. The current shape restrictions and examples are in [data-oriented design](11-data-oriented.md).

Views avoid payload copies but retain a lifetime dependency. Borrow `str` or a slice while its owner stays alive; clone when an independent owner is required. A byte slice's `.view_le()` can expose naturally aligned little-endian numeric data without copying, and returns `None` when length or alignment is unsuitable. Its expected result type supplies the numeric element type; the call has no written type argument. It preserves the source's access authority and lifetime. Packed, unaligned, or opposite-endian data still needs the scalar byte accessors. This view is not a promise that its consumers use SIMD.

## Keep numerical semantics explicit

Start numeric kernels with ordinary slice pipelines. Check the generated code before replacing them with `vecN` intrinsics. Explicit vectors still require valid full-width loads, a tail strategy, and a width appropriate to the target. A mask cannot make an out-of-bounds load safe, and `select` evaluates both value arguments. See [explicit SIMD](12-simd.md).

Floating-point evaluation is ordered and uncontracted by default. `--profile fast` does not grant fast-math semantics. If the algorithm permits different rounding, `float(reassoc) { xs.sum() }` explicitly permits reassociation; `float(contract) { a * b + c }` permits contraction of that multiply/add pair. The permissions are independent and can be combined. Each named function or lambda starts strict, so a relaxed caller does not relax arithmetic inside its callback.

Reassociation and contraction can change rounding, signed-zero results, and NaN payloads. Set an error tolerance justified by the algorithm and test exceptional inputs as well as ordinary ones. A permission to transform the calculation is not a promise of a vector instruction or a faster result. The exact rules are in [the specification](../../draft.md#float-semantics).

## Add parallelism after measuring sequential work

`par_map(f)` returns the same ordered values as `map(f).to_array()` for an accepted Pure callable. Compare those two when the output is an array. If the output is only a scalar reduction, also compare with `map(f).sum()`: the sequential fused version avoids the intermediate array altogether.

Worker setup, scheduling, materialization, memory bandwidth, and work per element all affect the result. Small or cheap work can be faster sequentially. `explain-opt` shows whether the compiler selected a range plan or a sequential fallback; the runtime may still execute a range plan on the caller alone. It does not tell you which runtime path a particular input took. Parallel callbacks still have purity and capture restrictions; Move captures and region allocation capabilities cannot be sent to workers. Use `task_group` for separate tasks and keep their joins within the scope. [Closures and parallelism](10-closures-and-parallelism.md) covers both forms.

## Inspect the code that answers your question

For the first example, retain the function as an inspection root so literal inputs in `main` do not hide the kernel:

```text
alignc explain-opt energy.align --profile release --target-cpu baseline --export positive_energy --verbose
alignc emit-llvm energy.align --profile release --target-cpu baseline --export positive_energy --stage optimized --no-rt-lto
```

Look for fused traversals, remaining runtime calls, bounds checks, vector operations, and materialized storage. A bounds check may be necessary; do not remove validation to improve a timing. Current loop proofs recognize specific shapes, so an equivalent handwritten loop need not receive the same optimization. A vector type in IR is useful evidence, but it does not establish the cost of the final machine code.

`explain-opt` reports per-unit choices and LLVM remarks with runtime LTO off. It does not report the final linked ThinLTO executable. A file with no `main` uses its own `pub` functions as inspection roots unless `--export` selects a narrower set. For final instruction selection, inspect the built binary with your platform's disassembler and profile that same artifact.

Runtime LTO is already on by default for `release` and `fast`. Cross-unit `--thin-lto` is an explicit option for linked builds in those profiles; measure its runtime benefit and build cost. Instrumented PGO is a further option with a representative training workload, but currently cannot be combined with `--thin-lto`. Commands and restrictions are in [the toolchain chapter](16-toolchain.md#profiles-targets-and-whole-program-optimization).

## Choose the next experiment from the evidence

| Observation | Next experiment |
| --- | --- |
| Allocation or copying dominates | Reuse a destination, reserve a measured capacity, shorten an arena lifetime, or keep a borrowed view. |
| The same rows are scanned repeatedly | Fuse compatible stages, combine aggregates, or reuse a deliberately materialized result. |
| A hot scan touches few fields | Compare columns/SoA, including the cost of creating them. |
| Numeric work stays scalar | Inspect legality and remaining calls; consider explicit float permissions only if the algorithm permits them. |
| Parallel execution regresses | Compare the equivalent sequential pipeline and several input sizes; include output allocation. |
| A kernel improves but the app does not | Profile parsing, I/O, startup, and output separately; retain the end-to-end result. |

The repository's [benchmark methodology](../../bench/README.md#methodology-dont-skip) gives concrete examples of constant folding, measurement drift, and misleading timing differences. For practice before measuring, [Little Aligner's final chapter](../little-aligner/15-read-it-four-ways.md) asks you to predict the answer, trace the data and lifetimes, and count the work.
