This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — hash (+ scalar math)

> 🌐 **English** · [Japanese](./ja/hash.md)

## Overview

Non-cryptographic hashing over byte views (draft §18.1): one canonical mixer (wyhash, the
`align_hash` crate), one free-function surface. There is deliberately **no `Hash` trait** — you
hash bytes, not arbitrary values; a value's byte view is the caller's explicit business. This
file also records the scalar-math method surface (small enough to not warrant its own file).

## Signatures (verified)

```text
hash64(data)   -> u64            // data: str | string (auto-borrowed, not consumed) | slice<u8>
hash128(data)  -> (u64, u64)     // .0 == hash64(data); .1 = decorrelated second lane (no u128 type)

// scalar math — intrinsic methods on numeric values (no import, no core.math module):
x.abs()                          // signed int / float; identity on unsigned
a.min(b) / a.max(b)              // pairwise; int = llvm.{s,u}min/max, float = NaN-propagating minimum/maximum
x.sqrt() / .floor() / .ceil() / .round() / .trunc()   // float-only; round = ties away from zero
b.pow(e)                         // float-only
fma(a, b, c)                     // free builtin; float scalar or float vector; one rounding
```

## Type & ownership classification

All Copy in, Copy out. `hash*` borrows its view (an owned `string` remains usable — the `print`
precedent). Pairwise `a.min(b)` coexists with the array-reduction `arr.min()` by arity.

## Effects

Pure. Hash results are deterministic for a given input **within a build** (fixed seed) — safe
in `par_map`.

## Errors & aborts

None. Non-byte-view `hash64(5)` and wrong arity are type errors; `sqrt` on an int is rejected;
unsigned `.abs()` is a defined identity, not an error.

## Regions

None — values in, values out.

## Spec'd but not implemented

- **Not part of the contract:** hash output stability across *builds/versions* — wyhash with a
  fixed seed happens to be stable today, but §18.1 explicitly scopes it as "not a stable
  on-disk/wire format". Do not let tests or users pin exact hash values as API (the driver
  tests pin *properties*: determinism, lane equality, view acceptance).
- Not DoS-resistant, not cryptographic — `std.crypto` (M11, designed in `../std-design/
  crypto.md`) is the answer for security contexts; refuse "just use hash64" shortcuts there.
- No transcendentals (`sin`/`cos`/`log`/`exp`) — the `MathFn` enum stops at `pow`/`fma`.
  Adding them is small mechanically but decide the precision/libm-dependence stance once, in
  `open-questions.md`, not per-function.
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
