# Nested struct / enum fields — implementation plan

The one remaining **language** gap (casts, modules, constants, bitwise are all done). Today a struct
field must be a primitive scalar or `str`, and field access is depth-1 (`local.field`). This plan
lifts both restrictions so structs can model composite data.

## Current limitation (the three walls)

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

### Slice 3 — owned (str/array) nested fields + struct Drop
`User { name: str, addr: Address }`. Recursive struct **Drop** (drop owned fields in order) + move /
null-flag along the field path; extends the M3 box-drop machinery to structs. Risk: **high**
(move/drop correctness, the double-free class fixed in #175). Independent PR, fresh context.

### Slice 4 — arrays / soa × nesting
`arr[i].a.x` (struct-array element nested field) and a soa column over a nested field. Extends
`StoreElemField` / `IndexFieldPtr` / soa offset math to field paths. Risk: medium–high (nested soa
column layout is a design choice).

### Slice 5 — cross-module field types (`f: other.T`)
The module B3 leftover, unblocked once nesting exists. Thread module qualification through field-type
resolution. Risk: low (plumbing only).

## Order

Slice 1 first, as its own PR (the body — unblocks composite data). Slices 2 and 3 each independent;
**Slice 3 (owned + Drop) is the highest risk → fresh context**. Slices 4 and 5 follow. Per the
mandatory workflow, reflect the gemini-code-assist review before merging each PR. The cycle check and
owned-nested rejection must be early sema errors.
