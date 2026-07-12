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

## Sequential control

For a long time the language had no loop construct at all.

Collection iteration was the pipeline; the rest was said to be recursion.

Recursion-as-iteration was rejected (2026-07-09).

The reasons:

```text
scope-end drops kill tail position
? kills tail position
TCO is invisible in source
loop back-edges are what compilers want
```

`for` and `while` were also rejected — `for` competes with the pipeline,
`while` is a second loop form that cannot yield a value.

The direction chosen:

```text
loop { ... break value }
```

One narrow expression. The pipeline owns the data path; `loop` owns the control path.

---

## Sequential pipeline effects

The implementation accepted Impure sequential callables while early implementation notes described
all data-processing callables as Pure. The conflict became observable when branchless `where`
speculated a later callable on a rejected element.

The direction chosen (2026-07-13):

```text
sequential pipeline  -> Impure allowed, exact guarded input/stage order
par_map              -> Pure required
```

Effect inference controls optimization legality. It does not reject ordinary sequential effects,
and Pure alone does not make a trapping or nonterminating call safe to speculate. `sort_by_key` key
evaluation remains separate because comparison sorting has a data-dependent call count.

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
