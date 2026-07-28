# Owned tagged payload probe

Manual L1a regression evidence for `Option<string>` fields. The harness compares scalar and tagged
struct construction/pass/drop for always-`None`, always-`Some`, and 1%-`Some` inputs. It reports wall
time and reads the runtime `alloc-count` counters, requiring every expected string allocation to
have exactly one free. Separate rows cover `Some→Some→None→Some` field replacement; `if`, `match`,
and value-carrying `loop` replacements that move the old field on only one path; and an early `?`
after the first owned field was initialized. A bound `Result<T,string>` row verifies that the Err
edge of `?` transfers the owned error before exit cleanup. `run.sh` also records the raw LLVM
tag-guard count.

```text
bench/owned_tagged_payload/run.sh [native|baseline]
```

This is manual PR evidence, not an ordinary CI gate.
