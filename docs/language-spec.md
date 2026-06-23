# Align Language Specification v0.1 (Summary)

A summary of `draft.md` (the authoritative detailed spec). For detail and the latest version, always refer to `draft.md`.

## Purpose

Align is an AOT-compiled language designed to align the following.

* Human intent
* AI-generated code
* Compiler optimization
* Modern hardware

What Align prioritizes:

```text
Less code
Predictable performance
Compiler-friendly design
Data-oriented programming
```

## Core principles

* One way to do the same thing
* No hidden allocation
* No hidden error
* No hidden parallelism
* Data-oriented by default
* Cache-friendly by default
* SIMD-friendly by default
* AI-friendly by default

## What it includes

### Types

```text
bool

i8 i16 i32 i64
u8 u16 u32 u64

f32 f64

char

str
string
bytes
buffer
builder

Option<T>
Result<T,E>

(T, U, ...)   // anonymous tuple; multi-value return = returning a tuple

array<T>
slice<T>

vec<N,T>
mask<T>
bitset
```

### Memory

```text
value types
arena
explicit heap
unsafe
```

### Error handling

```text
Result<T,E>
?
```

No exceptions.

### Data processing

```text
map
par_map
filter
where
reduce
scan
partition
group_by
sort
chunks
```

### Reduction

```text
sum
min
max
count
any
all
dot
```

Stages and reducers take a named function or an inline lambda `fn x { ... }` (parameter types
inferred). A lambda may capture enclosing variables by value — with no hidden closure
environment (it compiles like a named function, captures passed as arguments). `where(.active)`
is shorthand for a one-field lambda.

### Strings

```text
str
string
bytes
buffer
builder
```

### JSON

```text
json.scan
json.decode
json.encode
json.validate<T>
```

`decode`/`encode` take no written type argument — the target type comes from
context (`u: User := json.decode(d)?`) or the value argument; Align has no
expression-position type-argument syntax (no turbofish). `validate<T>` is the
residual schema-selector case (type in neither args nor result), still open.

### Templates

```text
template
html
json
raw
```

### Parallelism

```text
par_map
reduce
chunks
task_group
```

No async/await in v1.

### Safety

Normal code:

```text
safe
```

Dangerous operations:

```text
unsafe
```

Only inside an unsafe block.

## Core library

```text
core.option
core.result

core.array
core.slice

core.vec
core.mask
core.bitset

core.builder

core.json
core.template

core.hash
core.math

core.arena
```

## Standard library

```text
std.io
std.fs
std.path
std.process
std.env
std.time
std.net
std.cli
std.encoding
std.compress
std.rand
std.crypto
std.http
```

## Packages

```text
pkg.db.*
pkg.web.*
pkg.rpc.*
pkg.cloud.*
pkg.ai.*
```

Not part of the language core.
