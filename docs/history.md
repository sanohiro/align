# History of Align

## The first idea

The project began with a simple observation.

> The same thing should not have many ways to be written.

This led to the following.

```text
one error model
one ownership model
one optional model
```

---

## The performance discussion

The focus shifted to the following.

```text
cache locality
allocation cost
memory layout
```

over raw instruction performance.

Observation:

The cache is often more important than SIMD.

---

## The turn toward data orientation

The discussion moved away from OOP.

Where it headed:

```text
array processing
SoA
hot/cold split
chunk processing
```

---

## The AI-era discussion

The big realization:

Programming is now this.

```text
Human -> AI -> Compiler
```

This changed the priorities.

What the language should optimize for:

```text
convergence
predictability
consistency
```

over maximal freedom.

---

## Error handling

The exception-based approach was rejected.

Go-style explicit error handling was judged too verbose.

The direction chosen:

```text
Result<T,E>
?
```

---

## Memory model

The GC-first approach was rejected.

Rust-style visible lifetimes were judged too heavy.

The direction chosen:

```text
value types
arena
explicit heap
unsafe isolation
```

---

## The SIMD direction

The goal:

Not to make developers write SIMD.

But to make them write code that naturally becomes SIMD.

This led to the following.

```text
map
reduce
scan
mask
vec
```

These became core concepts.

---

## The string and JSON direction

Repeated scanning was identified as a major cost.

The direction chosen:

```text
scan once
reuse metadata
builder output
zero copy
field tables
```

---

## The compiler-friendly direction

Restrictions were added intentionally.

The goal:

To enable compiler inference.

Rather than requiring programmer annotations.

---

## Library structure

The final direction:

```text
core
std
pkg
```

core contains data-processing primitives.

std contains OS integration.

pkg contains frameworks and the ecosystem.

---

## Naming

Several names were considered.

For example:

```text
Opt
Air
Bound
Fuse
Grain
```

The final front-runner:

```text
Align
```

The reason is that it expresses the alignment of the following.

```text
Human
AI
Compiler
Hardware
```

while also pointing to the following.

```text
memory alignment
cache alignment
SIMD alignment
```
