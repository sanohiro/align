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
