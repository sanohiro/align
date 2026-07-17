# The Little Aligner

> 🌐 **English** · [Japanese](./ja/README.md)

*In the tradition of "The Little Schemer."*

This is not a reference and not a textbook — [the guide](../guide/README.md) is the textbook. This is a **drill book**: a long conversation of small questions and small answers, each one a half-step past the last. It teaches your hands the parts of Align that are unlike other languages — pipelines, `match`, Move, arenas, and turning data sideways into columns — until the idioms come out on their own.

## How to use it

Read a question. **Answer it out loud before reading the answer.** If you were right, keep going; if you were wrong, back up a few questions — the answer was built there. When a program appears, you may run it (`alignc run`), but try to be the compiler first: most questions can be answered with nothing but the previous page.

Some answers are one word. Some questions look identical to the one before — the difference is the lesson. And when a rule has earned it, it is carved into a **Commandment**.

## The chapters

1. [Toys](01-toys.md) — values, bindings, and functions
2. [Do It Again](02-do-it-again.md) — `map`
3. [Keep Some](03-keep-some.md) — `where` and field projections
4. [Collapse It](04-collapse-it.md) — reductions: `sum`, `count`, `reduce`, and friends
5. [Chains](05-chains.md) — whole pipelines, and why they cost one loop
6. [One of Many](06-one-of-many.md) — sum types and `match`
7. [Maybe, or It Failed](07-maybe-or-it-failed.md) — `Option`, `Result`, `?`
8. [Whose Is It?](08-whose-is-it.md) — Copy, Move, arenas, and `.clone()`
9. [Turn It Sideways](09-turn-it-sideways.md) — `soa`: data as columns
10. [Count Me by Name](10-count-me-by-name.md) — `group_by`, `agg`, `dict_encode`
11. [Do It Until](11-do-it-until.md) — the `loop` expression, when a pipeline can't say it

Everything here runs with today's `alignc`. Bon appétit.
