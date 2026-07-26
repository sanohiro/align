# Task-group record probe

This is a measure-first probe for the proposed one-allocation task record. The shipped codegen
allocates a task's capture environment, result slot, and (for fallible tasks) error slot separately
from the task-group arena. A packed record would allocate one block and address those fields by
offsets.

Run it with:

```sh
bench/task_group/run.sh
TRIALS=15 REPS=5 bench/task_group/run.sh
```

The harness drives the same `align_rt_tg_register`/`align_rt_tg_wait` ABI for three layouts:

```text
split          shipped shape: env + result (+ error) are separate arena allocations
packed-tight   one record, fields adjacent; the candidate one-allocation shape
packed-padded  one record, result/error start on cache-line boundaries; false-sharing control
```

Each performance task runs the same generated-trampoline-shaped body and writes exactly one final
result. The CPU body is kept live with `black_box`; its result token encodes the task index and
control fields. An unmeasured correctness pass validates every task slot independently for each
layout; measured repetitions retain an aggregate checksum so result writes remain observable
without adding the per-task oracle to the timing. The matrix includes zero-work, CPU-heavy, and
bounded blocking-sleep controls and the one-task caller-only path; it does not claim to model
filesystem or network I/O. Zero-task groups are rejected because they contain no record to measure.
Because the split control uses the same bump arena, its small allocations may already be physically
adjacent; packed-tight primarily measures the one-allocation shape and allocation-call reduction,
not an isolated locality improvement. A separate correctness smoke for each layout makes one task
write the shipped `{i32 tag, i32 code}` Error into `err_slot` and checks the pointer returned by
`tg_wait`.

The probe reports nanoseconds per task, paired median ratios, and the known arena allocation count
per task (`2/1/1` for infallible and `3/1/1` for fallible; the final three columns are split, tight,
padded). It rotates all six layout permutations and validates each task result independently in
the correctness pass, so even repetition counts cannot hide swapped or compensating results.
Fallible records use the shipped Error size and alignment. Release LTO is disabled so the normal
runtime C-ABI boundary remains visible; this is not an `rt-lto` measurement.

The recorded native Apple Silicon run used:

```sh
LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib:/opt/homebrew/opt/llvm/lib \
TRIALS=31 REPS=7 bench/task_group/run.sh
```

The harness prints the host parallelism/runtime worker count with the result. The recorded host
reported eight workers; release LTO was disabled as above.

The result is a decision input, not a production optimization. Ship packed records only when
`packed-tight` shows a repeatable at least 10% win on both tiny and large task groups, and when the
`packed-padded` control does not reveal a material cache-line penalty. Otherwise retain the current
ABI and allocations. The production codegen/runtime is intentionally unchanged by this probe.
