# 08 — Memory Model v2: borrow regions + owned heap/drop

Status: **IMPLEMENTED** (see the §11 slice ledger for the foundation record).
This was the foundation phase slotted **after the foundation-safe scalar work and before M6
(SIMD)** (`07-roadmap.md`, `open-questions.md` Settled "Memory model v2"). It was designed **as
a whole here first**, then implemented in ordered slices (§11) — nothing it gated shipped in a
corner-cut form before it landed. **`draft.md` §19 now runs end-to-end, including the `fs`/`io`
boundary.** The text below retains the original design narrative, while §11 records what actually
shipped. Later slices also delivered tuples, `partition`, `array<slice>`/`chunks`, whole-struct
indexing for supported element shapes, deep-drop `array<string>` producers, and `buffer`.
Important remaining boundaries include `array<Struct>.clone()`, Move-element indexing, and precise
per-function return-borrow summaries.

It exists because the deferred "ideal forms" of M4 and M5 both rest on one foundation:
- M5 `json.decode` for `str` / `array<T>` / nested fields → zero-copy views region-tied to
  the input → **draft.md §19 runs in full** (true M5 completion).
- M4 carryover `where` / `scan` / `partition` / `sort` / `chunks` + array-valued results →
  built on owned, dynamic heap arrays with drop (materialization).

Before v2 these were handled by three unrelated **point solutions** in `align_sema`'s
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
literal / owned-from-scalars              → Static
view parameter (slice/str/… param)        → Static within this fn  (borrows the caller; returnable)
heap.new                                   → region of the enclosing arena (Static if none)
dynamic template                           → enclosing arena, or Frame with a hidden owner if none*
clone                                      → owned `string` in the current implementation**
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

\* Since 2026-07-15 an arena-free dynamic `template`/`json.encode` view is backed by a hidden scoped
`string` owner and cannot escape its frame; a static-only template folds to a `Static` literal.
String `+` is a settled hard error and has no checker/MIR concatenation path. The owned-collection
case is different — see §6: a
free-standing **owned** `array`/`string` is heap-owned and `Static`-lived *until its `Drop`*
(not `Frame`). `Frame` is only for **borrows** of frame-local storage (a slice of a
frame-local array literal, or a view into a by-value parameter's interior), never for a
view *parameter* (which borrows the caller → `Static`, returnable).

\*\* Prose about making an in-arena clone a bump allocation has drifted from the current heap-owned
implementation. Audit 13 leaves that contract as a Claude Code question rather than deciding it.

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
the arena, allocate it free-standing (the default outside an arena) or use a type-specific explicit
copy operation. In the current implementation `str.clone()` always produces a free-standing,
heap-owned `string` with its own `Drop`, even when called inside an arena;
`array<Struct>.clone()` is not implemented.

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
- **Runtime:** the foundation cut lowered flat `array`/`string` drop to one
  `align_rt_free`-style call. Later slices added recursive struct drop and deep-drop collections
  such as the `array<string>` returned by `fs.read_dir` and `dns.resolve`.

Order: drops run in reverse declaration order at a scope exit, after arena frees of inner
arenas but consistent with lexical nesting.

---

## 8. Materializing terminals

With §6 + §7 in place, the M4 carryover terminals become ordinary owned-array producers:

- `where(p)`, `map(f).to_array()`, `partition(p)`, `sort()` / `sort_by(k)`, `chunks(n)`,
  and array-valued results all return an owned `array<T>` (or `array<array<T>>` for
  `chunks`).
- Allocation site follows §6 (arena bump if in an arena, else heap-owned + drop).
- The fused single-loop model (M4) still applies to the *non-materializing* prefix; a
  materializing terminal writes into the freshly allocated result buffer at the end of that
  loop (sizing: `where`/`partition` need a count pass or a growable buffer — v1 uses a
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
   bound `Local`/`Field` (a borrow, freed by its owner). Since 2026-07-15, general borrowed uses of
   unbound Move expressions use path-local synthetic owners instead: a separate temporary bit joins
   `Block`/`If`/`match`/`else`/`?` paths, so a fresh selected arm is dropped while a bound selected arm
   remains borrowed. Value-carrying owned moves also carry their ordinary runtime ownership bit and
   null the selected source, removing the older branch-move restriction without blanket frees.
5. **More terminals (complete for this slice; later additions noted).** `min`/`max` reductions **[done]** — fused-loop reducers
   (`Reducer::MinMax`) that keep an element only when it beats the running best (a conditional
   update branching to the loop `cont`), seeded with the element type's extreme so an empty
   pipeline yields that extreme (the fold identity, like `sum → 0`). Completes the common
   reduction set (`sum`/`count`/`min`/`max`/`any`/`all`/`reduce`). `scan(init, f)` **[done]** —
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
   argument (a `sort(cmp)` overload and a faster-than-O(n²) sort are follow-ups). The original
   remaining items, `partition` and `chunks`, shipped after tuples and the required nested view
   machinery landed; see the M6/M7 records in `07-roadmap.md`. Runtime-length `dot` and richer sort
   policies remain follow-ups.
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
     - **Region tracking through `Option`/`Result` (fix).** The region tie has to survive the
       `Result` wrapper, since `json.decode` yields `Result<Struct, Error>`. `tracks_region` now
       recurses into `Option`/`Result` payloads (true iff a `Struct` payload tracks a region), and
       `region_of` propagates through `Ok`/`Some`/`Err`/`?` (= the inner region) and `else`
       (= the shorter of the two arms). Without this, binding the raw decode to a `Result`-typed
       local and unwrapping it later (`res: Result<U,E> := json.decode(d); u := res?; return Ok(u)`)
       slipped the escape check (the `Result` local wasn't region-tracked) → use-after-free.
   - **[done] 6b — call-result region tie.** Closed the (pre-existing, unsound) gap where
     `region_of(Call)` was `Static`: a view-returning function called with an arena argument
     (`return dup(arena_str)`, `fn dup(s: str) -> str = s`) lost the tie at the call boundary and
     escaped the arena unchecked → use-after-free. `region_of(Call { args })` now folds the
     **shortest-lived argument region** (the region analogue of `slice_is_local`'s arg
     propagation): the result is assumed to borrow its shortest-lived arg. Conservative — a
     function that does *not* return a borrow of its args is over-restricted (precise per-fn
     "returns a borrow of arg i" inference is a later slice) — but sound. Non-tracked args
     (ints/literals) are `Static` and don't shorten, so `dup("hi")` stays returnable.
     - **`reduce` accumulator region (same fix, historical trigger).** A sibling gap:
       `region_of(ArrayReduce)` was `Static`, so a region-tracked accumulator could escape the arena
       it was folded in → use-after-free. The original trigger used the now-forbidden string `+`;
       the propagation rule remains necessary for any permitted region-tracked accumulator. `reduce` now joins
       `to_array`/`scan`/`sort` at `Region::arena(depth)` (the accumulator is folded in the
       enclosing arena). Scalar accumulators are unaffected (no region). (Note: the precise region
       is `arena(depth)`, not `shorter(init, source)` — an empty/all-`Static`-arg reduce still
       allocates its fresh accumulator at `depth`.)
   - **Since shipped:** array and nested-struct field decode. Precise per-function borrow inference
     (which argument, if any, a call result actually borrows) remains a refinement that could lift
     the conservative region join described in 6b.
7. **`string` (owned) + `bytes`/`buffer`.** Owned string per draft.md §12, on the same
   owned/drop machinery.
   - **[done] 7a — owned `string` + `str.clone()`.** Added `Ty::String`, the heap-owned dual of
     `str` (same `{ptr,len}` layout, but **Move** and region-tracked). It reuses the owned-array
     machinery wholesale: a free-standing (`Static`) `string` is in `drop_locals` and `Drop`-freed
     at every exit, nulled-on-move, `ret_is_move`, and `tracks_region`. `str.clone()` (`Ty::Str |
     Ty::String → string`) is the producer and the **explicit escape hatch out of a zero-copy
     view**: unlike `box.clone` it needs no arena — the result owns a fresh `malloc`'d buffer
     (`align_rt_str_clone`, null buffer for the empty string so `free` is a no-op), so it can be
     returned out of the arena its source was built in. `print`/`.len()` **borrow** a `string`
     (read `{ptr,len}` as a `str`), so a printed string stays usable; both sema's `MoveCheck` and
     MIR's null-on-move special-case `print` as non-consuming. Tested: clone escapes an arena;
     clone of a decoded field; non-cloned arena `str` still rejected; use-after-move rejected.
     A `string` passed by value to a callee is **moved** (the callee owns and `Drop`-frees it;
     the caller's slot is nulled on the move). An owned-`string` *parameter* is therefore NOT
     entry-null-initialised — it arrives owning a valid buffer, and zeroing it would clobber the
     argument (a bug fixed here; the entry `DropFlagInit` now skips parameter slots).
   - **[done] 7b — `string` → `str` borrow coercion.** An owned `string` argument now satisfies a
     `str` parameter by *borrowing* it (`ExprKind::StrBorrow`): the two share the `{ptr,len}`
     layout, so the coercion is zero-cost (MIR lowers it to its inner load; no new runtime/codegen).
     The borrow is **non-consuming** — sema's `MoveCheck` treats `StrBorrow` like the other read-only
     receivers and MIR does not null the source slot — so the `string` stays owned by its slot and is
     `Drop`-freed once at exit, usable across multiple borrows (`a := show(s); b := show(s)`). The
     view is **`Frame`-regioned** (`region_of(StrBorrow) = Frame`): it borrows storage the current
     frame `Drop`-frees, so a function returning a borrow of its `str` arg, fed a borrowed `string`,
     is correctly rejected as an escape (via the slice-6b call-result region tie). Applied at call
     arguments only (`check_arg`); a `let s: str := some_string` borrow is a follow-on. As with 6b,
     the call-result tie is conservative — a `str`-param callee that returns a *fresh owned* `string`
     is over-restricted when fed a borrowed `string` (use `.clone()`); precise per-fn borrow
     inference is the lift.
   - **[done] 7c — `builder()` / `.write()` / `.to_string()`.** The canonical string-construction
     API (draft.md §12, recommended over `a + b` concat), surfaced on the runtime `Builder` that
     already backs `template`. New `Ty::Builder` (an opaque owned handle, a Move type) with three
     forms: `builder()` opens it (`BuilderNew`), `b.write(str)` / `b.write_int(i64)` append
     (`BuilderWrite`, borrowing the builder — mutate-through-handle, not consume), and
     `b.to_string()` finishes into an **owned** `string` (`BuilderToString`), consuming the builder.
     No parser changes — `builder()` is a builtin call and the methods are ordinary method-call
     syntax. `to_string()` lowers to a new runtime `align_rt_builder_into_string` that copies into a
     fresh `malloc`'d buffer (so the finished `string` outlives the builder and any arena, freed by
     its slot's `Drop`); an unfinished builder is `Drop`-freed at exit via `align_rt_builder_free`
     (null-safe). The builder reuses the owned-local machinery: it is in `drop_locals`, nulled on
     the `to_string` move, and `DropFlagInit`/`Drop` are **type-aware** in codegen (a `builder` slot
     holds a bare pointer — null-init and `builder_free` — vs the `{ptr,len}` collections' `{null,0}`
     + buffer `free`). `b.write(owned_string)` reuses the slice-7b borrow, so the source `string`
     stays usable. Tested: build a greeting + length (e2e), borrow a `string` into `write` and an
     unfinished builder (e2e), and sema move/`to_string`-consume/wrong-arg/non-builder-receiver.
   - **[done] 7d — builder scalar writers.** Surfaced `write_bool` / `write_char` / `write_float`
     on `builder` (the runtime fns already existed for `template`), so the builder's write set
     matches `print`/`template` scalar coverage. `BuilderWriteKind` gains `Bool`/`Char`/`Float`;
     `check_builder_write` dispatches by method name (`builder_write_kind`) and validates the arg's
     scalar (no implicit int→float — `write_float(3)` is rejected). codegen mirrors `print`'s
     widening: `write_bool` zexts i1→i32, `write_char` passes the u32, `write_float` picks
     `f32`/`f64` by operand width. Tested e2e (all kinds in one builder) + sema (each accepts its
     scalar, rejects a mismatch).
   - **[done] 7e — `let`/assign-position `string` → `str` borrow.** Extended the slice-7b borrow
     coercion (call arguments only) to `str`-annotated `let` bindings (`view: str := owned`) and
     `str`-place assignments (`view = owned`), via a shared `check_str_init` helper (mirrors how
     `check_slice_init` serves both call args and slice-annotated lets). The borrow is `Frame`-
     regioned, so returning a `let`-bound `str` view of a local `string` is rejected — now with a
     borrow-specific diagnostic ("…borrows local storage…; use `.clone()`…") split from the arena
     message. Also fixed a latent bug this surfaced: the `Stmt::Assign` escape check used
     `target = arena(decl_depth)` (= `Static` at depth 0), wrongly rejecting a `Frame` borrow
     assigned to a frame-local binding; the target is now `Frame.shorter(arena(decl_depth))`
     (escape past the frame is still caught by the return / struct-field-store checks; a deeper
     arena value into a shallower binding stays rejected).
   - **Later status:** `buffer` and its I/O surface have shipped. The in-arena bump-clone
     optimization remains optional: `str.clone()` is heap-owned, which is sound but does not turn
     the clone into an arena bump allocation.
8. **Owned (Move) payloads in `Option`/`Result`.** The remaining "ideal form" carryover (a
   materializing `json.decode` into `array<T>`, fallible functions returning owned values) is gated on
   one type-system gap: `Ty::Result(Scalar, Scalar)` could only carry *scalars*, so a fallible
   function could not even return `Result<string, Error>`. This slice lets an `Option`/`Result`
   hold an owned **Move** payload.
   - **[done] 8a — owned `string` payload.** Added `Scalar::String` (var-free, so `Ty: Copy`
     holds) with `Scalar::is_move()`; `payload_is_move(ty)` marks an `Option`/`Result` whose
     payload is owned. Such an aggregate is itself Move: it joins `drop_locals` / `is_move` /
     `ret_is_move`, and its `Drop` frees each owned payload field's buffer pointer (`Some`/`Ok` =
     field 1, `Err` = field 2) **null-safely**. The key invariant that makes the drop branch-free:
     constructors now build on a **zeroed** aggregate (`const_zero`, not `get_undef`), so the
     *inactive* arm's payload reads `{null,0}` and its drop is a `free(null)` no-op — only the
     active owned payload is real. `?` / `else` move the payload out and **null the source** slot
     on the success edge (`null_moved_source` in `lower_try`/`lower_else_unwrap`), so the source's
     exit `Drop` frees null (no double-free); on the failure edge the source already holds a
     zeroed payload. Region: `tracks_region` already recurses into payloads, so `Ok(owned_string)`
     is `Static` (returnable) while a view-backed payload stays region-tied. Enables
     `fn f() -> Result<string, Error>` / `Option<string>`. MIR-verified: bound `r := mk(); s := r?`
     frees the buffer exactly once (source nulled on the Ok edge), Err/None paths free null.
     Tested e2e (Result/Option unwrap + Err path) + sema (construct/return/use-after-`?`).
   - **[done] 8b — owned `array<T>` payload.** The owned-collection dual of 8a: added
     `Scalar::DynArray(PrimScalar)` (the array dual of `Scalar::String`), reusing the 8a machinery
     wholesale — same `{ptr,len}` layout, `is_move`, zeroed-base construction, null-safe payload
     free, and `?`/`else` move-out-then-null-source. The only new piece is `PrimScalar` (a small
     `Copy`, **non-recursive** Int/Float/Bool/Char enum) carrying the element type, so an
     `array<T>` can sit in an `Option`/`Result` payload without making `Scalar`/`Ty` recursive;
     `ty_to_scalar`/`scalar_to_ty` round-trip through `scalar_to_prim`/`prim_to_scalar` (a
     non-primitive element — struct/dynamic-array — is simply not payload-representable yet). The
     box-payload guard already rejects Move scalars, so `box<array<T>>` is a clean diagnostic.
     Enables `fn f() -> Result<array<i64>, Error>` / `Option<array<f64>>`. MIR-verified: the
     heap buffer is moved `mk → Result → ?-unwrap → local` and freed exactly once; Err/None free
     null. Tested e2e (Result/Option unwrap + sum) + sema (construct/return/box-rejection).
   - **[done] 8c — `json.decode` into `array<scalar>`.** Parse a JSON array of scalars into an **owned**
     `array<T>` (`Result<array<T>, Error>`, now representable thanks to 8b). New
     `ExprKind::JsonDecodeArray` + MIR `Rvalue::JsonDecodeArray` + runtime `align_rt_json_decode_array`,
     mirroring the struct-decode pattern (materialize into an out slot, branch `Ok(<array>)` /
     `Err(<code>)`). The elements are **copied** into a fresh `malloc`'d buffer (not borrowed), so
     the result is `Static`/returnable — *not* region-tied to the input (`region_of` leaves it at
     the `Static` default; only the struct decode ties to its input for zero-copy `str` fields).
     `check_json_decode` dispatches on the expected Ok scalar (`Struct` → object, `DynArray(prim)` →
     array); int/float/bool elements only (a `str` element would be region-tied — deferred). Same
     `(kind<<8)|width` element tag as struct fields. M5 cut: an empty array allocates nothing
     (`{null,0}`). **Latent-bug fix surfaced here:** `null_moved_source` now sees through
     `Ok`/`Some`/`Err` wrappers, so `return Ok(xs)` of a *bound* owned local nulls its slot — else
     the local's exit `Drop` double-freed the buffer now owned by the returned aggregate (a slice-8a
     gap exposed by `return Ok(decoded_array)`; also fixes `return Ok(bound_string)`).
   - **[done] 8d-1 — `json.decode` into `array<Struct>` (decode + len + drop + region/escape).** The
     draft.md §19 headline type: an owned, dynamic AoS of structs. New `Ty::DynStructArray(id)` +
     `Scalar::DynStructArray(id)` (the struct dual of `DynArray`; both `{ptr,len}`, Move, freed by
     `Drop`), so `Result<array<Struct>, Error>` is representable and threads through `?`. The
     `array<T>` annotation resolves a struct element to `DynStructArray`. New
     `ExprKind::JsonDecodeStructArray` + MIR `Rvalue::JsonDecodeStructArray` + runtime
     `align_rt_json_decode_struct_array` (parses a JSON array of objects, reusing the per-object
     `parse_object` helper factored out of the scalar struct decode, into a growing buffer then a
     fresh `malloc`'d AoS). The buffer is owned, but each element's `str` fields are zero-copy
     views into the input, so the array is **region-tied to the input** (`region_of` ties it like
     the single-struct decode; `tracks_region` includes it) — a decoded array from an arena/param
     input cannot escape that input (`.clone()` to escape — array clone is a later capability).
     `.len()` works; the buffer is `Drop`-freed at scope exit, and the `return Ok(bound)` slot-null
     (slice 8c) covers a bound struct array too. Empty `[]` → `{null,0}` (no alloc). Decode-side
     only this slice — **the `.where(.active).field.sum()` pipeline over a dynamic struct array is
     8d-2**.
   - **[done] 8d-2 — pipeline over a dynamic `array<Struct>`.** The rest of the draft.md §19
     example now runs (compiler side): `users.where(.active).score.sum()` fuses into one counted
     loop over the heap AoS. The fused-loop lowering (both the reducing and collecting loops)
     gained a "struct view" source mode: a `DynStructArray` is set up as its `{ptr,len}` value with
     a runtime `SliceLen` bound, and field projection / `where(.field)` index it through the buffer
     pointer via a new `Rvalue::IndexFieldPtr` (`getelementptr %Struct, ptr, index, field`) instead
     of the stack-slot `IndexField` — shared by both loops through a `lower_field_access` helper.
     sema's pipeline check accepts a `DynStructArray` source (requiring a variable, since field
     projection addresses through the owned buffer that the binding keeps alive); `map`/`where`
     over the whole struct element is still rejected (project a field first), same as the fixed
     AoS. The source is a borrow (the owner's exit `Drop` frees it — `setup_source` sets no
     `temp_free`). **draft.md §19 runs end-to-end except the `fs`/`io` std boundary.**
   - **[done] 8e — scalar element indexing `recv[index]`.** Surface `[]` subscript on a scalar
     `array` / `slice` / owned `array<T>` (yields the element scalar). Bounds-checked: MIR emits
     `if index < 0 || index >= len { bounds_fail(index, len); unreachable }` then the element load
     (`Index` for a stack slot, `SliceIndex` for a `{ptr,len}` view), with the runtime
     `align_rt_bounds_fail` aborting on the failing path (the settled panic model — a memory-safety
     violation in ordinary code is a hard error, never silent UB). New `ExprKind::Index` (a postfix
     `[]` operator in the parser).
   - **[done] 8f — struct-array element field access `arr[index].field`.** `users[i].name` /
     `ps[i].x` on a fixed `array<Struct>` or an owned dynamic `array<Struct>`. Fused into one
     bounds-checked element-field load (new HIR `ExprKind::ElemField`, lowered to the slot-based
     `IndexField` for a stack array or the pointer-based `IndexFieldPtr` for a dynamic one — the
     same addressing as a fused pipeline projection); no whole-struct copy. Detected in
     `check_field_access` when the receiver is an `Index` over a struct array. A `str` field is a
     view region-tied to the array (so it cannot escape the array's input); a Move-type field is
     rejected (same double-free concern as 8e).
   - **Later status:** whole-struct element values for supported Copy structs, broader struct-array
     pipelines, the `fs.read_file` / `io.stdout.write` boundary, tuples/`partition`, and
     `array<slice<T>>`/`chunks` have shipped on this foundation. `array<Struct>.clone()` remains an
     escape-hatch gap, and indexing an element whose value itself is Move remains restricted because
     a read must define whether it borrows or transfers that element.

`out` parameters (draft.md §7) are a no-alias optimization, largely orthogonal to
ownership/regions — deferred to its own slice (not gated on v2; recorded in `open-questions.md`).

---

## 12. Spec / doc updates this phase requires — DONE

Per `CLAUDE.md` "when changing a design decision, update all of": this phase changed the
decode-escape semantics and lifted several deferrals. All of the following are now reconciled
(the milestone-closing consolidation):

- **[done] draft.md §14 "Zero Copy"** — the explicit-`.clone()`-to-escape rule (the section
  renumbered from the original §12; JSON is now §14).
- **[done] draft.md §6/§7** — `array<T>` owned/move + arena bulk-free + "a view cannot escape
  the arena" are stated; the two allocation modes' implementation detail lives here (§6) and in
  `03-types.md` rather than the high-level spec.
- **[done] docs/language-spec.md** — the feature-surface digest (terse keyword lists) already
  names `array<T>` / `json.decode` / arena / explicit heap; no contradiction to reconcile.
- **[done] docs/design-notes.md** — the "Memory model v2: one region lattice, explicit copies"
  section records both rationales.
- **[done] docs/impl/03-types.md §7** — a status note generalizing the sketch to the region
  lattice + owned/drop, pointing here as the authoritative model.
- **[done] docs/impl/04-mir.md §5** — a status note on per-binding `Drop` / drop flags /
  null-on-move / Option-Result owned-payload drop.
- **[done] docs/impl/07-roadmap.md** — Memory Model v2 marked DONE, the `[todo — blocked]`
  items reconciled, and the §19 completion condition marked met (compiler side).
- **[done] docs/open-questions.md** — "Memory model v2" moved from Open to **Settled** (this
  doc as the record), with the region-lattice / owned-drop / explicit-clone decisions recorded.

---

## 13. Historical sub-questions — resolved

- Materializing terminals choose their sizing strategy per operation; this is an implementation
  choice, not a source-level semantic distinction.
- Arena ends and per-binding drops are explicit in MIR; owned bindings drop in reverse lexical
  order and task/arena cleanup is emitted on early exits.
- `chunks` uses borrowed `slice<T>` elements in an owned outer collection.
- Fixed arrays and owned dynamic arrays remain distinct types/layouts.
- Deep element drop is implemented for producer-owned `array<string>`. It does not imply that every
  container position accepts a Move element: struct fields, sum-type payloads, and element indexing
  retain their documented ownership restrictions.
