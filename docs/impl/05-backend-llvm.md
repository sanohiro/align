# Backend: MIR → LLVM

Implementation model for `align_codegen_llvm`. It commits to **pure lowering**—Align's semantic
decisions (desugaring, fusion, SIMD legality, ownership/region) are already represented by MIR
(`04-mir.md`), and here we mechanically lower MIR to LLVM IR. Types and explicit runtime operations
are carried in MIR, so we **do not recompute** them (anti-rewrite, `00-overview.md`).

```text
MIR (optimized)  →  LLVM IR  →  object (.o)  →  [driver links] → executable
                                                  + align_runtime (06)
```

The implementation uses LLVM 22 through Rust's `inkwell` bindings. Remaining portability and
optimization-policy questions are collected in §10.

---

## 1. Type correspondence (Ty → LLVM type)

Map MIR's `Ty` (`03 §1`) one-to-one to LLVM types.

```text
Bool              i1 (i8 when stored)
Int(w, signed)    iW            (sign distinguished by the operation)
Float(32|64)      float | double
Char              i32 (Unicode scalar)
Unit              void for a function return; it has no ordinary SSA value
Vec(n, T)         <n x T'>      ← maps directly to LLVM vector type
Mask(T)           <n x i1>
Bitset            iN / [iW]
fixed array<T,N>  [N x T]                        inline, contiguous
owned array<T>    { T* ptr, i64 len }            owned, contiguous
Slice(T, _)       { T* ptr, i64 len }            view (Region does not surface in the type)
Str               { i8* ptr, i64 len }           (+ meta is separate, §6)
String/Buffer     { i8* ptr, i64 len }            owned headers
Builder           pointer to runtime builder state
Named(struct)     %struct.S = type { each field }   (layout is §2)
Named(sum)        { i32 tag, payload fields... } tagged aggregate
Option(T)         { i8 tag, T payload }          tag 0=None, 1=Some
Result(T,E)       { i8 tag, T ok, E err }        tag 0=Ok, 1=Err; inactive owned payloads are zero
Fn(..)            function pointer (+ environment pointer if there is a capture)
```

`Region` **does not appear in LLVM**. Safety is already verified in HIR (`03 §7`); codegen receives only the concrete value (an arena pointer, etc.). This is the final destination of "do not surface lifetimes".

One Align type is one LLVM type. MIR's type ids are finer than LLVM's type identity, so several MIR
spellings can name one Align type: `Ty::Tagged(id)` versus the source-shaped `Ty::Option`/
`Ty::Result` for the same nested value; two origin-specific generic instances that share one
`source_name`; an `Option<string>` argument bound to an `Option<str>` parameter. Codegen therefore
keys nominal identity on what the type *lowers to* — structs and sum types by `source_name`, nested
`Option`/`Result` by LLVM body (`build_tagged_types`) — never on the id alone. Emitting two
structurally equal but distinct LLVM types for one Align type makes `insertvalue`, `ret`, and call
arguments ill-formed; #670 did exactly that for nested tagged values and it went unnoticed until
#730 made `--rt-lto`, whose merged-module verifier was the pipeline's only one, the default.

### Module verification (every profile, on every emit path)

`build_module` verifies the module it just built. Every emit path — object, PGO, ThinLTO prelink,
`emit-llvm`, and the remark lens — funnels through it, so **the whole owner suite is a
well-formedness gate**: a `build_and_run` owner compiles with `rt_lto = false` and would otherwise
never meet a verifier, which is precisely how #670's ill-formed nested-tagged IR survived sixty
PRs. `link_in_rt_lto` separately verifies the **merged** module as before: a different scope (baked
runtime bitcode `build_module` never sees) and a different failure mode (symbol retargeting), so the
default object path deliberately keeps both. `emit_llvm_ir` runs neither of its own — both of its
steps already verify the same module — and simply never prints IR it knows is ill-formed.

A verification failure always reports LLVM's message, which names only the offending instruction.
The **whole module** goes to stderr only under `ALIGN_DUMP_IR`: now that this runs in every profile,
`apps/db` alone would otherwise spray ~7MB of IR at a release user's terminal and bury that line.

Verification was `cfg(debug_assertions)`-only when #765 introduced it, blocked on one pre-existing
MIR defect the debug gate exposed:

```align
fn probe(flag: bool) -> i64 {
  bound := "bound".clone()
  return (if flag { " tmp ".clone() } else { bound }).trim().len()
}
```

A borrowed owned temporary produced by value-carrying control flow lowered its `if` **twice**, and
the second copy stored the first copy's per-arm SSA values into the hidden owner's join slot from
blocks that do not dominate their definitions (`Instruction does not dominate all uses`).

The dominance violation was the loud half. The same double lowering hit **borrow-transparent
scopes** (`{ }`, `unsafe`, `arena`, a named arena, `task_group`) silently, because a scope has no
join to violate: the eager memo deduplicated the body's *expressions*, but statement stores and the
scope's own framing are not memoized, so each such receiver emitted its statement stores twice and a
second, empty `arena_begin`/`arena_end` (or `tg_begin`/`tg_wait`/`tg_end`) pair around nothing — a
spurious runtime region or task group per evaluation, with a correct exit code hiding it.

**Root cause and fix (MIR, not codegen).** `lower_expr`'s eager worklist lowers a parent's
`direct_expr_children` *ahead of* the parent, and the parent's own `lower_expr` call then returns
the memoized operand. That is sound only for an edge the parent consumes through `lower_expr`.
`StrBorrow` — the implicit `string` → `str` view, and an eager-worklist parent — consumes its child
through `lower_borrowed_owned` instead, so a control-flow child was lowered once in **non-borrow**
mode (which also nulls a bound arm's source local and re-emits the scope framing) and then again in
borrow mode by `lower_expr_for_borrow`.

`borrow_mode_differs` is now the **single authority** on "borrow mode is not `lower_expr`", and it
gates both sides: `lower_expr_for_borrow` returns `lower_expr(b, e)` before its match whenever the
predicate is false, so the match handles exactly the admitted kinds and its catch-all is
`unreachable!`; `eager_worklist_children` uses the same predicate to keep `StrBorrow` from entering
such a child. Ordering the guard first also makes the dangerous drift impossible: a kind added to
the match without being added to the predicate is inert (it delegates, as before) rather than
double-lowered. The borrow-transparent scope kinds come from `borrow_transparent_scope_block`, one
enumeration shared with `moved_drop_flag` and `temporary_drop_flag` — the latter still checks
`task_group` first, deliberately, because it reaches that helper only with a fresh tail and must
forward the ordinary ownership bit rather than recurse. A `debug_assert` in `lower_expr_for_borrow`
aborts lowering if a future borrow edge is pre-lowered anyway; a test binary is always a debug
build, so the whole owner suite carries that tripwire.

Filtering only the divergent kinds — rather than moving `StrBorrow` to out-of-line dispatch —
keeps a `StrBorrow` chain over non-divergent producers frame-free, which
`validate_hir_tests::checked_hir_depth_closure_matrix` (deep `path`/`regex` string spines at
`MAX_CHECKED_HIR_DEPTH` on a 2 MiB stack) requires; the dispatch variant overflowed it.

**Closure matrix (14 cells).** `if` / `match` / `else`-unwrap crossed with fresh, bound, and mixed
arms, owned by `owned_temporaries::borrowed_control_flow_temporaries_lower_exactly_once`; the five
borrow-transparent scopes owned by
`owned_temporaries::borrowed_scope_temporaries_lower_their_scope_exactly_once`; the diverging-arm
and `?`-arm early exits owned by
`owned_temporaries::borrowed_control_flow_temporaries_survive_diverging_and_try_arms`; and
`owned_temporaries::mixed_if_arms_drop_only_the_selected_temporary`, which is no longer `#[ignore]`d.

Each cell's witness is a structural MIR count that was **mutation-verified against the pre-fix
compiler**: the exact number of `branch` terminators for the control-flow cells (a `str_clone` count
would not discriminate — in a bound cell every `.clone()` sits in a `let` outside the duplicated
subtree), and the statement-store plus scope-framing counts for the scope cells (whose exit code is
identical on both compilers, because the duplicated store happens to be idempotent).

**`loop` is not in the matrix.** `lower_expr_for_borrow` has no `Loop` arm and `lower_loop` has no
borrow mode, so a loop-valued borrowed temporary always went through `lower_expr` and its memo:
`loop` × fresh/bound/mixed probes return the same values and lower **byte-identical MIR** before and
after this fix. The kind is structurally outside the class, not an untested cell.

Re-measured after the promotion, interleaving the two release `alignc` binaries over a cold codegen
cache: `apps/db` alone — the largest module, ~7MB of IR — is 8.768s unverified against 8.700s
verified at the median of 9 pairs (8.282s against 8.301s at the minimum), and the 78-example corpus
plus `apps/db` is 13.736s against 13.694s over 3 pairs. Both differences sit inside the host's own
run-to-run spread, at or under the ~1% #765 recorded; verification is not a measurable cost on the
object paths.

---

## 2. struct layout

The default is **AoS** (row-major, the value-type-centric `draft.md`). **SoA**, which helps for data parallelism, is treated as a transform over arrays.

```text
AoS   array<User> = contiguous User → { User* , len, cap }      (row-major, default)
SoA   soa<User>   = one contiguous column per field → {id[], name[], active[], score[]}
```

**Field order within a struct is unspecified for a non-`layout(C)` struct** (SETTLED,
`open-questions.md` "Default struct layout: field reordering"). Codegen — the *one* place struct
layout is computed (the `set_body` in `align_codegen_llvm`) — lays fields out in **descending
alignment** (ties keep declaration order, a stable sort) to eliminate padding, matching Rust's
default (`{ a: i8, b: i64, c: i8 }` → 16 bytes, not 24). Source access is by name, so the reorder is
invisible; codegen keeps a **logical→physical field-index map** (`field_perm[struct_id][logical]`)
that *every* field-index consumer routes through — struct-field GEPs (`field_path_ptr`,
`elem_field_ptr`, AoS `IndexFieldPtr`, `NullStructField`, `DropElemField`, the `drop_struct_fields`
walk), byte-offset sites (`offset_of_element` for `json.decode` field tables, `group_by`/dict key &
value offsets, `GatherColumnI64`), and the `soa` gather's struct-aggregate insert. `sizeof`/alignment
follow automatically because they read back the built LLVM struct type. A `layout(C)` struct uses the
**identity map** (declaration order, natural alignment, no reordering) — its byte layout is the
FFI / `raw` / `json`-encode / by-value boundary and must not move. `soa` *column* order stays in
declaration order (a separate, self-consistent column layout, independent of the AoS field order).

**SETTLED (`open-questions.md` "Memory layout — `soa<T>`"): the layout is chosen by an explicit type,
not by automatic whole-program inference.** `soa<T>` is a first-class columnar collection (peer to
`array<T>`); the compiler lowers field access / pipeline stages over it to per-column contiguous
storage (fields naturally SIMD-aligned, `align(N)` when needed — `draft.md` §3.4). A pipeline that
touches a subset of fields (`users.where(.active).pay.sum()`) then streams only those columns. The
choice is visible (predictable performance, "nothing hidden"); the *field-wise lowering under the
type* is the automatic part. Crossing a byte-layout boundary (FFI, `json` encode/decode, by-value)
**materializes to AoS explicitly**. (This closes the earlier "automatic decision vs. annotation"
question in favor of annotation.) Uses the `Layout::Soa` seam.

The column buffer is **column-major with per-column alignment padding**: column `j` begins at
`align_up(start_{j-1} + len*size_{j-1}, size_j)`, so mixed-width columns (`bool` then `i64`) stay
naturally aligned for any `len`. A column read is `Rvalue::IndexColumn`; a column write (during
construction) is `Stmt::StoreColumn` — both share one `soa_column_offset` codegen helper.
Construction is `.to_soa()`: `Rvalue::SoaAlloc` arena-bump-allocates the buffer (total size = the
offset walk to the last column + its `len*size`, aligned to the widest field), then a fused loop
scatters each AoS element's fields into their columns (`StoreColumn`), yielding the `{ptr,len}` view.
The allocation uses a checked mirror of the offset walk: negative counts, product/addition wrap,
and byte totals above the signed `i64` allocator ABI abort before allocation.
`s: soa<T> := json.decode(d)?` takes a separate direct-fill rail: the runtime first counts rows,
arena-allocates the final column layout, then parses values directly into their columns. There is no
AoS intermediate and no transpose. Primitive and zero-copy `str` columns are supported; owned and
nested columns remain deferred. `.to_soa()` itself still uses the transpose loop described above.

JSON field dispatch is O(1): codegen bakes a **compile-time perfect-hash table** from the (known)
field names (`build_phf` finds a collision-free seed + power-of-two size; emits a `[i32]`
slot→index global beside the descriptor table), and the runtime hashes each key to a slot + one
confirming name compare instead of a linear scan. `phf_len = 0` (empty/1-field, or no table found)
falls back to the scan, so it is a pure speedup. Both ends call the **one** canonical `wyhash` (the
shared `align_hash` crate — same hash as the `hash64` builtin), so the codegen-built table and the
runtime probe route a field name identically *by construction* (the paired pinned tests are now a
canary against an accidental algorithm edit, not the mechanism that keeps them in sync). (Known-schema field-skip decode is deferred — the perf is already had by declaring a
narrow struct, since unknown keys are skipped; see `open-questions.md`.)

---

## 3. Functions, CFG, cold path

- MIR `Function` → LLVM function. `Block` → LLVM basic block (nearly one-to-one).
- Every **Align-generated** function is marked **`nounwind`** (`mark_nounwind`): Align never unwinds
  (errors are `Result` values; a fatal fault `abort`s — see "Panic / unwinding" in
  `open-questions.md`), so this is always sound and lets LLVM drop exception edges / unwind tables and
  inline more aggressively. The external `align_rt_*` declarations are **not** marked (ordinary Rust
  fns). Pure-function `memory(none)`/`readonly` is *not* emitted — Align's purity is "no I/O effect"
  and permits allocation, so it doesn't imply LLVM `readonly` (deferred; `open-questions.md`).
- Terminator correspondence:

```text
Goto         br
Branch       conditional br
Return       ret
Unreachable  unreachable
```

Source `match`, `?`, and loops have already become `Branch`/`Goto` CFG in MIR. Calls,
`ParMapParallel`, and allocation are rvalues rather than terminators.

### cold path (error)
The failure edge of `?` (`04 §2.1`) is cold. In LLVM:

```text
- attach llvm.expect / branch weights to the br branching to err_bb, making the ok side fall-through
- place the body of err_bb at the function tail (or a cold section)
- lean toward noinline for calls on the failure path
```

This keeps the normal path's I-cache clean (`draft.md` §10).

### Every return edge carries the cleanup bit — including the cold one

Cold is not dead. The `?` failure edge is an ordinary **return edge**, so `04 §1`'s rule applies to
it unchanged: a `ReturnCleanupAbi::DynamicBit` function returns the value together with one
path-local cleanup bit, on every edge. `lower_try` instead lowered its `Err` edge to
`Term::Unreachable` whenever `lowered_drop_flag` produced no bit for the `?` operand, so the whole
propagation path dead-ended and the program took a SIGTRAP (exit 133) where it had to return `Err`:

```align
fn check(flag: bool) -> Result<(), i64> {
  if flag { return Ok(()) }
  return Err(9)
}
fn via_try(flag: bool) -> Result<string, i64> {
  s := "try".clone()
  check(flag)?          // Err edge: drop `s`, then trap instead of returning Err(9)
  return Ok(s)
}
```

The trigger is a **Copy-typed `?` operand inside a recursively-Move-returning function**, which is
an everyday shape: `via_try` returns `Result<string, …>` (`DynamicBit`) while `check` returns
`Result<(), i64>`, a type with no owned payload and therefore no ownership bit anywhere.

**A missing bit is a `false` bit, never a dead end.** No bit means the lowering knows of no
individually owned payload, and the `Ok` edge already acts on exactly that reading — it attaches a
drop flag to the unwrapped value only `if let Some(flag)`. For the `Err` edge the answer is not
merely conservative but exact: sema requires the operand's error type to *equal* the function's
(`03`; there is no `From` conversion, so `?` re-wraps the very payload it extracted, allocating
nothing), the propagated `Err(err)` therefore owns something only if the operand did, and an
operand whose type is not recursively Move cannot own anything.

`terminate_return` is now the **single authority** over the three return edges — the `?` `Err`
edge, the function-body fall-through tail, and `Stmt::Return` — which previously repeated the same
ABI match, and the same `(DynamicBit, None) => Term::Unreachable` arm, three times. It emits **no
`Unreachable` at all**. A valueless return from a `DynamicBit` function is malformed HIR (that ABI
is chosen exactly when the return type is recursively Move, so it is never `Unit`), and it lowers
to the plain void return, which codegen rejects by name — "dynamic-cleanup function returned no
value or cleanup bit". That **diagnoses** the malformed input; an `Unreachable` would have turned
the same input into a silent runtime trap, weakening the failure mode in the one place the old
code still had it.

The helper also carries the provenance half of the rule, for every edge rather than only the one
where it was first noticed. Each caller names the type whose ownership the bit describes — the
returned value's own type at an ordinary return, the `?` operand's type on a propagation edge —
and a `debug_assert` requires an owned source to supply a bit. A firing assert marks exactly the
shape the pre-fix compiler dead-ended in `Term::Unreachable`, so it can only report a program that
already trapped; every test binary is a debug build, so the whole owner suite carries the check
while release lowering keeps the defined `false` bit.

**Closure matrix (return-cleanup ABI × return edge × cleanup-bit provenance × Err reachability).**
`ReturnCleanupAbi` has exactly two variants (`None`, `DynamicBit`), and `None` returns
`Term::Return` on every edge and provenance — one row. Provenance is the complete set of sources
`lowered_drop_flag` consults: an SSA bit attached to the value (a direct or indirect `DynamicBit`
call, a control-flow join), a bound local's drop flag, the one recursion arm shared by the
`Option`/`Result` wrappers, `Try`, and `TaskGet`, a borrow-transparent scope's tail, sema's static
allocation provenance, and no source at all. `if` and `match` reach the join through the same
`control_result_slots` flag, so the join row probes one of them.

| return edge | provenance | probe | before | after |
|---|---|---|---|---|
| `?` `Err` | attached SSA bit (`DynamicBit` call) | `dyn_call` | `…, %bit` | unchanged |
| `?` `Err` | attached SSA bit (`if` join) | `join` | `…, %bit` | unchanged |
| `?` `Err` | bound Move-`Result` local's flag | `move_local` | `…, %bit` | unchanged |
| `?` `Err` | wrapper literal → sema static provenance | `wrapper` | `…, true` | unchanged |
| `?` `Err` | scope tail recursion over an owned operand | `move_scope` | `…, %bit` | unchanged |
| `?` `Err` | `task_group` tail recursion over an owned operand | `move_task_group` | `…, %bit` | unchanged |
| `?` `Err` | `Try` recursion (a nested `??`) | `nested_try` | `…, %bit` | unchanged |
| `?` `Err` | **none** — Copy operand, direct call | `copy_call` | `unreachable` | `…, false` |
| `?` `Err` | **none** — Copy operand, bound local | `copy_local` | `unreachable` | `…, false` |
| `?` `Err` | **none** — Copy operand in `{ }` | `copy_block` | `unreachable` | `…, false` |
| `?` `Err` | **none** — Copy operand in `unsafe` | `copy_unsafe` | `unreachable` | `…, false` |
| `?` `Err` | **none** — Copy operand in `arena` | `copy_arena` | `unreachable` | `…, false` |
| `?` `Err` | **none** — Copy operand in a named arena | `copy_named_arena` | `unreachable` | `…, false` |
| `?` `Err` | **none** — Copy operand in `task_group` | `copy_task_group` | `unreachable` | `…, false` |
| `?` `Err` | any, under `ReturnCleanupAbi::None` | `copy_abi` | `return v` | unchanged |
| tail / `Stmt::Return` | every provenance above | `move_res`, `mixed_res`, `nested_res` | `…, %bit` / `…, true` | unchanged |
| tail / `Stmt::Return` | none | — | `unreachable` | `…, false` |

(`…` abbreviates `return_with_cleanup v`.) The five borrow-transparent scope kinds each get their
own missing-bit cell rather than one representative, because they reach the bit through
`moved_drop_flag`'s recursion and `task_group` is deliberately not that recursion in
`temporary_drop_flag` — a difference this matrix must be able to see, on the owned side
(`move_task_group`) as well as the missing-bit side.

The `%bit` rows are checked as a **correspondence, not a spelling**: the forwarded SSA value must
be one this function defines as an ownership bit — a `DynamicBit` call's cleanup result (`-> %n`)
or a load of a drop-flag slot — so forwarding some other in-scope value fails the owner.

No valid program reaches the sibling sites' missing-bit cell, which is why it has no probe: a
returned expression's type *is* the `DynamicBit` return type, so it is recursively Move and every
shape that can produce it carries a bit (moving out of a `Move` array element or a borrowed place
is rejected earlier). The row exists because the arm existed; the shared helper gives it the same
`false` bit as its `?` sibling, and the shared `debug_assert` is what reports it if a future
provenance gap ever reaches it. The rows above it are not execution-only: `move_res`, `mixed_res`,
and `nested_res` return through the tail and `Stmt::Return` edges in the same fixture, and the
owner asserts their static-provenance bits structurally.

The `Err`-reachability axis is per cell and needs an execution witness, not a structural one: an
`Err` edge that is never taken produces the same exit code on both compilers, and the pre-fix
`unreachable` is invisible until control actually reaches it. Every probe therefore runs both ways
and contributes its `Ok` length and its `Err` code to one exit code.

Owners: `move_return_cleanup::try_err_edges_forward_a_cleanup_bit_for_every_operand_provenance`
(the provenance battery — MIR structure for every row, plus execution of both reachability states),
and the acceptance case `owned_temporaries::moved_slots_emit_no_known_null_destructor_calls`, whose
`via_try` fixture is the reported reproduction and which fails with no exit code (SIGTRAP) before
this fix.

---

## 4. allocation lowering

Materialize MIR's explicit `Alloc` (`04 §5`).

```text
Alloc(Arena(id), layout)   → pointer returned by align_rt_arena_alloc(arena_ptr, size, align)
arena block exit            → align_rt_arena_reset(arena_ptr)   (bulk, no individual free)
Alloc(Heap, layout)        → align_rt_heap_alloc(...)  / align_rt_heap_free at the Drop point
Alloc(Stack, layout)       → alloca
```

The arena pointer is acquired via the `align_rt_arena_begin()` equivalent at the arena block entry, and carried around as a block-scoped value (function argument/local). The detailed runtime ABI is in `06-runtime-std.md`.

---

## 5. Loops and vectorization (the crux of Align's performance)

MIR is already fused and carries the **width-agnostic** vectorizable properties of each
element-independent loop (`04 §4`) — it never fixes a vector width. **Choosing the width is the
backend's job, chosen per target.** The current, working form emits clean IR (contiguous access,
branchless `where`, `noalias`) and hands it to LLVM's `-O2` vectorizer, which picks the width from the
target: this is the right split, not a fallback — MIR stays portable and each target gets its own
strategy (fixed width + a scalar remainder on NEON/AVX; `<vscale x N x T>` + active-lane predication on
SVE/RVV). On a fixed-width target the loop lowers to:

```text
vector body   load <W x T> → VecOp/Mask → store. pointer advances by W
remainder     handle the leftover scalarly
```

```text
total := scores.sum_where(scores > 80);   (MIR: VecCmp + MaskedReduceAdd)
=>
loop:
  v   = load <W x f32>, p
  m   = fcmp ogt <W x f32> v, splat 80.0     ; <W x i1>
  sel = select <W x i1> m, v, zeroinitializer
  acc = fadd <W x f32> acc, sel
  p  += W
; reduce: llvm.vector.reduce.fadd(acc) + remainder
```

- **mask** → LLVM `<W x i1>` and `select` (branchless, `04 §4`).
- **dot / sum / min / max** → `llvm.vector.reduce.*`.
- **no-alias** (`out`, `03 §6`) → scoped `!alias.scope`/`!noalias` metadata on the `map_into` fused
  loop's source load and `dst` store (a slice is passed by value as `{ptr,len}`, so its buffer
  pointer is not a standalone param to carry a `noalias` *attribute* — the scoped metadata is the
  equivalent per-access form). One fresh domain + `in`/`out` scope pair per loop; gated on the
  sema-proven `dst`-disjoint-from-source precondition. Makes explicit to LLVM the basis for
  dependence-free vectorization — verified to drop the loop's runtime overlap guard at `-O2`.
- aligned load/store when already aligned.

### Choosing the width (a backend, per-target choice)
```text
explicit vecN<T>   N is fixed in the type → the LLVM vector width directly (the fixed escape hatch)
inferred loops     no width in MIR → the backend chooses it per target:
                     fixed-width ISA (AVX/NEON)  a portable per-arch baseline + a scalar remainder
                     scalable ISA (SVE/RVV)      <vscale x N x T> + active-lane predication (no fixed W)
```
**SETTLED (`open-questions.md` "Build targets & portability") — for fixed-width ISAs:** the default
targets a portable per-arch baseline (`x86-64-v2` / `armv8-a`, i.e. 128-bit); `--target-cpu native` /
higher baselines are opt-in. This keeps one binary runnable across a varied cloud/Docker fleet.
**Wide SIMD on that fleet comes from runtime CPU-feature dispatch in the library layer** (`06 §1`),
not from raising the generated-code baseline — one binary picks AVX2/NEON at runtime and falls back
safely. Runtime-multiversioning the generated loops themselves (an ifunc-style v2 + v3 selector) is a
possible future refinement, deferred. This is a *fixed-width-ISA* policy, not a universal 128-bit cap:
a scalable ISA is handled by predicated scalable codegen instead, which is why MIR stays width-agnostic
(`04 §4`).

> Status note: the default build now targets the **portable per-arch baseline** (`x86-64-v2` on
> amd64, `generic`/`armv8-a` on arm64) via `BuildTarget` in `align_codegen_llvm`; `--target-cpu
> native` opts into the host CPU. The backend still builds **scalar** IR and leans on the LLVM `-O2`
> pipeline (SLP / loop vectorizer) for the actual SIMD. Branchless `where` is implemented for the
> inactive-lane-safe reducing suffix: MIR folds predicates into a mask and emits identity-select for
> `sum`/`count` (`0`) and `min`/`max` (`+∞`/`−∞`). `min`/`max` further lower to the
> `select(cur `cmp` acc, cur, acc)` idiom (`llvm.{s,u}{min,max}` / `llvm.{min,max}imum`) so the whole
> loop is branch-free and vectorizes: e.g. `xs.where(p).min()` over a `slice<i32>` emits `pminsd`
> over a `pcmpgtd` mask on x86-64-v2 (verified via `objdump`; before, the per-element branch blocked
> it entirely). General callable suffixes/reducers and materializing terminals use real skip-branches:
> the former may trap or have effects, and the latter must not append a rejected element.
>
> **Correctness fix (2026-07-13):** reducing lowering used to speculate a reducer's callable and
> every stage after `where` on rejected elements. Pure does not imply total/non-trapping, so
> `where(false).map(divide_by_zero).sum()` aborted. Identity `select` now stays branchless only for
> field operations plus builtin `sum`/`count`/`min`/`max`; every general post-`where` callable is
> guarded and never executes on a rejected element.
> See [`12-pipeline-closure-memory-io-simd-audit.md` §3.1](12-pipeline-closure-memory-io-simd-audit.md#31-fixed-2026-07-13--where-guards-later-callables-and-callable-reducers).
> `maskN<T>` remains the explicit hand-written value mask, and `dot` has no masked pipeline form.

> **Why the identity-select shape matters beyond perf.** Selecting each reducer's identity for a
> masked-out lane (`min` → `+∞`, `max` → `−∞`, `dot` → `0`, matching
> `sum`/`count` → `0`) makes *every* reduction **predication-ready**: a masked-out lane contributes the
> identity and cannot change the result. Generic `reduce` is the one exception — its user-supplied
> function has no known identity (`init` is the starting accumulator, not an identity), so its
> computation is guarded. `any`/`all` predicates are likewise guarded. That distinction is what
> makes the form semantics-correct and still predication-ready for scalable tails (`04 §4`).

---

## 6. Strings, builder, const pool

- **string literals**: bytes as an LLVM global constant. A `str` value is `{ptr,len}`. Compile-time meta (len/hash/ascii, `draft.md` §12, `03`) is embedded as constants and used for `write_static` lengths and hash comparisons.
- **const string pool** (`draft.md` §12): identical literals/JSON field names/HTTP header names are coalesced into a single global (deduplication).
- **builder**: the runtime's mutable buffer. In the `template` desugaring (`04 §2.5`), `write_static` becomes memcpy + known length, and `write_value` becomes a per-type formatting call.

---

## 7. Parallelism (`ParMapParallel` / `ParMapReduce` → runtime)

MIR's dedicated direct-source `ParMapParallel` materializer (`04 §6`) goes to the runtime's
parallel-map API.

```text
ParMapParallel { src, func, stages, captures, capture_tys, elem_in, elem_out, work_weight }
  → synthesize one typed range kernel (context, start..<end)
  → align_rt_par_map(...) for map-only stages
  → align_rt_par_map_filter(..., count_kernel, scatter_kernel) for callable `where` stages
  → owned array<elem_out>

task_group → align_rt_tg_begin / tg_alloc / tg_register / tg_wait / tg_end

ParMapReduce { src, func, captures, capture_tys, elem_in, elem_out, work_weight }
  → synthesize one typed reducing range kernel (context, start..<end)
  → align_rt_par_map_reduce(capture_ctx, in_buf, count, in_stride, result_stride, work_weight, kernel)
  → integer result bits, narrowed to elem_out
```

The range kernel loops over typed input/output GEPs, loads Copy captures once from the immutable
call-scoped context, calls the Pure Align function directly, and stores each output. LLVM can inline
and vectorize that loop; the runtime invokes the function pointer once per coarse range, not once per
element. The worker-sendability gate requires every capture value to be transitively worker-Send.
Checked HIR follows concrete callable targets and their capture environments and rejects every
reachable `ArenaHandle` before MIR. MIR intentionally strips that analysis-local provenance, so
codegen adds a defensive rejection for a
direct `ArenaHandle` in handcrafted `ParMapParallel` and `ParMapReduce` MIR before context layout,
kernel/global publication or a runtime declaration/call; malformed nested function environments
belong to the earlier checked-HIR gate rather than an opaque MIR guess.
A chunk source is an owned array of borrowed `{ptr,len}` slice headers, so the same typed
loop passes one `slice<T>` value to the chunk function and the caller drops the header buffer after
the synchronous runtime call. For `ParMapReduce`, the typed loop keeps an integer accumulator, uses
plain wrapping addition, and stores one partial at the range output pointer; the runtime combines the
partials and returns the result bits. Primitive-scalar length-preserving `map` stages are emitted in
the same ordered range kernel. Callable primitive-scalar `where` stages emit a count kernel and a
scatter kernel; the runtime prefix-sums per-range survivor counts and passes each scatter range its
stable output offset. The Pure chain is intentionally evaluated in both passes. AoS projection and
field-filter stages, plus the compiler-recognised invariant `str.contains`
filter, use the stable range path. Richer string-search expressions, unsupported aggregate layouts,
and other unsupported staged forms use the sequential pipeline fallback.
`work_weight` is a bounded MIR hint (`1`, `2`, or `4`) materialized as an `i64` argument; it is
combined with the concrete input/output strides by the runtime and does not change the source or
language ABI.
The fused node covers a direct stage-free integer map from either a scalar/AoS source or a chunk
source. It still excludes floating-point sums, staged reductions, and arbitrary reducers. There is
no generic parallel-reduce lowering in the current surface. The ABI is in `06`.

---

## 8. Target, optimization, output

```text
- build the TargetMachine for the host (or a specified triple). obtain the data layout and reflect it in the §2 layout
- LLVM optimization: since fusion/vectorization is done on the Align side, leave the
  lower-level optimizations (instcombine, regalloc, peephole, etc.) to LLVM. don't duplicate high-level transforms
- output: object (.o). the driver links it with align_runtime into an executable (01/06)
- alignc emit-llvm outputs the IR as text (for verification/debug, 01)
```

`// OPEN:` how far to use the LLVM pass pipeline (a single O2-equivalent pass vs. selecting the necessary passes). Decide empirically within the range that does not conflict with Align's optimizations.

---

## 9. Debug info, panic

```text
- generate DWARF/CodeView line info from Span (align_span). introduce at least step-debug-capable level in stages across M
- traps such as divide-by-zero (03/draft §5): to a runtime abort (align_rt_panic). message + location
- overflow defaults to wrap, so no check is emitted (optionally insert a check in dev builds only)
```

---

## 10. Settled backend choices and remaining refinements

### Settled (M0; upgraded to LLVM 22 post-M13): inkwell / LLVM version and linking method
Use LLVM 22 via `inkwell 0.9` (feature `llvm22-1`), with `llvm-sys` 221. `llvm-sys` is pinned to
**dynamic linking** (`prefer-dynamic` feature + `LLVM_SYS_221_PREFER_DYNAMIC=1` in
`.cargo/config.toml`); `llvm-config-22 --shared-mode` still reports `shared`. Unlike the Debian
llvm-19 era (shared-only — no static components such as `libPolly.a`, so dynamic linking was
mandatory), the apt.llvm.org llvm-22 packages ship the static archives and Polly is no longer a
separate `--libs` component, so a static build would work; dynamic linking is kept deliberately (it
links smaller and matches the rustc-side LLVM). In M0 the generated `main` is the C entry (called by
crt0), and the driver links the object with `cc`. (History: M0 shipped on LLVM 19 / `llvm19-1` /
`LLVM_SYS_191_PREFER_DYNAMIC`; the LLVM 19 → 22 upgrade checkpoint landed after M13 — see
`07-roadmap.md`.)

```text
- the scope of multi-ISA support: the vector width is a backend, per-target choice (§5) — MIR stays width-agnostic (04 §4) — so the open part is how far to carry scalable-ISA (SVE/RVV) predicated codegen, not whether MIR fixes a W (common with 04 §9)
- the scope of adopting the LLVM optimization pipeline (non-overlap with Align's optimizations)
- by which M and how far to raise the precision of debug info
- linking: static runtime, and how far to depend on libc (linked with 06)
```

`Option`/`Result` use tagged aggregates, and SoA is selected by an explicit `soa<T>` type; neither
is an open backend decision.

## `http_upgrade` lowering (`pkg.ws`)

`http_upgrade` lowers as the same nullable one-pointer owned handle class as `http_stream`, with its
own Drop key. LLVM contains no WebSocket handshake, SHA-1, frame, mask, UTF-8, subprotocol, or close
semantics. It purely lowers the checked MIR operations to eleven typed runtime keys using existing
ABI shapes A24/A20/A120/A37/A04/A03/A62, reconstructs builtin Results, nulls moved sources, and
emits cleanup at the MIR-selected edges. `HttpRespondUpgrade` uses an alloca out slot initialized
null; only zero status loads/publishes the handle. Its runtime computes checked exact wire-head
length `H`, allocates one `H`-byte serialization plus the handle shell before fd transfer, and owns
their overlap with still-live builder storage. Read publishes buffer length only through the
runtime's successful return. The header queries and readiness getter hard-abort detectable malformed
native context/view shapes before reference or slice formation, and invalid token bytes before
table scanning, rather than mapping either class to zero/false.
On macOS/iOS, checked `SO_NOSIGPIPE` acceptance precedes request-context publication; failure closes
the accepted fd and returns its mapped error. Linux retains `MSG_NOSIGNAL`. Curated LLVM function
attributes remain the reused shapes' exact empty sets; the Rust C exports do not unwind across C,
but this capability adds no declaration-side `nounwind` and does not mutate a shared shape
fingerprint. rt-LTO
on/off, whole/per-unit, extern-collision, declaration/export, and malformed-MIR owners are fixed in
`pkg-design/ws.md`. A124 remains unused. This lowering is active.
