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

## Memory model v2: one region lattice, explicit copies

(Design: `impl/08-memory-model-v2.md`. Decided as a whole before M6.)

Two principles drove the load-bearing choices.

**One region lattice, not three point solutions.** Escape safety started as three unrelated
mechanisms (arena depth for `box`/`str`, a "local-backed" flag for slices, a region-0
restriction for struct `str` fields). They are unified into a single total order
`Static ⊐ Frame ⊐ Arena(1) ⊐ … ⊐ Arena(d)` with one rule — a value may only be stored or
returned where it outlives the destination. Regions stay **inferred** (no lifetime syntax,
ever); they are an analysis result, not a surface type. Restriction-as-information: one
lattice keeps the checker simple and preserves the optimizer's no-alias / contiguous /
arena-lifetime facts.

**Explicit `.clone()` over a hidden copy-on-escape.** A zero-copy decoded view that needs to
outlive its input is cloned *explicitly*; the compiler never inserts the copy silently. The
cache-friendly fast path — borrow the input bytes, process, discard — is identical either
way; the difference is only the rare escape, where a copy is physically unavoidable. Making
it explicit honors **Nothing hidden** (allocation is visible) and **Predictable performance**
(a small edit that starts escaping a value does not silently jump its cost class). This is the
hardware-aligned choice: predictable allocation beats convenience, and an in-arena clone is a
bump allocation, not a malloc cliff. (Convenience-first auto-copy was rejected for the same
reason exceptions and GC were — it hides cost.)

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

**Owned `string` stays `{ ptr, len }` — no Small-String Optimization.** SSO (inline
`{ ptr, len, cap }` with a tag bit) was considered and rejected: it adds a branch to every
access and breaks FFI pointer stability, while Align's arena model already avoids the
small-`malloc` churn SSO targets — so it trades "predictable performance" + "nothing hidden"
for a marginal win. (Settled in `open-questions.md`.)

**Output writes into a `builder` sink, not a returned string.** The library convention is
`write_json(out: mut builder, …)` over `to_json() -> string`: serialization/formatting append
into a caller-provided buffer (often arena-backed), so complex output costs zero heap
allocations. Paired with read-oriented `std` APIs returning views (`str`/`slice`/`bytes`)
rather than owned copies, this makes zero-allocation pipelines the default. (A std design
rule — `open-questions.md` Future "Library architecture principle".)

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
