# Nested struct / enum fields — implementation record

> **Current status.** The core plan shipped: plain-data nesting, whole inner-struct values,
> string-owning nested fields with Drop, nested array element access/update, Move-struct arrays,
> owned collection fields used by the JSON model, and cross-module field types are implemented.
> The sections below preserve the original walls and slice rationale as a chronological
> implementation record: a `deferred` bullet inside an early slice may be closed by a later
> `DONE` follow-up. Current limits include partial moves through nested paths, nested SoA columns,
> Move-element extraction, and some non-primitive dynamic element-field writes.

This was the last broad language-modeling gap when the plan was written. At that time a struct
field had to be a primitive scalar or `str`, and field access was depth-1 (`local.field`). The plan
below lifted both restrictions so structs can model composite data.

## Original limitation (the three walls)

```align
Point { x: i64, y: i64 }
Line  { a: Point, b: Point }   // (1) rejected: "struct fields must be a primitive scalar or str for now"
l.a.x                          // (2) rejected: "field access is only supported on a local binding"
```

1. `is_field_ok` (`align_sema/src/lib.rs`) gates field types to scalar / `str`.
2. `place_local` matches only a bare local name, so `check_field_access` can't resolve a nested
   receiver (`l.a`).
3. Codegen builds the LLVM struct types passing an **empty** struct table to `abi_type` ("fields are
   scalars/str only, so the table is never consulted") — a nested `Struct(id)` field can't be typed.

The M1 model: a struct lives in its slot; `StoreField(slot, idx, scalar)` constructs it field by
field; a read is `IndexField` (GEP+load). Nesting generalizes the slot/GEP to a **field path**
(`[0, i0, i1, …]`).

## Design decisions (settle before building)

- **No non-`box` recursion.** Self/mutual struct recursion without a `box` indirection is forbidden
  (infinite layout). `is_field_ok` gains a cycle check (visiting-set) over the nested struct/enum
  graph. Same for enum payloads.
- **Place = root local + index path.** Generalize HIR/MIR `Field { base: LocalId, index }` to a
  field path `{ root: LocalId, path: Vec<u32> }`, mapping to a single GEP `[0, *path]`. Simpler for
  MIR / the flow analyses than a recursive `Field(Box<Expr>)` (they only inspect `root`).
- **Value-semantics scope is staged.** Slice 1 supports **leaf-scalar access only** (`l.a.x` read +
  write) plus nested-literal construction. Reading a whole inner struct as a value (`p := l.a`) and
  whole-inner assign (`l.a = pt`) are Slice 2.
- **Owned nested fields are staged.** Slice 1 forbids `str`/owned inside the nesting (all plain-data
  ⇒ the whole thing stays Copy / slot-resident, no Drop) — mirrors `enum_payload_ok`. Owned nested
  fields need struct Drop (Slice 3).
- **LLVM struct types in two phases.** Create all struct types as named **opaque** structs first,
  then `set_body` — so `abi_type` can map a `Struct(id)` field to `struct_types[id]`. (Acyclic ⇒ a
  topological sort would also work; opaque-then-body is the general form and survives future `box`
  recursion.)

## Slices

### Slice 1 — plain-data nested struct fields (the body, highest value) — DONE
A scalar-only struct can be a field of another struct; `l.a.x` reads/writes; `Line{a: Point{…}}`
constructs. No owned nesting, no whole-struct *value* reads. Landed as built below (struct-valued
field init from a local — `Line{a: p}` — and `l.a = Point{…}` literal assign also work, a free
down-payment on Slice 2). `crates/align_driver/tests/nested_structs.rs`.

- **sema**: `is_field_ok` allows `Ty::Struct(id)` (with cycle detection + recursive plain-data
  check). `place_local` → `place_path(e) -> Option<(LocalId, Vec<u32>, Ty)>` (bare local + recursive
  `recv.field`). `check_field_access` / `AssignField` take a path. `field_of` handles nested types.
- **HIR**: `Field { base, index }` → `Field { root: LocalId, path: Vec<u32>, ty }`; `Stmt::AssignField`
  carries `path`. The flow walks (effect / move / escape / finalize) only read `root` → small ripple.
- **MIR**: `StoreField(Slot, u32, op)` → `StoreField(Slot, Vec<u32>, op)`; `IndexField` path-aware. A
  nested literal `Line{a: Point{…}}` lowers **in place** (store each leaf scalar to its path; no temp
  slot for the inner struct).
- **codegen**: two-phase struct-type build (opaque + `set_body`); `abi_type`/`scalar_type` map
  `Ty::Struct(id)` → `struct_types[id]`; `StoreField`/`IndexField` emit a multi-index GEP `[0, *path]`.
- **tests**: construct `Line{a,b}`; read `l.a.x`; `mut l.a.x = v`; 3-level nesting; cyclic type
  rejected; owned-in-nested rejected (early sema error).
- **risk**: medium — the place-path generalization touches several flow walks (but `root`-based).

### Slice 2 — whole inner-struct read / assign — DONE (already working)
`p := l.a`, `l.a = pt`, struct-by-value params/returns. A struct value = a sub-struct memcpy (LLVM
aggregate load/store); safe for plain-data Copy. **Found already working** once Slice 1 generalized
`Field`/`Load`/`Store` to struct values — verified across the SysV by-value ABI (mixed-width, float,
and nested structs by value; returned-then-mutated; struct-to-struct assign). A `str`-bearing struct
by value copies the `{ptr,len}` and *leaks* (no Drop yet) but does not double-free — that's S3.
`crates/align_driver/tests/struct_by_value.rs`.

### Slice 3 — owned (`string`-bearing) nested fields + struct Drop — DONE
`User { name: string, addr: Address }`. A struct that (transitively) owns a heap buffer — a `string`
field, or a nested struct that does — becomes a **Move** type: it gets a recursive **Drop** (free each
owned field in declared order, recursing into nested Move-struct fields) and whole-struct move
semantics (return / pass / assign by value nulls the source so its exit Drop is a no-op — no
double-free). Landed as built below. `crates/align_driver/tests/owned_structs.rs`.

- **sema**: `struct_is_move(id)` (recursive over the acyclic field graph); `is_field_ok` allows
  `Ty::String`; pass 0b-2 relaxes the Slice-1 scalar-only nested gate to an **acyclicity-only** check
  (`struct_acyclic`) — a nested struct may now own a `string` (region tracking already flows through
  `StructLit`/`Field`, so a nested `str` *borrow* field is sound too). `is_owned_droppable` /
  `is_move_ty` / `ty_capture_is_move` include Move structs (so they join the drop set, use-after-move
  tracking, and the lambda-capture rejection). Reading an **owned field out** of a struct (`n := u.name`,
  a partial move) is deferred — a clean sema error.
- **soundness (the Move-vs-Copy seams)**: a Move struct must never be silently copied. Rejected:
  an **array** of a Move struct (`[User{…}]` / indexing → per-element drop; **lifted in Slice 4a**,
  below); a Move struct as an **`Option`/`Result`/sum-type payload** (the aggregate's drop frees a flat `{ptr,len}`,
  not a struct). `box`/`soa`/tuple payloads were already scalar/primitive-only.
- **MIR**: `null_moved_source` nulls a moved-out Move-struct slot; `DropFlagInit`/`Drop` already cover
  every `drop_local` (Move structs now qualify); struct-literal lowering stores each owned field
  operand into its slot path (moved in).
- **codegen**: `DropFlagInit` zeroes the whole struct aggregate; `Stmt::Drop` for a Move struct calls
  `drop_struct_fields` — GEP+free each `string` field's buffer, recurse into nested Move-struct fields
  (null-safe: a zeroed / moved-out struct frees `null`).
- **tests**: construct + drop; nested recursive drop; return / pass / assign by value (no double-free,
  verified under `MALLOC_CHECK_=3`); the unsupported-container rejections above; partial owned-field
  move-out rejected.
- **deferred at this slice:** owned **collection** (`array<T>`) fields. Later JSON/ownership slices
  added the supported scalar/struct array-field shapes and recursive drop; see
  `core-design/json.md`.

#### Follow-up (landed) — moving an owned `string` field out of a struct (partial move)
A depth-1 owned `string` field can now be **moved** out (`n := u.name`, `f(u.name)` by value,
`return u.name`): the buffer transfers to the new owner, the struct's slot field is nulled, and the
struct's recursive `Drop` frees null there — so the buffer is freed exactly once. The struct can no
longer move as a whole / the field be reused, but its other fields stay readable. `crates/align_driver/tests/owned_structs.rs`.

- **sema**: `MoveCheck`'s `Field` arm tracks per-field moves like a tuple (`MovedKey::Field` /
  `field_moved`): a consuming read of a depth-1 `string` field marks just that field moved (so a
  sibling Copy-field read still type-checks; `whole_moved` then blocks moving the struct as a whole).
- **MIR/codegen**: `null_moved_source` on a depth-1 `Field` of `string` type pushes the new
  `Stmt::NullStructField(slot, idx)`, which GEPs the field and stores a zeroed `{ptr,len}` — exactly
  the tuple `NullTupleField` shape, for a struct slot.
- **deferred**: moving a field out through a *nested path* (`n := u.addr.name`) or a whole nested
  **Move-struct** field (`a := u.addr` — needs the sub-struct nulled, not a single `{ptr,len}`).

#### Follow-up (landed) — borrowing an owned field out of a struct (read)
Slice 3 made owned struct fields constructible/writable, but their contents were **unreadable** (any
`u.name` read was rejected). A `string` (or nested-Move-struct → `string`) field can now be
**borrowed** as a zero-copy `str` view in any non-consuming position — `u.name.len()`, a `str`
argument, `io.stdout.write(u.name)`, a `s: str := u.name` binding. `crates/align_driver/tests/owned_structs.rs`.

- **sema**: `check_field_access` no longer rejects a Move-typed leaf — it returns the `Field` (typed
  `string`), and the existing `string`→`str` coercion (`check_str_init` → `StrBorrow`) / `Len` wraps
  it non-consuming. The borrow inherits the struct's region (`region_of(Field)` = the root's region)
  and is then `Frame`-capped by `StrBorrow`, so a view of a field cannot escape the struct's frame
  (returning it is an escape error). **Moving** the field out is still a partial move: `MoveCheck`'s
  `Field` arm now errors when a Move-typed field is read in a *consuming* position (bind / by-value
  arg / return). A borrow reaches that arm non-consuming (wrapped in `StrBorrow`/`Len`), so it passes.
- **no codegen change**: a `Field` load of a `{ptr,len}` `string` leaf already works; `StrBorrow` is
  identity. The borrowed buffer is freed once, by the struct's recursive `Drop` (no separate free).
- the partial *move* out (`n := u.name`) landed as its own follow-up (above).

#### Follow-up (landed) — reassigning an owned local drops the old value
A pre-existing gap for *all* owned types (`string`/`array<T>`/Move struct/box): `mut s := …; s = …`
silently overwrote the slot and **leaked** the old buffer. Now fixed (orthogonal to the nesting
slices, but it reuses the Slice-3 Drop machinery). `crates/align_driver/tests/reassign_drop.rs`.

- **sema**: `hir::Stmt::Assign` carries `drop_old: Cell<bool>`. `MoveCheck` is the authority on
  whether the RHS *moved the old value out* — it sets `drop_old` true iff the local is owned (Move)
  and the RHS did **not** consume it (the local did not transition live→moved while checking the
  value). A `Cell` lets the move walk, which holds only `&Stmt`, record the decision without a
  mutable HIR traversal. Because it uses the real move analysis (not a structural "does the RHS
  mention `s`?" heuristic), a *non-consuming borrow* of the local in the RHS — `s = make(s.len())` —
  still drops the old value (no residual leak).
- **MIR**: `Stmt::Assign` lowering computes the new value first (it may read the old), then, when
  `drop_old` and the local is in `drop_locals` (arena-owned excluded — the arena bulk-frees those),
  emits a `Drop` of the slot before the store. The slot holds a live buffer or null (a prior move /
  the entry `DropFlagInit`), so the drop frees once or no-ops `free(null)`. `s = f(s)` / `s = s`
  (RHS consumes the old value → ownership transferred) emit no reassign drop — no double-free.
- **deferred at this point in the sequence:** reassigning an owned **field** (`u.name = …`) /
  **element** (`a[i] = …`) still leaks
  the overwritten value (`AssignField`/`AssignIndex` don't yet drop-old). The degenerate self-assign
  `s = s` keeps leaking (the move machinery nulls the slot before the store). Separately, a local
  whose region is demoted by a self-borrowing reassign (`s = dup(s)` with `dup(v: str)`) drops out of
  the drop set entirely — a pre-existing conservative-region limitation, not this fix. The owned
  field/element replacement cases are closed later in Slice 4b.

### Slice 4 — arrays / soa × nesting
`arr[i].a.x` (struct-array element nested field) and a soa column over a nested field.

- **`arr[i].a.x` read — DONE.** `ElemField`'s `field: u32` became a `path: Vec<u32>`. Routing:
  `check_field_access` peels a `FieldAccess` spine bottoming at an `Index` (`peel_index_field_chain`)
  and hands the whole name path to `check_index_field`, which resolves it through the (nested) element
  struct (each non-final field must be a struct). MIR loads the **first** field via the existing
  single-field seam (`lower_field_access` — the pipeline path is untouched), materializes that
  sub-struct to a temp slot, and projects the remainder with the ordinary slot-field GEP
  (`Rvalue::Field`). Works for fixed and dynamic (`{ptr,len}`) struct arrays, any depth.
  `crates/align_driver/tests/struct_index.rs`.
- **`arr[i].a.x = v` write — DONE.** The symmetric write counterpart of the read: `StoreElemField`'s
  `field: u32`, `StoreElemFieldPtr.field`, `DropElemField`'s field, and `Place::ElemField.field` all
  became a `path: Vec<u32>` — the same generalization Slice 1 did to the local-field-path `StoreField`.
  Sema's write-place builder reuses `peel_index_field_chain` (the read side's) to resolve the whole
  path through the nested element struct (each non-final field must be a struct); the leaf value
  restriction is unchanged from the depth-1 write (a fixed `array<Struct>` leaf may be a scalar or an
  owned `string` with drop-of-old; a dynamic `array<Struct>` leaf stays primitive-scalar). Codegen
  emits a single multi-index GEP per store (`[0, index, *pfield(path)]` fixed, `[index, *pfield(path)]`
  dynamic; `phys_field_indices` maps each logical segment to its physical slot under #307 reordering),
  and `DropElemField` frees the old buffer for a nested owned `string` leaf. The flow walks
  (effect / `MoveCheck` / `EscapeCheck` / drop) read only `base`/`index`/`value` of `AssignElemField`,
  so the path change is transparent to them. Fixed + dynamic both path-generalized (symmetric); the
  dynamic-nested case has no source constructor today (json decode is scalar/str only), so it is
  exercised at depth-1 by the existing tests and is dead-but-symmetric at depth-2+.
  `crates/align_driver/tests/struct_index.rs`, `owned_structs_arrays.rs`, plus the wave-2 differential
  fuzzer (`fuzz_differential.rs`, now array-rooting the nested tower half the time).
- **arrays of Move structs — Slice 4a DONE** (PR #279). A fixed array of a Move struct
  (`[User{name: string}]`) is now allowed: `is_owned_droppable` includes a Move `StructArray`, so the
  slot is null-initialised + drop-scheduled; codegen's `Stmt::Drop` on a `StructArray(sid, n)` frees
  each element's owned fields via `drop_struct_fields` (unrolled), and `DropFlagInit` zeroes the whole
  `[N x %Struct]`. Verified in LLVM (one free per element's owned buffer — no leak / double-free).
  Construction + scalar-field read supported. `crates/align_driver/tests/owned_structs_arrays.rs`.
  - **Slice 4b DONE** (PRs #281–#283): **mutable** Move-struct arrays. **4b-1** (`us[i] = newStruct`,
    element replace): new MIR `Stmt::DropElem` frees the old element's owned fields before the store,
    the RHS's moved source is nulled; whole-array reassign (`us = …`) is cleanly rejected (array values
    aren't materialized). **4b-2** (`us[i].name` read): an owned `string` field of an element reads as a
    borrowed **`str` view** (region-tied to the array — a Move-struct array is `Frame`-region since its
    heap buffers die at function exit, so a view can't escape; `drop_locals` drops any non-`Arena`
    owned local). `.clone()` is the owned-copy escape hatch. **4b-3** (`us[i].name = new` / `u.name =
    new`, owned-field reassign): new MIR `Stmt::DropElemField` + an `AssignField` drop-of-old free the
    overwritten `string` before the store (Slice 3 only dropped on *whole*-struct reassign — this closed
    a pre-existing field-level leak too). All verified in LLVM (drop-of-old + exit free, no leak /
    double-free). `crates/align_driver/tests/owned_structs_arrays.rs`, `crates/align_driver/tests/reassign_drop.rs`.
  - **still deferred** (hard, with `.clone()` workarounds): moving an owned field *out* of an element by
    value (`n: string := us[i].name` — needs per-element runtime drop flags for a dynamic index); whole-
    array move (return / pass — array materialization).
- **deferred**: a soa column over a *nested* field (the nested-soa-column layout is a design choice —
  a soa column stays scalar, so a nested soa element-field write is rejected in sema as "field is not
  a struct"). A whole *Move-struct* leaf element-field write (`arr[i].inner = MoveStruct{…}`) still
  leaks the overwritten value — a pre-existing depth-1 gap (a fixed-array element-field write has no
  Move-struct-leaf drop-of-old), carried unchanged into the nested path, not introduced here.

### Slice 5 — cross-module field types (`f: other.T`) — DONE
The module B3 leftover. A struct field, enum payload, or generic-template member may name a `pub`
type exported by an imported module (`field: geom.Point`); reaches only `pub` types of `import`ed
modules — the same visibility rule as functions. `crates/align_driver/tests/cross_module_types.rs`.

- **sema**: the resolver already handled `mod.Type` in function signatures / `let`s; the gap was that
  the type-declaration passes (0b struct fields, 0c enum payloads, generic templates) resolved with
  `no_imports` in scope (a deliberate Slice-1 stub). Now a per-module `imports_by_module` map (built
  before pass 0b, resolution-only — the authoritative import validation stays in the module-table
  pass) is threaded into those passes' `TyCx`, so a qualified field/payload type resolves against the
  declaring module's imports. An imported Move struct as a field makes the outer a Move type as usual
  (its recursive Drop crosses the boundary).
- **no MIR/codegen change**: types are interned to a global id in pass 0a, so a cross-module field is
  byte-identical to a same-module one downstream.
- **risk**: low (plumbing) — confirmed: the full suite is unchanged and the only new surface is the
  resolution context.

## Order

Slice 1 first, as its own PR (the body — unblocks composite data). Slices 2 and 3 each independent;
**Slice 3 (owned + Drop) is the highest risk → fresh context**. Slices 4 and 5 follow. Per the
mandatory workflow, reflect the gemini-code-assist review before merging each PR. The cycle check and
owned-nested rejection must be early sema errors.
