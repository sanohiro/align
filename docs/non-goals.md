# Align Non-Goals

The following are intentionally not goals.

## Not a replacement for C++

Align does not try to support every programming style.

---

## Does not aim for maximum expressiveness

Align does not optimize for the following.

```text
metaprogramming
DSL creation
advanced type wizardry
```

This includes a **general compile-time execution model** (Zig-style `comptime` / user-defined CTFE
running arbitrary code at compile time). It is a deliberate non-goal: a second computation model
erodes *One Way* and AI-friendliness. Align's compile-time story is **builtin-driven static data**
only — JSON field tables, `template` analysis, literal/hash tables — computed by the compiler, not
by user `comptime` code. (Considered and declined in the 2026-06-24 language stock-take.)

---

## Not OOP-first

Align is not object-oriented.

There is no goal to support the following.

```text
class hierarchies
inheritance
deep object graphs
```

---

## No runtime magic

What it avoids:

```text
hidden allocation
hidden exceptions
hidden thread creation
hidden copying
```

---

## Not async everywhere

Align does not build the language around async/await.

Primary model:

```text
map
reduce
chunks
task_group
```

---

## No trait complexity

Avoids Rust-style complexity.

---

## No template complexity

Avoids C++-style template complexity.

---

## Not GC-first

Align is not a garbage-collection-centric design.

---

## Not framework-driven

Web frameworks, ORMs, cloud SDKs, and AI SDKs belong to packages.

Not in core.

---

## Not GPU-only

Align is GPU-compatible.

Not GPU-centric.

---

## Does not pursue academic purity

Prioritizes practical performance over theoretical elegance.
