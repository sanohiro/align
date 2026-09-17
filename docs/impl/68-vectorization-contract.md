# Vectorization contract

Status: design of record for issue
[1088](https://github.com/sanohiro/align/issues/1088). This document is the
public-contract ledger for guarantees G1–G10; it settles what the compiler
promises, who owns each promise, and what proves it. It implements nothing.
Each owner issue stays independently landable, but acceptance is judged against
the whole contract.

Evidence baseline: Align `8c8bfbc7a3169e84ecc8415f5149ab8c61afe863`
(alignc 0.7.5, LLVM 22.1.8), Apple M1, `--profile release`, default
`--target-cpu baseline`. The measurements quoted below are the align-llm binary
audit recorded on issues 1069–1087; align-llm is evidence, not the yardstick.
The yardstick is Align as a general data-oriented language.

## 1. The rule

The data-oriented core promises that normal array and slice pipelines lower
well to SIMD. Today they do not: a 5.3 MB numeric release image contains zero
floating-point SIMD instructions in any Align-generated function. The contract
adopted here is one sentence:

> The compiler removes a blocker it can prove away rather than documenting it.

Three corollaries bound that sentence, and every guarantee below is written to
respect all three.

```text
own facts only     a fact Align states to LLVM must be a property of Align's
                   own lowering or representation, never one minted from
                   source spelling (63-codegen-performance-audit.md:218,
                   64-composed-byte-optimization-plan.md:186)
no silent relaxing  no guarantee relaxes IEEE 754 semantics without a visible
                   source scope; floats never abort
say why            when a loop stays scalar anyway, the compiler names the
                   blocker (G10); a scalar loop is never a silent outcome
```

Each guarantee separates two different kinds of statement, and the difference
is load-bearing for acceptance:

```text
promise   a property of emitted MIR or LLVM IR that an owner test pins by
          shape; a change to it is a red test
try       a profitability decision left to LLVM's stock pipeline; observable
          through explain-opt, never pinned as a shape
```

Align does not promise that a given loop vectorizes on a given target at a
given width. It promises that nothing Align emits structurally prevents the
stock pipeline from vectorizing it, and that the residual decision is visible.

## 2. What actually blocks SIMD today

Measured, not inferred. The owner column names the guarantee that removes it.

| # | Blocker | Layer | Measured cost | Removed by |
| --- | --- | --- | --- | --- |
| B1 | Borrowed view header `{ptr,len}` re-loaded per use; no alias facts on `borrow` header params | codegen | 8–10× on `out[i] = out[i] * k` and AXPY; 12,185 LICM remarks | G1 |
| B2 | Two-compare bounds check per access; two or more surviving checks switch LoopVectorize off | MIR | 16–30× on matvec; 3,605 trap branches | G2 |
| B3 | Counted loop puts its trip-count exit in the header; LLVM rejects early-exit vectorization structurally | MIR lowering | 10.5× on a byte scan | G3 |
| B4 | Pipeline `.max()`/`.min()` and `if x > best` lower to `fcmp`+`select`; scalar `a.max(b)` lowers to `llvm.maximum` | MIR/codegen inconsistency | 16× (191 µs → 11.8 µs on 152k f32), no semantics change | G4 Part 1 |
| B5 | No scoped reassociation or contraction, so f32 `sum`/`dot` stay serialized | missing surface | 13–17× against clang `reassociate(on) contract(fast)` | G4 Part 2 |
| B6 | Runtime primitives in loops are opaque calls with no effect attributes | runtime ABI | memset, LICM and vectorization all blocked | G5 |
| B7 | Transcendentals lower to libcalls; no vector math implementation is configured | codegen/driver | a `pow` loop "vectorizes" to per-lane `bl _powf` plus shuffles, slower than scalar | G6 |
| B8 | Byte data cannot become `slice<T>` without a copy | language | 65.7 µs copy per 152k f32; typed SIMD unreachable from bytes | G7 |
| B9 | `maskN<f32>` cannot `select` a `vecN<i32>` | sema | vectorized argmax with exact indices inexpressible | G8 |
| B10 | Cross-unit non-generic `pub fn` in a pipeline stage stays a call; generic ones inline | build model | stage loop stays scalar | G9 |
| B11 | An in-loop early exit prevents vectorizing the whole loop | source shape / MIR | argmax stays scalar | G3 + G10 |

Two conjunctions are recorded here because a partial fix is not measurable:

- **B1 and B2 are conjunctive.** On the byte-to-`f32` conversion kernel, hoisting
  the in-loop checks alone gives 0 `vector.body`; `noalias` on the borrow header
  alone gives 0; both together give 2 vector bodies and 4.7×.
- **Typed data alone buys nothing.** argmax over `slice<u8>` plus `f32_le`
  measures 190.6 µs; over an already typed `slice<f32>`, 190.4 µs. G7 removes a
  copy, not a scalar loop.

## 3. The ledger

Every row is a public promise. "Layer" is the single owner; a guarantee with two
owners is a guarantee nobody owns.

| G | Exact statement | Promised | Tried | Layer | Acceptance corpus | Implementing issue |
| --- | --- | --- | --- | --- | --- | --- |
| G1 | A borrowed view header is materialized once per function or loop preheader, and header memory is stated distinct from element memory | one header load per loop, dominating the loop; a TBAA node pair separating `align.view.header` from `align.elem.<T>`; `noalias dereferenceable(16) align 8` on `borrow` header parameters | that LICM then hoists everything else | LLVM lowering | `vectorize_shapes` owner `g1_view_header_hoisted`; `bytes_to_f32_out` conjunction pin | 1079 |
| G2 | At most one bounds check per loop, in the preheader, for a monotone index | the check is fused to one unsigned compare; for a monotone index with loop-invariant bound the guard is *moved* to the preheader, not deleted; trap text and first-failing-access iteration unchanged | which residual checks LLVM then folds | MIR | `vectorize_shapes` owner `g2_monotone_check_hoisted`; trap-parity owners for zero-length, length-1, first-out-of-range | 1081 |
| G3 | A counted loop lowers with its trip-count exit at the latch; the recognized shape produces one named MIR fact | recognition is total for the canonical shape: zero-trip test peeled into the preheader, only body-derived exits in the header, `i + step REL bound` at the latch; the fact carries trip count, step and monotone index and is the single source G1's and G2's owners read | whether LLVM's early-exit vectorizer then fires | MIR lowering | `vectorize_shapes` owner `g3_counted_latch_exit`; zero-trip, one-trip, first-element exit, last-element exit, no-exit owners | 1084 |
| G4 | One floating-point semantics model: uniform `minimum`/`maximum` lowering, ordered by default, relaxation only inside an explicit lexical scope | Part 1: all three spellings of a float min/max emit `llvm.minimum`/`llvm.maximum`, with NaN propagation and ±0 ordering unchanged. Part 2 (future): `reassoc` and `contract` are named individually and scoped lexically without inheriting across a function boundary; `nnan` and `ninf` are permanently excluded | the vector width LLVM picks for the resulting reduction | MIR (Part 1); sema + MIR (Part 2) | `vectorize_shapes` owner `g4_minmax_uniform_lowering`; NaN, `-0.0`/`+0.0`, all-NaN conformance owners; `k6_float_sum_does_not_vectorize_without_fast_math` as the standing negative control | 1082 Part 1 (now), 1082 Part 2 (future RFC) |
| G5 | Every runtime primitive that can appear in a loop has an effects record and either an inline fast path or a vector form | each runtime ABI symbol carries a complete memory-effects attribute; per-element primitives have a visible inline fast path and a visible slow path; `Result`/`?` failure edges are cold with one model | which of the fast paths LLVM then widens | runtime ABI + LLVM lowering | the runtime-ABI owner in `20-runtime-abi-ledger.md`, extended by plan 70 | 1071, 1072, 1073, 1074 |
| G6 | Every `core.math` function has a vector lowering on every supported target and one accuracy contract that holds for both lowerings | a documented ULP bound per function; scalar and vector results agree within it; the same results on every supported target; no scalar libcall inside a vector body | which loops the vectorizer chooses to widen | LLVM lowering + driver | `vec_math` owner extended to the exp/log family; a ULP conformance owner; `examples/vec_math.align` | 1063 (revised, §6.1) |
| G7 | Bytes reach typed slices through one checked, order-explicit, zero-copy view | construction allocates nothing and copies nothing; alignment and length are validated and yield `None` rather than trapping; the view carries the source borrow's authority and provenance; the named byte order must be the target's native order or the program is rejected at compile time | that a loop over the result then vectorizes — that is G1–G4 | sema (+ MIR for the lowering) | a view owner asserting no allocation and no copy in emitted IR; `vecN` load reachable from `buffer.bytes()`; a negative owner for a non-native order | 1064 (revised, §6.2) |
| G8 | A mask is structural: lane count and lane bit width, not element type | `select` accepts any mask whose lane count and lane width match the blended vectors; emitted IR is a plain `select <N x i1>` with no conversion; a lane-count or lane-width mismatch keeps its existing diagnostic | nothing | sema | `examples/vec_argmax.align` (planned) plus its codegen owner; negative owners for `mask4<f32>` gating `vec2<f64>` and `vec8<i32>` | 1083 |
| G9 | A function called from a pipeline stage or a hot loop is inlinable across units | a non-generic `pub fn` body under a size budget travels in the unit interface exactly as a generic body does today | whether LLVM inlines it at a given call site | driver + `align_interface` | a two-unit owner asserting the stage loop vectorizes across the unit boundary | 1066 (comment); no separate issue yet |
| G10 | When a pipeline or counted loop stays scalar, the compiler says why | the vectorizer remarks `explain-opt` already collects are promoted to a first-class diagnostic on the loop, naming the blocker | nothing | driver (`09-explain-opt.md` owner) | an `explain-opt` owner asserting a known-scalar loop names its blocker | unfiled; see §4 |

### G1 — view header materialized once, alias facts stated

The smallest possible in-place kernel is the witness.

```align
fn scale_only(borrow mut out: array<f32>, k: f32) {
  mut i := 0
  loop {
    if i >= out.len() { break }
    out[i] = out[i] * k
    i = i + 1
  }
}
```

As emitted today the data pointer and the length are both reloaded every
iteration, so the store is assumed to invalidate the header and the loop stays
scalar at every profile and every `--target-cpu`. Hoisting `count := out.len()`
in source does not help, because the bounds check reloads `len` anyway: there is
no user-side workaround, which is exactly why this is a compiler promise and not
a style rule.

The TBAA node pair states a fact about Align's own representation — the header
lives in a parameter or local slot, the elements in a separately allocated
buffer — so it is outside the prohibition on facts minted from source syntax.
`noalias` on a `borrow` header slot asserts what Align's exclusivity rules
already guarantee.

Recorded negative result, so it is not retried: `noalias dereferenceable(16)
align 8` on the `borrow` header alone leaves the loop scalar, and so does adding
`memory(argmem: readwrite)` to the runtime calls. Header materialization or the
TBAA pair is required.

### G2 — one bounds check per loop

A length is non-negative by construction, so `(idx < 0) || (idx >= len)` is
`(u64)idx >= (u64)len`. The fusion is unconditionally correct and independent of
everything else in this contract. The elimination half is stated for induction
variables: for a loop exiting on `i >= N` with a positive constant step and an
index `a*i + b` with loop-invariant `a > 0`, one preheader guard replaces the
in-loop one.

The guard is moved, never deleted. The `(index, len)` diagnostic text stays
byte-identical and the first failing access still fails at the same iteration;
that is a promise, and it is what separates this from bounds-check removal.

`!range` on the length load (issue 1080) is part of G2's proof surface rather
than a separate guarantee: it is what makes the unsigned fusion provable to LLVM
as well as to us, and it also widens the two-length kernels from `<2 x float>`
to `<4 x float>`. §5 records why it is not the rejected `llvm.assume` policy.

### G3 — counted loops exit at the latch

Align has exactly one loop expression, and that is locked. The consequence is
that the one `loop` form must carry the canonical lowering itself: recognition
belongs in MIR's contract for `loop`, not in a peephole, and it must be total
for the canonical shape. "Did you write the loop the fast way?" must not be a
question a programmer can ask about the only loop the language has.

```align
fn first_nonzero(borrow xs: slice<u8>) -> i64 {
  mut i := 0
  loop {
    if i >= xs.len() { break -1 }
    if xs[i] != 0 { break i }
    i = i + 1
  }
}
```

The trip-count exit (`i >= xs.len()`) rotates to the latch; the data-dependent
exit (`xs[i] != 0`) stays in the header, where LLVM's early-exit vectorizer can
work with it. A `find`-style loop still returns the *first* match, and an exit
with a side effect still takes it at the same iteration. B11's in-loop early
exit is answered here and by G10, not by a library function: an exit test that
does not feed the reduction is a MIR transformation, and one that does is a
scalar loop the compiler must explain.

### G4 — one floating-point semantics model

Part 1 is a defect, not a feature. Three spellings of one operation have three
lowerings:

```align
fn max_cmp(borrow xs: slice<f32>) -> f32 {
  mut best: f32 := 0.0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    if xs[i] > best { best = xs[i] }
    i = i + 1
  }
  return best
}

fn max_mathfn(borrow xs: slice<f32>) -> f32 {
  mut best: f32 := 0.0
  mut i := 0
  loop {
    if i >= xs.len() { break }
    best = best.max(xs[i])
    i = i + 1
  }
  return best
}

fn max_pipe(borrow xs: slice<f32>) -> f32 = xs.max()
```

`max_mathfn` widens to `fmax.4s` with a closing `fmaxv.4s` and measures 12.0 µs
on 152,064 `f32`. `max_cmp` and `max_pipe` emit `fcmp ogt` plus `select` and
measure 191.0 and 190.7 µs. `llvm.maximum` is the deterministic IEEE 754-2019
operation the scalar path already chose for exactly the reason Align cares
about — identical results across builds and targets — so routing all three
through it changes no semantics. This is a "one way to do things" repair.

Part 2 is a genuinely missing surface and is deferred to a separate RFC (§4).
What this contract settles now is only its boundary, because the boundary is
what protects the locked decisions:

```text
default              strictly ordered, IEEE 754, no reassociation, no
                     contraction, NaN-propagating minimum/maximum
scoped relaxation    an explicit lexical scope naming which guarantee is
                     relinquished; reassoc and contract are named and
                     selected independently, never bundled as one "fast"
non-inheriting       a callee's semantics never change because of its caller
excluded forever     nnan and ninf: they make the result poison for a NaN or
                     infinite input, which is a hidden undefined-behaviour
                     class inside a language whose settled rules are that
                     floats follow IEEE 754 and never abort
outside any scope    bit-for-bit unchanged, and that is testable
```

The surface itself — attribute, block, spelling, level names — is not settled
here and must not be inferred from this paragraph.

### G5 — runtime primitives in loops

A loop containing an opaque call with unknown memory effects cannot be
vectorized, hoisted from, or turned into a `memset`. The guarantee is that no
runtime primitive reachable from ordinary loop code is opaque: each has a
complete effects record, and each per-element primitive has an inline fast path
with the slow path still visible in source terms. The `Result`/`?` failure edge
is part of the same promise — a cold-path model with branch weights and a
`cold` fail family — because an unweighted failure edge in the loop body is
itself a vectorization blocker.

### G6 — `core.math` vector lowering and accuracy

Stated in §6.1, because it is also the rewritten acceptance criterion for 1063.

### G7 — one checked, order-explicit, zero-copy byte view

Stated in §6.2, because it is also the rewritten acceptance criterion for 1064.

### G8 — masks are structural

A mask has no element type at the machine level: every `maskN<T>` is
`<N x i1>`, and a lane-wise blend depends only on lane count and lane width.
Codegen already ignores the element type; the restriction is entirely a
front-end one and is stricter than both the IR and the machine.

The cost of the restriction is that the correct spelling of an entire family of
masked-index algorithms — argmax, argmin, top-k, first and last match, index
compaction, histogram bucketing — does not compile, while the accepted
workaround keeps indices in `f32` and is exact only below 2^24. A language
should not reject the correct program and accept the approximate one.

The relaxation is a pure widening: every program that compiles today still
compiles with identical IR. The rule is stated once and applies uniformly to
`select`, to any future masked load or store, and to masked pipeline stages,
rather than being special-cased where `select` is checked.

### G9 — inlinable across units

A `pub fn` called from a pipeline stage stays a call across a unit boundary
while a generic one inlines, so the same helper vectorizes or not depending on
whether it happens to have a type parameter. The fix is to carry small
non-generic bodies in the unit interface exactly as generic bodies already
travel. Default ThinLTO is not the fix: it is blocked by the prelink defect
(1070), it is a whole-program hammer for a per-function question, and it does
not make the behaviour predictable at the default profile.

### G10 — the compiler says why

G1–G9 remove structural blockers. They do not make every loop vectorize, and
they must not pretend to. What closes the contract is that the remaining
decision is visible: the vectorizer remarks `explain-opt` already collects are
promoted to a first-class diagnostic attached to the loop, so a loop that stays
scalar names its blocker and a regression is detectable without a binary audit.
Without G10 this contract is a list of hopes; with it, it is checkable.

## 4. Ownership map

```text
plan 69 loop facts (planned)          G1  1079, 1080
                                      G2  1081
                                      G3  1084
plan 70 runtime boundary effects
  (planned)                           G5  1071, 1072, 1073, 1074
in progress, parallel PR              G4 Part 1  1082 Part 1
                                      G8  1083
future RFC, surface not settled       G4 Part 2  1082 Part 2
revised by this document              G6  1063 (§6.1)
                                      G7  1064 (§6.2)
unfiled                               G9  1066 comment, needs its own issue
                                      G10 needs its own issue against
                                          09-explain-opt.md's owner
```

Plan 69 and plan 70 do not exist yet; they are cited here as the planned owning
documents so that the guarantees have a named destination, not as existing
sources of truth. Issues 1079, 1080, 1081 and 1084 record the conjunction
evidence that makes them one unit of work rather than four patches: two of the
three data facts in isolation produce zero vector instructions on a real kernel.

G4 Part 1 and G8 are being implemented in a parallel PR at the time of writing.
Neither changes a public contract stated here: Part 1 unifies a lowering and G8
widens a type-check predicate.

G4 Part 2 is a separate future RFC. It introduces a new source annotation for
scoped reassociation and contraction, which is a language surface addition and
must go through the normal design gate on its own evidence. This document
records its direction and its permanent exclusions (§3, G4) and settles nothing
about its spelling.

## 5. Compliance with the locked decisions

This contract adds no exception to any locked decision. The four that it comes
closest to are recorded explicitly.

**Floats follow IEEE 754 and never abort.** G4 Part 1 changes no float
semantics at all: `llvm.minimum`/`llvm.maximum` propagate NaN and order ±0
deterministically, which is what the scalar path already chose. G4 Part 2's
excluded flags are excluded permanently and for this reason, not deferred to a
later level.

**No `llvm.assume` as a general policy.** `docs/open-questions.md:4187-4189`
rejects "`llvm.assume` / early intrinsic emission / loop-metadata overrides as a
general policy", with the guidance "attributes and flags first". Nothing in
G1–G10 reopens that.

- `!range` on a length load (1080, inside G2) **is not** that policy. It is
  per-load metadata, carries no control dependence, costs no instruction, and
  cannot be hoisted to a place where it asserts something false. It states the
  language's own capacity limit on a load Align itself emits. It is precisely an
  "attributes and flags first" mechanism, and it is additive: removing it changes
  nothing but the achievable vector width.
- G1's TBAA pair and G1/G3's `dereferenceable` attributes are likewise
  attributes describing Align's own data layout.

**1084's dereferenceable fallback needs an explicit carve-out.** The preferred
form is an attribute on the data pointer once G1 has materialized it in the
preheader, which needs no carve-out. The documented fallback — one operand-bundle
`assume` in the preheader, emitted only for a recognized counted loop over a
slice and describing only Align's own `{ptr, len}` invariant — is a narrow,
recognized-shape use of the rejected intrinsic. It may not be implemented under
this contract alone. It requires a recorded carve-out in the Settled section of
`docs/open-questions.md` naming the exact shape, the exact emission condition,
and the reason the general prohibition still stands, added by the PR that needs
it. Absent that carve-out, G3's dereferenceability half ships in its attribute
form or not at all. Recorded negative result: `dereferenceable(16) align 8` on
the `borrow` *header* parameter does not reach the data buffer.

**One loop expression; no `for`, `while`, `continue`, labels.** G3 is a lowering
contract, not a syntax addition. No guarantee in this document proposes new loop
syntax, and a range loop is explicitly not the fix for B3.

**No facts minted from source spelling.** Every fact stated to LLVM by G1, G2,
G3 and G7 is a property of Align's own representation: the header/element split,
the non-negative length, the `len * sizeof(T)` buffer extent, the exclusivity of
a `borrow`. None is derived from how the program was written.

**No per-algorithm intrinsics.** G7 exists so that `argmax`, `argmin`, `top_2`,
`dot`, `variance`, `norm` and their successors are ordinary Align code rather
than compiler builtins, and G8 exists so the vectorized spelling of that code
type-checks. Adding a terminal per algorithm is rejected for the same reason a
terminal per relaxation level is rejected in G4.

## 6. Rewritten acceptance criteria

Both issues were filed with an acceptance criterion that is measured false and
does not become true through the change the issue proposes, because it depends
on G1–G4. Both are restated here as criteria the owning change can actually
satisfy on its own.

### 6.1 Issue 1063 — `core.math` exponential and logarithmic family

The issue's criterion "loop vectorizer successfully vectorizes loops over slices
using these operations" is unreachable by adding intrinsics. Worse, with no
vector implementation configured, LLVM 22 turns an `llvm.exp.v2f32` loop into
per-lane `bl _expf` with lane insert and extract overhead — slower than scalar.
`examples/vec_math.align` already records this for `pow`.

The real contract has two parts, and neither depends on the loop-facts work.

**Accuracy.** Every `core.math` function has one documented ULP bound. The
scalar lowering, the vector lowering, and every supported target produce results
within that bound of the correctly rounded result, and the scalar and vector
lowerings of the same function agree with each other. A program's results do not
change because the vectorizer fired or because the target changed. Align already
pays for this class of determinism: `llvm.maximum` was chosen over `maxnum` for
identical-across-builds results.

**Vector lowering on every target.** A vector form exists on every supported
target, from Align-owned portable kernels shipped as runtime bitcode — the
mechanism `--rt-lto` already uses for four string primitives. This is
target-independent, bit-identical across targets, inlinable into the caller's
loop, and adds no link dependency. A per-target vector library
(`Darwin_libsystem_m`, libmvec, SLEEF, SVML) is an explicit opt-in for users who
accept non-identical results, never the default. The portable kernel is the
contract; the library is the escape hatch.

```align
fn softmax_weight(v: vec4<f32>) -> vec4<f32> = v.exp()
```

Acceptance criteria that replace the issue's current ones:

1. For each shipped function, the explicit vector receiver (`vecN<f32>`,
   `vecN<f64>`) emits a vector body containing **no scalar libcall and no
   per-lane insert/extract** on aarch64 and x86-64 at the default target. This
   is pinned on the explicit SIMD surface, which is reachable today, instead of
   on auto-vectorization of a slice loop, which is G1–G3's promise.
2. Scalar and vector results agree within the documented ULP bound, and both
   agree across targets. A conformance owner checks each function against a
   reference at the bound, including the sub-normal and infinity edges; floats
   never abort, so every input has a defined result.
3. `examples/vec_math.align`'s "`pow` is a libcall, so it stays scalar" caveat
   is deleted, because `pow` is covered by the same policy.
4. The auto-vectorization of an ordinary `slice<f32>` loop over these functions
   is **not** a criterion of 1063. It moves to the umbrella corpus (§7) and is
   gated on G1, G2 and G3.
5. The function set is defined by IEEE 754-2019 §9.2's recommended elementary
   functions, adopted as the closed `core.math` surface and shipped in stated
   tiers, so that the next client need does not reopen the set. exp/log first is
   fine; the closed list is what stops the next single-function issue.

### 6.2 Issue 1064 — typed slice reinterpretation views

The issue's criterion "normal loops over the resulting `slice<f32>` auto-
vectorize cleanly" is measured false: argmax over an already-typed `slice<f32>`
is 190.4 µs against 190.6 µs over bytes, a 0.1 % difference. The view is a
prerequisite, not the cause. What the view *does* remove is real and large: the
explicit conversion loop it replaces costs 59.4 µs per 152k `f32`, and the view
makes that zero.

**Answering plan 62's three recorded rejections.** `62-decode-optimization-plan.md`
rejected `as_f32_le()` for alignment, byte order and access authority. Each has
an answer, and the answers are the view's contract:

```text
alignment        checked at runtime; a misaligned or wrongly sized view yields
                 None, never a trap and never a silent reinterpretation.
                 buffer and array payloads are malloc-backed and 16-aligned, so
                 a view at offset 0 always succeeds; a sub-slice is checked.
byte order       a view is a native-order reinterpretation by definition, and
                 the spec's rule that every multi-byte access names its order
                 is kept, not weakened: the order is named in the view, and the
                 compiler rejects a view whose named order is not the target's
                 native order, with a diagnostic that says to decode instead.
                 The wire format stays explicit and nothing is hidden.
access authority the view carries the source borrow's authority. A slice<u8>
                 yields a read-only slice<T>; a mut slice<u8> yields a writable
                 one. Plan 62's concern was raw and foreign memory, which stays
                 with resource.view_from_raw (plan 61). Two routes, one rule
                 each: Align-owned bytes take the checked view, foreign memory
                 takes the owner-published typed view.
```

**Recommended shape: one generic operation, not a method per type.** The issue's
`as_f32_slice`/`as_i32_slice`/`as_f64_slice`/`as_i64_slice` repeats the same
per-width enumeration the `<ty>_le`/`<ty>_be` accessor family already carries
across 36 methods, and G7's whole point is to make the view the one primitive
that those accessors are then defined on top of.

```align
fn peak(borrow raw: slice<u8>) -> f32 {
  view := raw.view_le<f32>() else { return 0.0 }
  mut best: f32 := 0.0
  mut i := 0
  loop {
    if i >= view.len() { break }
    best = best.max(view[i])
    i = i + 1
  }
  return best
}
```

Public surface of the recommended shape:

| Operation | Signature | Errors | Ownership | Allocation |
| --- | --- | --- | --- | --- |
| checked view | `slice<u8>.view_le<T>() -> Option<slice<T>>` for the closed scalar set `u16..u64`, `i16..i64`, `f32`, `f64` | `None` on a misaligned pointer or a length that is not a whole multiple of `sizeof(T)`; compile-time rejection when the named order is not native | borrows the same region as the source, with the source's authority and provenance; a `mut` receiver yields a writable view | none; zero copy |
| inverse | `slice<T>.as_bytes() -> slice<u8>` | total: a typed slice is always aligned and always a whole multiple | same region, same authority | none; zero copy |

Acceptance criteria that replace the issue's current ones:

1. View construction emits no allocation and no copy; an owner test asserts the
   absence of both in the emitted IR.
2. A misaligned pointer or a non-multiple length yields `None`. It does not trap
   and it does not produce a shortened view.
3. The view carries the source's authority: a read-only source cannot produce a
   writable view, and a writable view's stores are checked against the source's
   exclusivity exactly as a direct store would be.
4. A view whose named byte order is not the target's native order is rejected at
   compile time with the "decode instead" diagnostic; a negative owner pins it.
5. `vecN<T>` load and store are reachable from `buffer.bytes()` through the
   view, so the explicit SIMD route from byte storage exists end to end.
6. `slice<T>.as_bytes()` round-trips: viewing the bytes of a typed slice yields
   the original slice.
7. Auto-vectorization of an ordinary loop over the resulting `slice<f32>` is
   **not** a criterion of 1064. It belongs to G1, G2 and G4 and moves to the
   umbrella corpus (§7).

**Open on 1064, not settled here.** These are design questions the issue must
close before implementation; this document records the recommended shape, not
the final surface.

```text
spelling         raw.view_le<f32>() versus an annotation-driven
                 view: slice<f32> := raw.view_le()
_be at all       a _be view on a little-endian target is always a compile
                 error; is the name worth having for the sake of keeping
                 "every multi-byte access names its order" total?
accessor fate    are the 36 <ty>_le/<ty>_be accessors respecified as
                 "view then index" now, later, or removed?
byte elements    are u8/i8 views included, where the order is meaningless?
partial length   fail with None (recommended) or truncate to the whole prefix?
provenance       how the view participates in plan 52's read-only provenance
                 retention and plan 57's validated-text observation
foreign route    whether the checked view and resource.view_from_raw stay two
                 routes permanently or unify once plan 61 lands
```

## 7. Acceptance corpus

The contract is proved as a whole by one corpus, extending
`crates/align_driver/tests/vectorize_shapes.rs`, run on aarch64 and x86-64 at
the default target:

```text
one positive owner per guarantee   the named owners in §3
the two conjunction pins           bytes_to_f32_out vectorizes only with G1
                                   and G2 both present; a regression in either
                                   half fails the same test
the standing negative control      k6_float_sum_does_not_vectorize_without_
                                   fast_math keeps passing unchanged for plain
                                   f32 +, outside any G4 Part 2 scope
the trap-parity owners             zero-length, length-1 and first-out-of-range
                                   accesses trap at the same iteration with
                                   byte-identical (index, len) text under G2
                                   and G3
the diagnostic owner               a known-scalar loop names its blocker (G10)
the corrected example              examples/vec_simd.align's claim that the
                                   pipeline already vectorizes the same way is
                                   false for max/min today and is corrected
                                   when G4 Part 1 lands
```

Benchmarks are separate local measurements for the performance claims each
owner issue makes. They are not correctness gates, and no guarantee in §3 is
stated as a wall-clock number.

## 8. Author-side consistency pass

Recorded per the large-design authoring gate.

- Every normative promise in §3's prose appears in the ledger table, and every
  ledger cell has a stated owner, corpus entry and issue.
- Every guarantee separates promise from try, and no guarantee is stated as a
  target-specific vector width or a wall-clock number.
- Every public surface introduced here (G6's accuracy contract, G7's view and
  its inverse) has a signature, an error behaviour, an ownership and provenance
  rule, and an allocation rule, in §6.
- Every normative example is `loop`-only, expression-oriented, newline-
  terminated and uses only surfaces that exist today, except `view_le<T>()`,
  which is marked as the recommended, unsettled shape of G7.
- No guarantee consumes a decision scheduled for a later milestone: G4 Part 2's
  surface is explicitly deferred and nothing in G1–G3 or G6–G10 depends on it.
- The two locked decisions nearest this work — IEEE 754 float semantics and the
  rejection of `llvm.assume` as a general policy — are checked explicitly in §5,
  including the one carve-out G3's fallback would require.
- `docs/open-questions.md:5960` already lists opt-in fast-math flags as a future
  item and `:4956` already settles that ordered float reduction must not be
  silently reassociated, so G4 Part 2's direction is pre-approved and only its
  surface is missing. No Settled entry changes as a result of this document.
