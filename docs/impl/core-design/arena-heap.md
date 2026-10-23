# core — arena / heap.new / box

> 🌐 **English** · [Japanese](./ja/arena-heap.md)

## Overview

This file covers `arena {}`, scalar boxes allocated by `heap.new`, and their
region and escape rules (draft §6). An arena groups allocations under one
lifetime. `heap.new` allocates one scalar box within that arena. Ordinary owned
strings, arrays and buffers retain their own allocation and Drop rules.

## Signatures (verified)

```text
arena { … }                 // expression block; release its allocations and mappings at }
heap.new(x)   -> box<T>     // ONE arg; must be inside an arena {}; T = primitive scalar only
b.get()       -> T          // copy the payload out
b.clone()     -> box<T>     // deep-copy the box; both remain valid
```

`out` is contextual: `out: region` names the capability parameter, while `out dst: slice<T>` is an
out-mode parameter. Parsing uses token lookahead, not whitespace.

Arena exit releases each backing chunk and registered file mapping. It does not
free each bump-allocated value separately, but its work grows with the number of
chunks and mappings; cleanup is not generally O(1).

Boxes have no `.set()` method or dereference operator.

Named-region form (required L4, **shipped**):

```text
arena out { … }             // binds the same arena as `out: region`
fn f(out: region)           // the same arena, substituted through ordinary calls
v.clone_in(out)             // copy a view into the named arena
```

The named arena form does not widen `box<T>`:
`out` is a scope-limited allocation-destination capability for ordinary library functions, and
anonymous/named arenas share the same begin/end and bulk cleanup. Full contract:
[`../17-library-boundary-prerequisites.md`](../17-library-boundary-prerequisites.md) §4.

## Type & ownership classification

- `box<T>` is region-tracked `Arena(depth)` data. It is **not** a general owner: it cannot be a
  function parameter or return type ("boxes are arena-local in M3" / "would escape its arena"),
  cannot be an array/slice element, cannot be an Option/Result payload.
- Payload whitelist: int/float/bool/char. Rejected with specific diagnostics: owned Move values
  ("an owned `…` cannot be boxed"), structs, sum types, `str` views (`box<str>` also rejected at
  type-resolution).
- `arena` is an expression: its trailing value escapes only if region-free (or cloned out).

## Effects

Pure. Arena allocation and cleanup are memory-management operations, not
language-level I/O effects. The runtime allocator supplies backing chunks;
individual allocations normally advance a bump cursor within a chunk.

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

(Named `region` is no longer in this list: `arena out {}`, `out: region` parameters, exact-arena
substitution through ordinary calls, escape/storage/task/FFI rejection, and `clone_in(out)` are
all shipped — F-B region materialization is complete through L4 and L6.)

## Pitfalls

- P1 — the **unnecessary-heap lint** (#323) flags a box that is only `.get()`-read
  and never escapes. Its suggested replacement uses the scalar value directly;
  preserve those cases when changing box lowering.
- P2 — `heap.new(x).get()` is arena-regioned, not a Move synthetic-owner case
  (`v: i32 := heap.new(7).get()` is a pinned test). General unbound Move cleanup now also composes,
  but through path-local ownership flags; do not merge the two lifetime mechanisms.
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
