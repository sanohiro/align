# Align Design Notes

## Why Align exists

Align is not an attempt to invent new syntax.

Align exists because modern software development has changed.

The old model:

```text
Human -> Code -> Compiler
```

The new model:

```text
Human -> AI -> Code -> Compiler
```

Language design must reflect this reality.

---

## The four-way alignment

Align seeks to align the following four parties.

```text
Human
AI
Compiler
Hardware
```

Most languages optimize for humans alone.

Align treats all four as first-class citizens.

---

## The central observation

Modern CPUs are extremely fast.

Modern compilers are extremely sophisticated.

Modern AI can write code.

Yet developers still hand-optimize the following.

```text
allocation
cache locality
SIMD
branch prediction
parallelism
```

Align seeks to make the optimal path the default path.

---

## Less code

One of the founding beliefs.

> Less code means fewer bugs.

A language should remove boilerplate wherever possible.

But it does not hide the following.

```text
allocation
errors
parallelism
unsafe operations
```

These stay visible.

---

## Convergence over expressiveness

Many modern languages maximize expressiveness.

Align maximizes convergence.

The goal:

```text
different developers
different AI models
different codebases
```

naturally arrive at similar solutions.

---

## One way

Align strongly prefers the following.

```text
one error model
one optional model
one ownership model
one parallel model
```

Converge on one rather than several competing approaches.

---

## Compiler-friendly first

Align is intentionally restrictive.

Restriction is not a weakness.

Restriction becomes information for the compiler.

The compiler should be able to infer the following.

```text
contiguous memory
no alias
cold error path
arena lifetime
non-null values
```

without requiring complex annotations.

---

## Hardware-friendly first

Performance begins with the cache.

Before SIMD.

Before GPU.

Before parallelization.

Key concepts:

```text
contiguous memory
SoA
hot/cold split
arena
chunk processing
```

---

## The SIMD philosophy

Align does not try to make developers write SIMD.

Align makes ordinary code naturally SIMD-friendly.

Examples:

```text
map
reduce
scan
filter
mask
```

These should lower naturally to vectorized code.

---

## The GPU philosophy

Align is not a GPU language.

Align only seeks to keep future GPU execution possible.

It prefers data-oriented operations because they map naturally to the following.

```text
CPU
SIMD
GPU
```

---

## The string philosophy

Strings are not magic objects.

Strings are data.

The goal:

```text
scan once
zero copy
builder based output
string pools
```

Repeated scanning should be avoided.

---

## The JSON philosophy

JSON is the de facto assembly language of modern APIs.

Align treats JSON as a first-class concern.

The goal:

```text
SIMD scanning
typed decode
zero-copy strings
field tables
arena allocation
```

---

## The safety stance

Align is intentionally positioned between the following.

```text
Rust
Zig
```

The position:

```text
safer than Zig
simpler than Rust
```

Normal code should be safe.

Dangerous code should be isolated.

---

## The AI philosophy

AI-friendliness is not a feature.

It is a design constraint.

What it avoids:

```text
complex lifetime systems
macro systems
multiple paradigms
excessive abstraction
```

What it prefers:

```text
predictability
clarity
consistency
```

---

## In one sentence

Align is a data-oriented language that aligns human intent, AI generation, compiler optimization, and modern hardware.
