# The Little Aligner

> 🌐 **English** · [Japanese](./ja/README.md)

*In the tradition of "The Little Schemer."*

This is a **drill book**: a conversation of small questions and small answers, each building on the last. You will practice pipelines, `match`, Move, arenas, and arranging data in columns. For a systematic introduction and a broader account of the language, see [the guide](../guide/README.md).

It does not tour every syntax form or library module. The aim is to help you approach a new problem: What is the shape of the data? Does it call for a pipeline, a choice, or a loop? Who owns each value, how long does it live, and what work will the machine do?

## How to use it

Read a question and **answer it before reading the answer.** Saying it out loud may help. If you were right, move on. If you were wrong, go back a few questions and try again. You may run a program (`alignc run`), but predict its result first. Most questions can be answered from what you have learned so far.

Some answers are one word. Some questions look identical to the one before — the difference is the lesson. Each chapter ends with a **Commandment** to help you remember its main idea.

After reading the book, choose a program you liked and read it again without running it. Predict its answer. Trace the data and the lifetimes of its values. Count the passes, allocations, and copies. [The final chapter](15-read-it-four-ways.md) practices this way of reading.

## Trying the examples

For installation and your first program, see [Getting started](../guide/01-getting-started.md). Most questions show expressions or short fragments. Use the declarations given with the question; when saving a complete program, put type and function declarations at file scope and the statements to run inside `main`. Questions that ask whether code compiles sometimes contain deliberate errors.

Treat each question as a separate experiment unless it explicitly continues the previous one. In `align-repl`, `:clear` starts a fresh experiment, `:list` shows the current program, and `:save PATH` saves it. The REPL runs the whole accumulated program on each entry, so earlier file writes or other side effects happen again. See [The toolchain](../guide/16-toolchain.md) for the details.

When an answer says an array is `[2, 4, 6]`, it describes the elements. The REPL displays an array's type, not its contents. To check your prediction, inspect elements such as `xs[0]`, or ask for a scalar result such as `xs.sum()`.

## The chapters

1. [Toys](01-toys.md) — values, bindings, and functions
2. [Do It Again](02-do-it-again.md) — `map`
3. [Keep Some](03-keep-some.md) — `where` and field projections
4. [Collapse It](04-collapse-it.md) — reductions: `sum`, `count`, `reduce`, and friends
5. [Chains](05-chains.md) — composing pipelines and fusing loops
6. [One of Many](06-one-of-many.md) — sum types and `match`
7. [Maybe, or It Failed](07-maybe-or-it-failed.md) — `Option`, `Result`, `?`
8. [Whose Is It?](08-whose-is-it.md) — Copy, Move, arenas, and `.clone()`
9. [Turn It Sideways](09-turn-it-sideways.md) — `soa`: data as columns
10. [Count Me by Name](10-count-me-by-name.md) — `group_by`, `agg`, `dict_encode`
11. [Do It Until](11-do-it-until.md) — the `loop` expression, when a pipeline can't say it
12. [Do It Apart](12-do-it-apart.md) — Pure work, `par_map`, and structured tasks
13. [Four at a Time](13-four-at-a-time.md) — explicit SIMD, vectors, and masks
14. [The Big Crunch](14-the-big-crunch.md) — mmap, zero-copy pipelines, and putting it all together
15. [Read It Four Ways](15-read-it-four-ways.md) — answer, flow, lifetime, and work

The examples use today's `alignc`. Let's begin.
