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

This is why **multi-value return is just returning a tuple**, not a separate mechanism. A
Go-style "multiple return values" feature produces several values that are not themselves a
value (you can't store, nest, or array them) — a second, special-cased way to hand back more
than one thing. Align instead has one first-class anonymous product type, the tuple `(T, U)`,
the positional companion of the keyword-less named struct: a named struct for a domain type, a
tuple for an ad-hoc result. A tuple's ownership falls out of its elements (the same rule as a
struct), so it adds no new ownership concept either.

It is also why there is **one filtering operation, `where`** — not both `where` and `filter`.
The two would be exact synonyms (keep the elements a predicate selects), and two names for one
operation is precisely the divergence Align rejects: different code would arrive at different
spellings of the same thing. `where` wins because it reads naturally with field selectors
(`users.where(.active)`) and fits the data-oriented, query-like core. (`filter` was dropped from
the function list for this reason.)

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

## The lambda philosophy

Lambdas (`fn x { ... }`) are not a separate paradigm bolted onto the data-processing core — they
**are** how you pass behavior to `map` / `where` / `reduce` / `par_map`. A lambda and a named
function are the same thing; the lambda just spares you a top-level declaration for a one-off.

The load-bearing decision is **how capture works**. A lambda that captures an enclosing variable
does *not* allocate a hidden closure environment: it is lifted to an ordinary function whose
captured values become extra parameters, passed at the call site. So a captured pipeline lambda
fuses into the same counted loop as a named function and carries zero allocation — the capture is
just a loop-invariant argument the backend hoists. This keeps lambdas inside the existing
guarantees rather than introducing a new cost class: **Nothing hidden** (no silent heap
environment), **Predictable performance** (a lambda is never secretly slower than the named
equivalent), and **Compiler-friendly** (the optimizer sees a direct call, not an indirect one
through a closure object).

**Escape decides the representation.** A lambda that *escapes* — stored in a variable, returned,
or handed to `task_group`'s `spawn` to run later — outlives the locals it captures, so it cannot
borrow them; it needs a **closure environment** holding the captured values. The compiler's escape
analysis (the same one that governs views and arenas) chooses: a non-escaping lambda (every
pipeline `map`/`where`/`reduce`/`par_map`) is inlined with captures-as-parameters — zero
allocation, SIMD/GPU-friendly; an escaping lambda gets an environment. That environment is not a
new hidden cost class: it is **owned by the enclosing region** — the `task_group {}` / `arena {}`
scope it escapes into — and freed with that region, exactly like every other region allocation. So
it stays inside the one region-based allocation model, and the *visible* act of escaping (and the
*visible* enclosing scope) is the allocation boundary — consistent with **Nothing hidden** (no
silent free-floating `malloc`). This is the load-bearing design point: it lets first-class function
values and `task_group` exist *without* eroding the inlined, offload-ready pipeline path, and the
two paths are distinguished by escape, not by two different lambda syntaxes. (The allocation model
for a closure that escapes *every* region — e.g. one returned to an unbounded caller — is part of
the deferred first-class-closure design; the `task_group` consumer is scope-bounded and clean.)

The **Side Effect Rule** completes the picture: a `par_map` lambda must be Pure (it may read
captured values but not mutate external state), which is what makes data-parallel execution safe
without locks. A `task_group` task, by contrast, *may* be impure — it performs I/O — and its
safety comes from capture being by value (no shared mutable state) rather than from purity.
Purity is inferred, never annotated.

---

## The SIMD philosophy

Align does not try to make developers write SIMD.

Align makes ordinary code naturally SIMD-friendly.

Examples:

```text
map
reduce
scan
where
mask
```

These should lower naturally to vectorized code. The point is *structural*: contiguous arrays mean a
pipeline walks memory sequentially (no random jumps), and `where` / conditional reductions lower
branchless (mask + `select`, not a per-element `if`) — so the predictable shape, not hand-tuning, is
what keeps hot loops vectorizable.

For the layout itself, Align takes the **explicit `soa<T>` over automatic inference** road. The safe
core has no raw pointers or field-address-taking, so a struct array's physical layout is
semantically unobservable — the compiler *could* silently turn `array<User>` into struct-of-arrays.
We deliberately don't: a silent layout switch hides performance (against "predictably fast") and
needs an opaque heuristic. Instead the choice is one visible token — `array<User>` (rows) vs
`soa<User>` (columns) — and the compiler does the field-wise column lowering *under* that type. So
the decision is explicit and predictable; the mechanism is automatic. It is the principled,
first-class form of the "split it into parallel arrays by hand" trick every data-oriented programmer
already reaches for.

### Where the SIMD actually comes from (and why the default build is conservative)

Align targets the real deployment world — **cloud and containers, where you build once and run on an
unknown, varied fleet** (Intel/AMD/Graviton, feature-masked or live-migrated hosts). A binary baked
for the build host's CPU (`native`), or for a high fixed baseline like AVX2, would crash (`SIGILL`)
on some hosts. So the philosophy splits SIMD by layer:

- **Generated code** is fixed at build time, so it targets a **safe, portable per-arch baseline by
  default** (`x86-64-v2` / `armv8-a`). `native` and higher baselines are **opt-in** — one good
  default, visible opt-in, never hidden. This caps generated-loop SIMD at the baseline (128-bit) for
  portability.
- **Wide SIMD on a varied fleet comes from the library**, via *runtime* CPU-feature dispatch (the
  binary detects AVX2/NEON at run time and falls back safely). This is why the library leans on
  portable dispatching crates rather than hand-written intrinsics: it adapts per-host *and* stays
  multi-arch (x86-64 + aarch64) from one source. The heavy SIMD work (JSON, string scan, bulk copy)
  lives here precisely because this is the only layer that can adapt at run time.

The lesson: for an AOT language aimed at the cloud, "automatic SIMD" is not a single fixed target —
it is a conservative portable floor in the codegen plus runtime-adaptive SIMD in the library.

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

"Typed decode" is written `u: User := json.decode(d)?`, not `json.decode<User>(d)`.
A decode's target type is return-position-only — it cannot come from the arguments —
so it is recovered from the expected type propagated from context (the binding
annotation, flowing back through `?`). Align deliberately has **no
expression-position type-argument syntax** (no turbofish): the binding annotation is
the single place a type is written ("one way"), and refusing `f<T>(x)` removes the
`<`-vs-comparison parse ambiguity at expression position outright — the same
ambiguity that pushed Go to `f[T](x)` and Rust to `::<>`. When context supplies no
type, that is a hard error asking for an annotation, never a silently-defaulted type.
(Settled 2026-06-22; see `open-questions.md`.)

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

## The resource-oriented north star

A sharpening of the AI philosophy, not a new direction (recorded 2026-06-28 from the `work/`
research sweep; benchmarks in `open-questions.md`).

AI can now write a lot of code. That shifts the language's job. The question is no longer only
"can a skilled human express this?" but:

```text
when AI writes ordinary code, does it land on a fast, predictable, resource-aware shape by default?
when the user's CPU / GPU / RAM / VRAM / SSD is limited, does the language help use what they have?
when code is slow, can the toolchain explain why before days are wasted?
```

This is the Rust contrast, stated as a different bet:

```text
Rust:  a skilled human can write very fast, very safe systems code.
Align: AI-written ordinary code should fall into fast, safe-enough, resource-aware rails.
```

Rust rewards expertise. Align reduces the expertise required to *avoid the obvious resource mistake*.
The win is not "a stronger optimizer than Rust" — flat scalar loops hit parity, same LLVM. The win
is that **the slow shape is hard to write**: SoA over `Vec<Struct>`, fused pipelines over intermediate
arrays, arena over per-object alloc, zero-copy views over `read_file` copies, sink-first buffered I/O
over flush-per-write, dictionary ids over hot-loop string hashing. The benchmarks bear this out (SoA
column scan ~11×, mmap view ~12×, buffered stdout ~355×, dictionary-id reuse ~21× — all measured).

Consequences already in the design:

```text
- fast data layout is the default rail (soa<T>, fusion, columnar group_by)
- cost is visible (no hidden alloc / copy / async / thread)
- memory layout is a first-class, explicit choice (type- and scope-driven, never whole-program inferred)
- I/O is sink-first, buffered, region-scoped (mmap views, writev, io.copy)
- the std library encodes performance rails, not only convenience APIs
- diagnostics explain resource mistakes in plain terms (the perf-rail lints)
```

The north star, plainly: *a constrained person, on constrained hardware, should be able to ask AI to
write systems code that lands on the fast path by default.* This is not a claim that weak hardware
beats expensive hardware — it is a claim that the floor rises. Local LLM inference is the headline
long-term instance of this pressure (recorded as a Future direction in `open-questions.md`); it is a
direction, not a v1 commitment, and it must not distort the language into a GPU-only ML framework.

---

## In one sentence

Align is a data-oriented language that aligns human intent, AI generation, compiler optimization, and modern hardware.
