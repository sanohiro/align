# 08 — Memory Model v2: borrow regions + owned heap/drop

Status: **design (pre-implementation).** This is the foundation phase slotted **after the
foundation-safe scalar work and before M6 (SIMD)** (`07-roadmap.md`, `open-questions.md`
"Memory model v2"). It is designed **as a whole here first**, then implemented in ordered
slices (§11). Nothing in M4/M5 that depends on it ships in a corner-cut form before it lands.

It exists because the deferred "ideal forms" of M4 and M5 both rest on one foundation:
- M5 `json.decode` for `str` / `array<T>` / nested fields → zero-copy views region-tied to
  the input → **draft.md §19 runs in full** (true M5 completion).
- M4 carryover `filter` / `scan` / `partition` / `sort` / `chunks` + array-valued results →
  built on owned, dynamic heap arrays with drop (materialization).

Today these are handled by three unrelated **point solutions** in `align_sema`'s
`EscapeCheck` (see `lib.rs`):
1. **arena depth** — a `box` / arena-backed `str`'s region is the `arena {}` nesting depth at
   which it was allocated; escaping to a shallower depth is an error (`region_of`,
   `region`/`decl_depth` maps, `arena_depth`).
2. **"local-backed" slices** — a `slice<T>` that views a frame-local array is tracked in a
   `local_backed_slice` set and may not be returned (`slice_is_local`).
3. **struct `str` region-0** — a struct field may only hold a *region-0* (literal / non-arena)
   `str`, so a struct never carries an arena region and stays freely returnable.

v2 **generalizes these three into one region model** and adds owned heap collections + drop.

---

## 1. Core invariants this must respect

(From `CLAUDE.md` / `design-notes.md` — non-negotiable.)

- **No visible lifetimes.** Regions are *inferred*, never written in source. There is no
  `'a`-style annotation, ever. This is the hard constraint that shapes the whole design:
  the region system is an analysis, not a surface type parameter.
- **Nothing hidden.** Allocation must be visible in source. The compiler never silently
  inserts a heap allocation or a copy the user did not write. (This decides §9.)
- **Predictable performance.** An abstraction must not silently change cost class. A small
  source edit must not turn a zero-copy borrow into a hidden full copy.
- **One ownership model.** value / arena / explicit heap — no new keyword, ownership stays a
  property of the type (`array`/`string`/`buffer`/heap = Move; primitives/small structs/
  `slice` = Copy).
- **Compiler-friendly by restriction.** The region lattice is total and the rule is a single
  comparison, so the checker stays simple and the optimizer keeps no-alias / contiguous /
  arena-lifetime facts.

---

## 2. The region lattice (the unifying core)

Every value has an inferred **region** drawn from a total order (outer outlives inner):

```text
Static  ⊐  Frame  ⊐  Arena(1)  ⊐  Arena(2)  ⊐  …  ⊐  Arena(d)
longest-lived                                         shortest-lived
```

- **Static** — process / program lifetime. String literals, leaked allocations, values built
  from only owned + scalar data. Freely returnable from any function.
- **Frame** — the current function's stack frame. A view *created inside this function* over
  frame-local storage: a `slice` of a frame-local array literal, or a view into the interior
  of a by-value (owned) parameter. Lives until the function returns; **cannot** be returned.
  Note this is **not** a view *parameter*: a `slice`/`str` *parameter* borrows the **caller**
  (which outlives this call), so within this function's analysis it is treated as returnable —
  region `Static` (see §3). This matches today's `slice_is_local`, which never marks a slice
  parameter local-backed.
- **Arena(k)** — the `k`-th enclosing `arena {}` block (1 = outermost arena, larger = more
  nested = shorter-lived). Freed at that block's `}`.

Representation: a single `Region` value per binding/expression. Internally an integer is
enough — `Static = 0`, `Frame = 1`, `Arena(k) = 1 + k` — but the checker reasons in terms of
the named lattice. The ordering test is `outlives(a, b)  ⟺  a ≤ b` (smaller = longer-lived).

This **subsumes** all three point solutions:

| today                              | v2                                            |
|------------------------------------|-----------------------------------------------|
| arena depth `d` for box/str        | `Arena(d)`                                     |
| "local-backed" slice               | `Frame`                                        |
| struct `str` must be region-0      | a struct's region = max of its fields (Static if none borrow) |

There is exactly **one** escape rule (§4), replacing the three bespoke checks.

---

## 3. Region inference & propagation

A value's region is computed bottom-up (generalizing today's `region_of`). The rule for a
*view* (a value that borrows another): **its region = the maximum (shortest-lived) region of
its sources** — a borrow can never outlive what it borrows.

```text
literal / leaked / owned-from-scalars     → Static
view parameter (slice/str/… param)        → Static within this fn  (borrows the caller; returnable)
heap.new / clone / template / str-concat  → region of the enclosing arena (Static if none*)
slice of a frame-local array literal      → Frame
slice/view into a by-value param interior → Frame
slice of an arena-allocated array         → that Arena(k)
x.field   (field is a view)               → region(x)
view[i] / project(.field) on a view       → region(view)
array literal of views [v0, v1, …]        → max region over the elements
index/project an array-of-views  arr[i]   → region of the array's elements
f(args)  returning a view                 → max region over the view-typed args it reborrows
if/block/match yielding a view            → max region over the yielded branches
struct literal { … }                      → max region over its fields  (Static if all owned/scalar)
decoded str/array field                   → region of the decode input  (§9)
```

\* `heap.new`/`clone`/`template`/concat outside any arena: the result is leaked /
process-lifetime today, so `Static`. The owned-collection case is different — see §6: a
free-standing **owned** `array`/`string` is heap-owned and `Static`-lived *until its `Drop`*
(not `Frame`). `Frame` is only for **borrows** of frame-local storage (a slice of a
frame-local array literal, or a view into a by-value parameter's interior), never for a
view *parameter* (which borrows the caller → `Static`, returnable).

Regions are never written by the user and never appear in a type. They live only in the
checker (an inferred property of each binding), exactly like today's `region` map — just
generalized to every view-producing expression and to structs.

---

## 4. Escape checking (one rule)

> A value of region `r` may only be **stored into**, **assigned to**, or **returned to** a
> location whose region `r_dst` it outlives: `outlives(r, r_dst)`, i.e. `r ≤ r_dst`.

Checked at exactly the places the current `EscapeCheck::stmt` already visits, now uniformly:

- **`return e`** — `r_dst = Static` (the caller outlives every callee region). So only
  `Static` (and trivially-owned) values may be returned. A `Frame` view or an `Arena(k)`
  view cannot be returned — same outcome as today, one rule.
- **`let`/`Assign` to an outer binding** — `r_dst = region(target binding's declaration
  depth)`. Reassigning a deeper-region value into a shallower binding is an error.
- **`AssignField`** — `r_dst = region(base struct)`. Storing a deeper view into a
  longer-lived struct is an error (generalizes the current arena-str-into-field check to all
  view fields).
- **arena block value** — the value an `arena {}` yields is region-checked against the
  enclosing region (it must not be `Arena(inner)`).

Diagnostics keep the current human-readable phrasing ("…cannot escape its arena", "…views a
local array…"); the *machinery* behind them is now the single lattice comparison.

---

## 5. Structs carry a region

The current "struct `str` must be region-0" restriction is replaced by: **a struct instance's
region = the max region of its fields.**

- A struct of only scalar / owned fields → `Static` → freely returnable (unchanged from
  today's common case).
- A struct holding a borrowing field (a `str` view, a `slice`, a decoded view) inherits that
  field's region and is escape-checked like any other view.

This is what makes zero-copy decode of `array<User>` (where `User` has a `str name` borrowing
the input) expressible without forcing a copy. Struct fields may now be `str` views, `slice`,
`box`, and owned `array`/`string` (lifting the M5 "region-0 str only / no box-slice-array
fields" cut), each contributing its region to the whole.

---

## 6. Owned heap collections + Move

Per draft.md §7 (`array<T>` = owned contiguous memory) and §12 (`string` = owned). These are
**Move** types (use-after-move via the existing `MoveCheck`), the M3-deferred piece.

**Decision (locked): both allocation modes.**

- **Inside an `arena {}`** — an owned `array`/`string` is bump-allocated in the arena and
  **bulk-freed** at the block's `}` (no per-binding drop; reuses `align_rt_arena_*`). This is
  the cache-friendly fast path: linear allocation, no malloc/free churn.
- **Outside any arena (free-standing)** — heap-owned with a **drop** inserted at end of the
  binding's scope (§7). This is what lets a function **return** an owned `array<T>` (move out;
  no drop at the producing scope because it was moved), so materializing terminals are fully
  usable, not arena-confined.

**Move-out is allowed only for free-standing owned values.** A *free-standing* owned value is
`Static` (it owns its heap buffer, which it carries with it), so returning it / moving it to
an outer binding transfers ownership to the destination and no drop fires at the producing
scope. An *arena-allocated* owned value is `Arena(k)` — its buffer lives in the arena and is
bulk-freed at the block's `}`. The single escape rule (§4) therefore **forbids moving or
returning an arena-allocated owned value to a longer-lived region**: doing so would leave a
dangling pointer after the bulk-free. To produce an owned `array`/`string` that must outlive
the arena, allocate it free-standing (the default outside an arena) or `.clone()` it out.
`.clone()` deep-copies: in an arena it allocates in that arena (`Arena(k)`); outside, it is
heap-owned with its own `Drop`.

---

## 7. Drop insertion (MIR level)

Free-standing owned values (§6) need their backing freed exactly once, on every path.

- **Where:** `align_mir` inserts an explicit `Drop(slot)` at each scope exit for every
  owning binding that is *live and not moved-out* at that point — block end **and** every
  early exit already handled for arenas (`return`, `?`, and later `break`/`continue`). This
  extends the existing `Builder.arenas` cleanup stack / `emit_arena_cleanup` to a parallel
  per-binding drop stack.
- **How MIR knows arena vs free-standing:** regions are an analysis result, not part of `Ty`,
  so the owned type alone does not say where it was allocated. `align_sema` therefore
  **records, per owning binding, its allocation mode** (`Arena(k)` vs free-standing `Static`)
  as part of the HIR/region result it already hands to `align_mir`. MIR drops only
  free-standing bindings; `Arena(k)` bindings are skipped (the arena bulk-frees them). A
  minimal carrier — e.g. a per-`Let` "owns a free-standing heap value" flag derived from the
  region analysis — is enough; MIR does not re-run region inference.
- **Skip when moved:** if `MoveCheck` marks the binding moved (returned, passed by value,
  reassigned), no drop is emitted — ownership left the scope.
- **Skip when arena-allocated:** arena values are bulk-freed; they are not on the drop stack.
- **Visible:** the `Drop` is explicit in MIR (honoring "nothing hidden" at the IR level; the
  source-level signal is that the type is a Move/owning type and the allocation was written).
- **Runtime:** `Drop` of an `array`/`string` lowers to a single `align_rt_free`-style call
  (the buffer is one allocation). No element-wise destructors in v1 (elements are scalars /
  owned-flat); nested-owning-element drop is a later refinement.

Order: drops run in reverse declaration order at a scope exit, after arena frees of inner
arenas but consistent with lexical nesting.

---

## 8. Materializing terminals

With §6 + §7 in place, the M4 carryover terminals become ordinary owned-array producers:

- `filter(p)`, `map(f).to_array()`, `partition(p)`, `sort()` / `sort_by(k)`, `chunks(n)`,
  and array-valued results all return an owned `array<T>` (or `array<array<T>>` for
  `chunks`).
- Allocation site follows §6 (arena bump if in an arena, else heap-owned + drop).
- The fused single-loop model (M4) still applies to the *non-materializing* prefix; a
  materializing terminal writes into the freshly allocated result buffer at the end of that
  loop (sizing: `filter`/`partition` need a count pass or a growable buffer — v1 uses a
  growable owned buffer, an over-allocate-then-shrink or two-pass count; decide per terminal
  in its slice).

These stay compiler-known builtins (monomorphic per element type) until minimal generics
land; no general generics are required for v2.

---

## 9. Zero-copy decode — borrow + explicit `.clone()` (locked)

**Decision (locked):** a `json.decode`-d `str` / `array` / nested field is a **zero-copy
view into the input buffer**, region-tied to the input (§3, §5). To make a decoded value
outlive its input, the user **explicitly `.clone()`s** it. The compiler **never** silently
inserts a copy on escape.

Rationale (the four-way-alignment / hardware case):
- The cache-friendly fast path — decode → process → discard, all over the input bytes with no
  second allocation — is the *borrow itself*, and is identical to any auto-copy scheme.
- The only difference is the rare escape path, where a copy is physically unavoidable; making
  it an explicit `.clone()` keeps **Nothing hidden** and **Predictable performance** (no
  silent cost-class jump when a value starts to escape).
- An escaping `.clone()` *inside an arena* is a bump allocation (cache-local, bulk-freed), so
  "escape" is not a malloc cliff.

This **supersedes** the current draft.md §12 "Zero Copy" wording ("only when there is an
escape is a decode buffer used"), which implied a compiler-inserted copy. §12 must be updated
to the explicit-`.clone()` rule (§12 of this doc).

`json.decode` then extends from "all-scalar flat struct" to: `str` fields (a `{ptr,len}` view
into the input), `array<T>` fields (a length + a view/region over the input), and nested
structs — each carrying the input's region. The §19 example (`array<User>` with a `str name`)
runs by decoding inside the `arena {}` that holds the input, processing zero-copy, and only
cloning anything that must outlive the arena.

---

## 10. What changes vs. today (migration map)

| area | today (point solution) | v2 |
|------|------------------------|----|
| `region_of` | handles HeapNew/Clone/Template/Local/Block/If; `_ => 0` | total over all view producers incl. Call-reborrow, field/index/project, struct-lit, decode |
| slices | `local_backed_slice` set + `slice_is_local` | `Frame` region; same single escape rule |
| struct fields | region-0 `str` only; no box/slice/array fields | any field; struct region = max(fields) |
| owned arrays | none (literals consumed by reduction only) | owned `array<T>` Move; arena-bulk-free or free-standing+drop |
| drop | arena bulk-free only | + per-binding MIR `Drop` for free-standing owned values |
| decode | scalar-only (copied in) | + str/array/nested as region-tied borrows; explicit `.clone()` to escape |
| `Ty` | `Slice(Scalar)`, struct str region-0 | regions are an *analysis* (not in `Ty`); `Ty` may gain owned `Array`/`String` (dynamic) distinct from fixed `Array(Scalar,N)` |

The region is an **analysis result**, not part of `Ty` (keeps `Ty: Copy` and avoids surfacing
lifetimes). `MoveCheck` and `EscapeCheck` consume it; codegen/`Ty` do not carry it.

---

## 11. Implementation slices (ordered)

Each slice is a vertical, test-backed PR; later slices depend on earlier ones.

1. **[done]** **Region lattice + unified `EscapeCheck`.** Introduce `Region` and the single
   `outlives` rule; re-express the existing point solutions on top of it with **no behavior
   change** (pure refactor, all current tests green). This is the safety net for everything
   after.
2. **[done]** **Structs carry a region.** Lift the region-0 `str`-field restriction; struct
   region = max(fields); `region_of` handles `StructLit`/`Field`; `AssignField` uses the
   single `outlives` rule against the base struct's region. A struct holding an arena `str`
   is now constructible and usable inside its arena, and escape-checked as a whole. (Adding
   `slice`/`box` view *fields* — needs the matching field-layout/codegen — is a follow-on.)
3. **[done]** **Owned dynamic `array<T>` (arena mode first).** New `Ty::DynArray(Scalar)`
   (owned, Move, `{ptr,len}` layout). `.to_array()` materializes a fused map/where pipeline
   into an arena-bump-allocated owned array (bulk-freed), **restricted to arena context** (no
   drop yet). New MIR `ArenaAlloc` / `PtrStore` / `MakeDynArray` + a `lower_array_collect`
   loop (over-allocates to the source length — map/where never grow). The result is consumed
   like a slice (`.len()`, `.sum()`, pipeline source) via the shared `{ptr,len}` path; region
   = `Arena(k)`, so it cannot escape its arena. (`where`-first inline-literal element
   inference still defaults to i64 — a separate, pre-existing limitation.)
4. **[done]** **Free-standing drop.** Per-binding MIR `Drop` + runtime `free`; move-out skips
   drop (a returned/moved array is owned by the callee, so the source frame does not free it);
   owned arrays now allowed outside arenas and **returnable** across functions. `.to_array()`
   with no enclosing arena heap-allocates via libc `malloc` and is freed at every function exit.
   Drop slots are null-initialized (`DropFlagInit`) so an unreached/never-allocated path is a
   no-op `free(null)`. New MIR `HeapAllocBuf` / `Drop` / `DropFlagInit` + `emit_exit_cleanup`
   (drops then arena-ends at each `return`/`?`/fall-through); `MoveCheck` tracks `ever_moved`,
   and `check_file` derives each fn's `drop_locals` (owned arrays neither moved-out nor
   arena-regioned). Now materializing terminals work anywhere. A bare owned local returned as
   the function's trailing expression (`fn make() -> array<i32> { ys := …; ys }`) is correctly
   treated as moved-out (return type is a Move type) so it is not double-freed.
4.5 **[done]** **Complete drop coverage (null-on-move).** Closes the two sound-but-leaky gaps
   the slice-4 Gemini review (#42) flagged. `drop_locals` now keeps **every** free-standing
   owned local (the `ever_moved` exclusion is gone); MIR nulls a moved local's slot at each
   *direct* move site (`null_moved_source` at return / let / assign / call-arg / function tail,
   recursing through block & arena tails) so its exit `Drop` is a no-op `free(null)` when moved
   and a real free on the path where it is not — no double-free, no leak on conditional moves.
   Unbound owned-array temporaries consumed in place are freed via a new MIR `DropValue`
   (`SrcSetup.temp_free`), both after a fold (`make().sum()`) and after a collect copy
   (`make().map(f).to_array()`). `temp_free` is set for sources that unambiguously own a fresh
   buffer — a `.to_array()` materialization or a call returning `array<T>` — but **not** for a
   bound `Local`/`Field` (a borrow, freed by its owner) nor for `Block`/`If` sources (which may
   borrow a bound local in a branch, e.g. `(if c { ys } else { zs }).sum()`; blanket-freeing
   would double-free, so those stay a sound bounded leak). Moving a *bound* owned local out
   through an `if`/`else` arm (or `else`-unwrap fallback) is rejected for now (sema deferral
   diagnostic) — codegen only nulls at direct sites; bind the branch result to a local first.
5. **More terminals (in progress).** `min`/`max` reductions **[done]** — fused-loop reducers
   (`Reducer::MinMax`) that keep an element only when it beats the running best (a conditional
   update branching to the loop `cont`), seeded with the element type's extreme so an empty
   pipeline yields that extreme (the fold identity, like `sum → 0`). Completes the common
   reduction set (`sum`/`count`/`min`/`max`/`any`/`all`/`reduce`). `scan(f, init)` **[done]** —
   a *materializing prefix fold* on the `to_array` collect loop (`CollectKind::Scan`): threads an
   accumulator seeded with `init` and appends the running `acc = f(acc, element)` per survivor,
   yielding an owned `array<A>` (freed as a free-standing temporary / arena-bulk-freed like
   `to_array`; `temp_free` now also recognizes a `scan` source). `dot` **[done]** — `a.dot(b)`,
   the inner product `Σ a[i]*b[i]`, folded in one counted loop over two slot-backed sources
   (`lower_array_dot`). First cut restricted to two fixed-length arrays of the same numeric
   scalar element and the same statically known length (sema-checked) — the SIMD/vector case;
   `slice`/`array<T>` dot with runtime lengths (and a runtime length-equality check) is a
   follow-up. `sort` **[done]** — `source.….sort()` materializes survivors into an owned
   `array<T>` (the `to_array` collect loop) then sorts that buffer ascending in place with
   insertion sort (`lower_array_sort`): reads via `SliceIndex`, writes via `PtrStore` through the
   buffer pointer (new `SlicePtr` rvalue). First cut: numeric scalar elements, no comparator
   argument (a `sort(cmp)` overload and a faster-than-O(n²) sort are follow-ups). **Remaining:**
   `partition` (needs a 2-array product/tuple result — no tuple type yet) and `chunks` (needs a
   nested `array<slice<T>>` type) — each its own follow-up, gated on new type machinery.
6. **Zero-copy decode (str/array/nested).** Decoded views region-tied to the input; explicit
   `.clone()` to escape; **draft.md §19 runs in full** → M5 truly complete.
   - **[done] 6a — `str` field decode.** `json.decode` now accepts `str` fields: each decodes
     as a zero-copy `{ptr,len}` view into the input buffer (the runtime's `string()` already
     borrows the input and rejects escapes, so its pointer is the content's absolute address;
     codegen tags `str` fields `(3<<8)|16`). The decoded struct is **region-tied to its input**:
     `region_of(JsonDecode{input}) = region_of(input)` and `region_of(Try) = region_of(inner)`,
     so a view decoded from arena-allocated input cannot escape the arena (conservative — even a
     scalar-only struct is tied; decode from a `str` param/literal is Static, hence returnable,
     which is sound because the caller owns the buffer). Tested both directions.
   - **Deferred:** `str.clone()` to escape; array / nested-struct field decode; and a general
     **call-result region tie** — `region_of(Call)` is `Static`, so a view-returning function
     called with an arena argument (`r := parse(arena_str)`) loses the tie at the call boundary.
     This is a *pre-existing* gap (it already affects `fn f(s: str) -> str = s`), not introduced
     here; closing it (propagate the shortest region-tracked arg region through a `Call`, like
     `slice_is_local` does for slices) is its own slice.
7. **`string` (owned) + `bytes`/`buffer`.** Owned string per draft.md §12, on the same
   owned/drop machinery.

`out` parameters (draft.md §7) are a no-alias optimization, largely orthogonal to
ownership/regions — deferred to its own slice (not gated on v2; recorded in `open-questions.md`).

---

## 12. Spec / doc updates this phase requires

Per `CLAUDE.md` "when changing a design decision, update all of": this phase changes the
decode-escape semantics and lifts several deferrals, so on landing each slice update:

- **draft.md §12 "Zero Copy"** — replace "only when there is an escape is a decode buffer
  used" with the explicit-`.clone()`-to-escape rule (§9 here).
- **draft.md §6/§7** — note owned `array<T>` allocation modes (arena bulk-free vs
  free-standing drop) and that views carry inferred regions.
- **docs/language-spec.md** — mirror the §12/§6/§7 changes in the digest.
- **docs/design-notes.md** — record the rationale for explicit-clone-over-auto-copy
  (Nothing hidden + Predictable performance > convenience) and for the unified region lattice
  over point solutions.
- **docs/impl/03-types.md §6–§7** — the region lattice, struct-carries-region, owned/drop.
- **docs/impl/04-mir.md** — the `Drop` terminator/stmt and per-binding drop stack.
- **docs/impl/07-roadmap.md** — move items from "[todo — blocked on Memory Model v2]" to done
  as slices land; flip the §19 completion condition once slice 6 ships.
- **docs/open-questions.md** — move "Memory model v2" from Open to Settled (with this doc as
  the record); record the decode-escape decision and the owned-array allocation decision.

---

## 13. Open sub-questions (decide within the phase, not before)

- `filter`/`partition` result sizing: two-pass count vs growable buffer vs over-allocate +
  shrink (per-terminal, slice 3/5).
- Drop order across mixed arena + free-standing owned values at one scope exit (likely:
  inner arenas freed first, then per-binding drops in reverse declaration order).
- `chunks` element type: `array<slice<T>>` (views into one buffer) vs `array<array<T>>`
  (owned). Views are cheaper and fit the region model — leaning views.
- Whether owned dynamic `array<T>` and fixed `Array(Scalar, N)` share one `Ty` variant or
  stay distinct (affects ABI; `05-backend-llvm.md` §2 SoA interacts).
- Nested-owning-element drop (an `array<string>`): element-wise free — v1 keeps elements
  flat/scalar and defers this.
