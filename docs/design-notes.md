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

### Why the default struct layout is unspecified (field reordering)

A normal struct's field order is the compiler's business, not the source's: fields are laid out in
descending alignment so padding disappears (`{ a: i8, b: i64, c: i8 }` is 16 bytes, not the 24 that
declaration order would waste). This is the cache-density lever applied to the *element* level — a
tighter struct means more elements per cache line, fewer bytes streamed, better use of every load —
and it is exactly the reasoning behind `soa<T>` and the arena, one layer down. It costs nothing: safe
Align has no field-address-taking, so the physical order is semantically unobservable (access is by
name), and there is a well-worn precedent — Rust reorders struct fields by default for the same
reason. The one place a fixed byte layout matters — crossing to C, `raw` memory, JSON's byte
contract, by-value register passing — already has its marker, `layout(C)`, which pins declaration
order. So the default optimizes for the machine, and the escape hatch is explicit and visible where a
human or an ABI actually needs the bytes nailed down: hardware-friendly by default, "nothing hidden"
where it counts.

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

**Escape joins and cleanup joins are different facts.** A region join deliberately keeps the
shortest lifetime any path may produce; using that conservative result to choose `free` would lose
which path actually allocated the value. Every resource-owning slot therefore has a path-local
individual-vs-arena bit. Value-carrying `block` / `if` / `match` / `else` / `?` lower that bit beside
the value and select both on the same CFG edge. This keeps the one region lattice conservative for
safety while making cleanup exact, without visible lifetime or ownership-mode syntax.

**Explicit `.clone()` over a hidden copy-on-escape.** A zero-copy decoded view that needs to
outlive its input is cloned *explicitly*; the compiler never inserts the copy silently. The
cache-friendly fast path — borrow the input bytes, process, discard — is identical either
way; the difference is only the rare escape, where a copy is physically unavoidable. Making
it explicit honors **Nothing hidden** (allocation is visible) and **Predictable performance**
(a small edit that starts escaping a value does not silently jump its cost class). This is the
hardware-aligned choice: predictable allocation beats convenience, and an in-arena clone is a
bump allocation, not a malloc cliff. (Convenience-first auto-copy was rejected for the same
reason exceptions and GC were — it hides cost.)

**An aggregate constant is a `slice<T>`, not an `array<T>` — ownership is a property of the type.**
A top-level array constant (`PRIMES := [2, 3, 5]`) could have been an owned `array<T>`, but that would
contradict the model: ownership is decided by the *type*, and a compile-time table owns nothing. It is
the exact analogue of a `str` literal — `GREETING := "hello"` is a `str` view of static bytes, not an
owned `string`; so `PRIMES` is a `slice<i64>` view of a static table, not an owned array. This falls
out of the region lattice for free: the elements are one **per-unit read-only data** table and the
constant is a `Static` `{ptr,len}` view of it, so it is shared (never copied), returnable from any
function, and never dropped — with no new mechanism. It also keeps **one way**: indexing, `.len()`,
slicing, and pipelines reach it through the existing borrowed-`slice<T>` paths, so there is no
array-constant-as-value seam and no allocation. Per-unit (not whole-program) rodata is the settled
storage: each importing unit rematerializes the constant from its exported initializer source, which
is exactly what makes cross-unit edits invalidate dependents through the interface hash for free. An
`array<T>` annotation is therefore rejected, not accepted-and-coerced — the type would be a lie about
ownership.

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
Ordinary sequential pipeline callables may also be Impure: deterministic input/stage order and
`where` guards preserve their observable behavior. Their inferred effect is optimization evidence,
not a rejection rule. Purity is inferred, never annotated, and is still weaker than
non-trapping/total execution.

---

## The loop philosophy

Align has exactly one loop construct, and it is deliberately narrow: `loop { ... break value }`.
**The pipeline owns the data path; `loop` owns the control path.** Traversing a collection is
`map` / `where` / `reduce` — that is not a style preference but what lets the compiler see
*which* data-parallel operation the code is (SIMD, fusion, offload). So `for x in xs` does not
exist: it would compete with the pipeline for the same territory, split the culture, and hide the
map/filter/reduce structure the compiler needs. What the pipeline cannot express is iteration
whose trip count is decided by the iteration itself — read until EOF, retry with backoff, drive a
protocol, pump a state machine to convergence. That category needs exactly one tool, and `loop`
is it.

**Why not recursion?** "No loops, use tail recursion" was considered and rejected — not on taste,
but because guaranteed tail-call optimization structurally conflicts with four load-bearing
decisions:

1. **Drops and regions kill tail position.** Move types drop at scope end and arenas free at
   scope end, so any frame holding one cannot tail-call — the cleanup runs *after* the call. This
   is the same reason Rust rejected implicit TCO. I/O pump loops are exactly the frames that hold
   Move values (`reader`, `buffer`), so TCO fails precisely where loops are most needed.
2. **`?` kills tail position.** The one error model makes sequential loops fallible, and
   `r.read(buf)?` followed by anything is not a tail call. An error model and a recursion-based
   loop model fight each other.
3. **Nothing hidden.** Whether a call is in tail position is invisible in source; a one-line
   refactor silently turns O(1) stack into O(n) and surfaces as a runtime stack overflow. Align
   does not build hidden failure modes into its basic iteration idiom.
4. **Compiler- and AI-hostile.** A loop back-edge is the friendliest CFG LLVM can get;
   reconstructing loops from recursion is the fragile inverse. And accumulator-threaded tail
   recursion is a known bug source for both humans and models, while `loop` + `mut` state is not.
   Recursion-as-iteration loses on all four alignment axes at once.

Recursion itself remains legal — a parser or a tree walk is genuinely recursive — but it is for
recursive *problems*, never a substitute for iteration, and Align guarantees no TCO.

**Why not `while`?** `while cond` is a second loop form that cannot yield a value; `loop` with
`break value` subsumes it and stays an expression like `if` / `match` / `arena`. **Why no
`continue` or labels?** Minimality with an exit: skip-to-next is an `if` around the rest of the
body, and a nested loop needing a two-level exit is a function waiting to be extracted. Both can
be revisited on real-code evidence; starting without them is the smaller regret.

The boundary is enforced, not hoped for: walking an array by index inside a `loop` draws a
"write it as a pipeline" lint — the same pattern as the unnecessary-heap and unhandled-`Result`
lints. `loop` also finally gives the deferred frequency-dependent lints
(allocation-in-loop, branch-in-hot-loop, `prefer-pipeline-over-vecN`) their firing surface.

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
pipeline walks memory sequentially (no random jumps), and safe primitive conditional reductions can
lower to a mask + `select` — so the predictable shape, not hand-tuning, is what keeps hot loops
vectorizable. A callable after `where` **must be guarded** unless it is separately proven safe on an
inactive lane; the current reducing lowering does not yet do this. Pure alone is insufficient because
a Pure function may trap (audit: `impl/12` §3.1).

**Branchless is for vectorization, not because branches are slow (recorded 2026-07-04, external
design-note review adoption).** Modern branch predictors (TAGE-class) make well-predicted branches
near-free, and scalar CMOV chains create data dependencies that can be *slower* than branching.
Align's masked `where` form exists because select/predication enables SIMD for operations that are safe
on inactive lanes, not as a scalar-branch-avoidance dogma — don't cargo-cult branchless into scalar
std code or speculate trapping callables. The one
exception where branchless is mandatory is `std.crypto`'s constant-time requirement (see
`open-questions.md`).

SIMD lives in two layers, and the split is deliberate. **`vecN<T>` / `maskN<T>` are an escape hatch**
for hand-tuned fixed-width register kernels (a dot product, an FMA loop, a FIR filter) — they are
*always* a fixed size, so they can be a `Copy` register value with a constant `sizeof` and constant
lane indices. **The pipeline (`map` / `where` / `reduce`) is the main road**, and it never names a
width — which is exactly why a future scalable ISA (SVE/RVV) lives here invisibly: the same source
lowers to a fixed-width loop on NEON/AVX or scalable predicated codegen on SVE/RVV, chosen in the
backend. That a width is *not* in the source is consistent with "nothing hidden": a vector length,
like the AVX-vs-NEON choice itself, is a hardware detail, not a semantic effect — so hiding it (unlike
allocation, errors, or parallelism, which are real effects) is correct, not a leak.

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
  default, visible opt-in, never hidden. The right frame is a *portable per-arch vectorization
  strategy*, not one fixed width: on fixed-width ISAs (AVX/NEON) the baseline is 128-bit + a scalar
  remainder, but on a scalable ISA (SVE/RVV) it is scalable *predicated* codegen — one binary that
  adapts its vector length at run time, not a 128-bit cap. MIR stays width-agnostic precisely so the
  backend can make that per-arch choice (`impl/04 §4`, `impl/05 §5`).
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

Decode is **strict and exactly-once**: every declared field must appear once; a missing *or duplicated*
declared field is an `Err`, never a serde-style silent last-wins (undeclared keys are the only thing
skipped). The reason is the one error model: decoding into a fixed struct, a duplicate key is a data
error, and surfacing it as a value beats a silent partial decode — "nothing hidden". This is the
intended contract; the current decoder's speculative fast-path has one known narrow deviation, tracked
as a pre-freeze gap in `open-questions.md`. (Settled 2026-06-29.)

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

## The performance philosophy

Stated by the owner (2026-07-11, the optimization consultation): the ideal is that

> normally-written Align compiles to what an expert would have hand-tuned in Rust.

Three consequences shape every performance decision:

- **Constraints buy information.** The reason a pipeline can fuse, vectorize, and skip bounds
  checks is that `map/where/sum` LEAVES the intent standing — a hand-written loop destroys it and
  forces rediscovery. Every "one way" restriction is a promise kept to the optimizer (and, dually,
  to the adversarial reviewer: the same legibility that enables optimization makes verification
  converge).
- **Data movement before instruction execution.** Align optimizes what is read, in what order,
  from how few cache lines — before it optimizes how it is computed. The measured wins bear this
  out: the soa column scan beats Rust 8–10× as a *cache* win, not a SIMD win. Contiguous by
  default; indirection visible in the type; only needed fields loaded; hot and cold data apart;
  memory traffic weighed alongside asymptotic complexity.
- **The benchmark target is a triple.** Align-normal vs Rust-normal vs Rust-expert. Winning every
  case against Rust-expert is not the bar (both end in the same LLVM); the bar is
  **Align-normal ≈ Rust-expert at a fraction of the effort** — with the receipts (benches, and
  eventually the per-build optimization report) checked in.

Speed alone is not the moat — expert Rust catches up. The moat is speed that is **explainable**
(the compiler says why a loop did or didn't vectorize), **verifiable** (shape tests pin the fast
form), and **non-regressable** (CI gates on allocation/fusion counts). Fast, and provably so.

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

## Why `core.hash` is one dependency-free mixer over bytes

`core.hash` exposes a single canonical non-crypto hash (`wyhash`) over a byte view, not a generic
`Hash` trait over arbitrary values. Three forces converge on that shape:

- **One way.** A public `hash64` forces a decision the `group_by` perf work kept deferring — *which*
  hash is Align's non-crypto hash (FxHash vs `ahash` vs hand-rolled AES). Picking one canonical mixer
  and pointing every internal path (group_by, dict-encode, the JSON PHF) at it is the convergent
  answer; two "non-crypto hashes" would be the thing to avoid. Realized 2026-07-03: all three now
  route through the one `align_hash::wyhash` (see `open-questions.md`), replacing FxHash / FNV-1a.
- **Minimal-runtime identity over peak speed.** `ahash` (AES-NI) benched faster but adds a dependency
  and a cross-arch fallback to a runtime whose whole identity is small/zero-dep/predictable. `wyhash`
  is ~40 lines, dependency-free, strong-avalanche, and proven — the ideal fit. Speed that costs the
  identity is the wrong trade here (it can still be revisited as a perf lever, isolated).
- **No trait complexity.** Hashing arbitrary values needs a `Hash` derivation mechanism — a trait
  system Align deliberately doesn't have. Hashing a *byte view* (`str`/`slice<u8>`) needs none: the
  data-oriented core already hands you bytes. `hash128` returns a tuple, not a `u128`, for the same
  reason group_by returns columns — the small, explicit, data-shaped value, no new scalar width.

---

## In one sentence

Align is a data-oriented language that aligns human intent, AI generation, compiler optimization, and modern hardware.
