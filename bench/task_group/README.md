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
packed-tight   one record, fields adjacent; the candidate locality win
packed-padded  one record, result/error start on cache-line boundaries; false-sharing control
```

Each task runs the same generated-trampoline-shaped body and writes exactly one final result. The
probe reports nanoseconds per task, paired median ratios, and the known arena allocation count per
task (`2/1/1` for infallible and `3/1/1` for fallible; the final three columns are split, tight,
padded). It counterbalances layout order on alternating trials and uses medians rather than a
minimum, so drift and position bias do not become a layout claim.

The result is a decision input, not a production optimization. Ship packed records only when
`packed-tight` shows a repeatable at least 10% win on both tiny and large task groups, and when the
`packed-padded` control does not reveal a material cache-line penalty. Otherwise retain the current
ABI and allocations. The production codegen/runtime is intentionally unchanged by this probe.
