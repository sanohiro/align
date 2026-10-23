# Align Non-Goals

These boundaries keep Align focused on data-oriented programs with explicit resource costs.

## Not a replacement for C++

Align does not try to support every programming style.

---

## Does not aim for maximum expressiveness

Align does not optimize for the following.

```text
metaprogramming
DSL creation
user-defined type-level computation
```

This includes a **general compile-time execution model** (Zig-style `comptime` / user-defined CTFE
running arbitrary code at compile time). It is a deliberate non-goal: a second computation model
erodes *One Way* and AI-friendliness. Align's compile-time story is **builtin-driven static data**
only — JSON field tables, `template` analysis, literal/hash tables — computed by the compiler, not
by user `comptime` code. (Considered and declined in the 2026-06-24 language stock-take.)

---

## Not OOP-first

Align models data with records and sums. It does not provide:

```text
class hierarchies
inheritance
deep object graphs
```

---

## No runtime magic

These operations must remain visible:

```text
allocation
error propagation
thread creation
copying
```

---

## Not async everywhere

Data parallelism uses pipelines; I/O concurrency uses structured task groups:

```text
map
reduce
chunks
task_group
```

---

## No trait complexity

Generic constraints are closed builtin bounds. User-defined trait systems are outside the scope.

---

## No template complexity

Generics use type parameters and monomorphization. Macro-based substitution and template
metaprogramming are outside the scope.

---

## Not GC-first

Align is not a garbage-collection-centric design.

---

## Not framework-driven

Web frameworks, ORMs, cloud SDKs, and AI SDKs belong to `pkg`, outside the language core.

---

## Not GPU-only

The data-oriented core keeps future GPU execution possible. CPU programs remain primary; a GPU
backend is not implemented.

---

## Does not pursue academic purity

Prioritizes practical performance over theoretical elegance.
