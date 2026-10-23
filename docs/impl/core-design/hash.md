This directory records the per-area design of the `core` library: signatures,
ownership and effect classification, error policy, pitfalls, and related tests.

# core — hash (+ scalar math)

> 🌐 **English** · [Japanese](./ja/hash.md)

## Overview

Non-cryptographic hashing over byte views (draft §18.1) uses wyhash from the
`align_hash` crate. Callers supply an explicit byte view; there is no `Hash` trait
for arbitrary values. This file also records the scalar-math methods.

## Signatures (verified)

```text
hash64(data)   -> u64            // data: str | string (auto-borrowed, not consumed) | slice<u8>
hash128(data)  -> (u64, u64)     // .0 == hash64(data); .1 = decorrelated second lane (no u128 type)

// scalar math — intrinsic methods on numeric values (no import, no core.math module):
x.abs()                          // signed int / float; identity on unsigned
a.min(b) / a.max(b)              // pairwise; int = llvm.{s,u}min/max, float = NaN-propagating minimum/maximum
x.sqrt() / .floor() / .ceil() / .round() / .round_even() / .trunc()   // float-only; round = ties away from zero
x.exp() / .exp2() / .log() / .log2() / .log10()     // float-only; also lane-wise on float vectors
x.expm1() / .log1p() / .sin() / .cos(), y.atan2(x) // same float scalar/vector type; atan2 takes y then x
b.pow(e)                         // float-only
fma(a, b, c)                     // free builtin; float scalar or float vector; one rounding
```

## Type & ownership classification

All Copy in, Copy out. `hash*` borrows its view (an owned `string` remains usable — the `print`
precedent). Pairwise `a.min(b)` coexists with the array-reduction `arr.min()` by arity.

`round_even()` is a zero-argument method on `f32`, `f64` and their float vectors.
It returns the same type, rounding to the nearest integral floating value with
ties to the even integer. It preserves signed zero (including `-0.5 -> -0.0`),
infinities and integral finite values. NaN remains NaN; its payload, sign and
quieting are unspecified. It never aborts or allocates. `round()` continues to
round ties away from zero. Vector semantics apply independently to every lane.

## Effects

Pure. Hash results are deterministic for a given input **within a build** (fixed seed) — safe
in `par_map`.

## Errors & aborts

None. Non-byte-view `hash64(5)` and wrong arity are type errors; `sqrt` on an int is rejected;
unsigned `.abs()` is a defined identity, not an error.

## Regions

None — values in, values out.

## Availability and limits

- **Not part of the contract:** hash output stability across *builds/versions* — wyhash with a
  fixed seed happens to be stable today, but §18.1 explicitly scopes it as "not a stable
  on-disk/wire format". Do not let tests or users pin exact hash values as API (the driver
  tests pin *properties*: determinism, lane equality, view acceptance).
- Not DoS-resistant, not cryptographic — the shipped
  [`std.crypto`](../std-design/crypto.md) supplies the cryptographic operations; refuse
  "just use hash64" shortcuts in security contexts.
- Elementary `exp`/`exp2`/`log`/`log2`/`log10` shipped together under plan 75:
  direct LLVM scalar/vector intrinsics, no ULP or
  cross-target bit-identity promise, visibility of pre-instruction-selection
  scalarization, and a warning that retained provider-less IR may still
  scalarize later. Final machine SIMD requires object inspection.
- `expm1`/`log1p` use width-exact scalar libm calls, including per-lane vector
  fallback ([Plan234](../234-cancellation-sensitive-math.md)). `sin`/`cos`/`atan2`
  use LLVM scalar/vector intrinsics ([Plan235](../235-trigonometric-math.md)).
  All are Pure, allocate no Align storage, and retain the target math accuracy
  and vectorization limits above. `atan2` requires exactly matching operand
  types, with no scalar broadcast, and returns radians in [-pi, pi].
- `core.bitset` (§18.1 neighbor) — not implemented; bit work uses integer operators. Its layout
  question is tied to packed-bool soa columns; settle together.

## Pitfalls

- P1 — the internal wyhash (group_by interning, dict_encode, JSON PHF — #321 converged them on
  `align_hash`) and the user-facing `hash64` are the **same mixer on purpose**: structural
  behavior (e.g. the PHF byte-match) must never depend on a *different* hash than the one users
  can compute. Change the mixer in one place or not at all.
- P2 — `hash128.0 == hash64` is a pinned property; if a future mixer breaks the lane
  relationship, that is an API break, not an internal detail.
- P3 — float `min`/`max` are NaN-*propagating* (`llvm.minimum/maximum`, not `minnum/maxnum`);
  vec lane `min`/`max` must keep matching the scalar choice — lane/scalar divergence is the bug
  class the differential fuzzer exists for.

## Test anchors

`crates/align_driver/tests/hash.rs` (determinism, lane equality, owned-string borrow, non-view
rejection, arity); `scalar_math.rs` (abs/min/max int+float+unsigned-identity, pairwise-vs-
reduction coexistence, transcendental values, sqrt-on-int rejection); #321 (hash convergence +
group_by speedup pins).
