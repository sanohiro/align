This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — arena / heap.new / box

> 🌐 **English** · [Japanese](./ja/arena-heap.md)

## Overview

The two visible allocation homes (draft §6): `arena {}` — the batch lifetime — and `heap.new` —
the single explicit allocation, arena-resident. Everything else in the memory model (Move types,
regions, drops) is language, not library; this file covers the library-shaped surface and its
current, deliberately tight, limits.

## Signatures (verified)

```text
arena { … }                 // expression block; all arena allocations freed at }, O(1)
heap.new(x)   -> box<T>     // ONE arg; must be inside an arena {}; T = primitive scalar only
b.get()       -> T          // copy the payload out
b.clone()     -> box<T>     // deep-copy the box; both remain valid
```

That is the entire box surface — no `.set()`, no deref operator.

## Type & ownership classification

- `box<T>` is region-tracked `Arena(depth)` data. It is **not** a general owner: it cannot be a
  function parameter or return type ("boxes are arena-local in M3" / "would escape its arena"),
  cannot be an array/slice element, cannot be an Option/Result payload.
- Payload whitelist: int/float/bool/char. Rejected with specific diagnostics: owned Move values
  ("an owned `…` cannot be boxed"), structs, sum types, `str` views (`box<str>` also rejected at
  type-resolution).
- `arena` is an expression: its trailing value escapes only if region-free (or cloned out).

## Effects

Pure. Arena allocation is a bump; nothing here touches the OS (the arena's backing pages come
from the runtime allocator with verified `noalias`/`nounwind` attributes, #301).

## Errors & aborts

None at runtime (allocation failure is the process-level abort path shared with all runtime
allocation). Everything else is a compile error: `heap.new` outside an arena, escaping
arena-regioned values (`cannot return a value allocated in an arena`), boxing a non-scalar.

## Regions

The reference implementation of the region model: `region_of(box) = Arena(depth)`;
`region_of(b.get()) = none` (scalar copy). The arena double-free class was closed in the
2026-07-02 audit (#270–#277 arc) — cleanup runs exactly once per arena on every exit path.

## Spec'd but not implemented

- **Struct / sum-type / owned payloads** in a box; boxes as params/returns/fields. The M3 cut is
  scalars, arena-local; widening the payload set is real design work (drop of boxed owned
  values, region interaction with Move) — not a mechanical extension.
- A box `.set()` / mutation surface — no use case has demanded it; the box today is "compute
  once, read locally". Growing it toward a general heap cell would need a One-way review
  against `mut` locals and arena values, which already cover the patterns.
- Escaping boxes (a box that outlives its arena / a global heap tier) — deliberately absent;
  the ownership model's answer to "longer-lived" is Move types, not box lifetimes.

## Pitfalls

- P1 — the **unnecessary-heap lint** (#323) fires on a box that is only ever `.get()`-read and
  never escapes; the guide teaches that lint as the norm. If a change makes the lint fire on
  idiomatic code, the change is wrong, not the lint.
- P2 — `heap.new(x).get()` as an unbound chain works only because the annotation flows through
  (`v: i32 := heap.new(7).get()` is a pinned test); do not generalize unbound Move-temporary
  patterns from it — box is region-tracked, not Move, which is *why* it composes.
- P3 — arena blocks nest; region depth comparisons, not block identity, drive escape checks.
  New constructs that open scopes (e.g. future `soa` builders) must integrate with depth, not
  add a parallel mechanism.
- P4 — do not route new library allocations around the arena (hidden `malloc` in a runtime
  helper): every runtime allocation on behalf of user code belongs to an arena, an owner, or a
  builder — the "nothing hidden" audit surface depends on it.

## Test anchors

`m3.rs` (box construct/get/clone, annotation flow-through, arena requirement);
`enum_match.rs:210` (`heap.new(C.R)` rejected, not panicking); escape/region tests across
`mmv2.rs` + the #270–#277 audit-fix suite (arena double-free, exit paths);
`lint_unnecessary_heap.rs`; examples `arena.align` (exits 42), guide ch05.
