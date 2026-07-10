//! MIR: backend-agnostic intermediate representation (`docs/impl/04-mir.md`).
//!
//! Align's semantics (desugaring, fusion, SIMD-ization, arena) are settled here, and
//! `MIR -> LLVM` is restricted to pure lowering. Allocation / error paths / parallel
//! units remain explicit nodes ("nothing hidden").
//!
//! M1 model: each function is a CFG of basic blocks. Named locals (params + `let`) are
//! addressable **slots** (lowered to allocas), read via `Load` and written via `Store`;
//! expression temporaries are SSA-like [`ValueId`]s. `if` becomes branches + blocks,
//! using a result slot when it produces a value. fusion/SIMD/arena arrive with their
//! features.

use align_ast::{BinOp, UnOp};
use align_sema::{hir, payload_is_move, struct_is_move, FloatTy, IntTy, Layout, Ty};

pub mod print;

/// SSA-like temporary value (defined once).
pub type ValueId = u32;
/// Memory slot (a local variable; lowered to an alloca).
pub type Slot = u32;
pub type BlockId = u32;

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Function>,
    /// Foreign (`extern "C"`) declarations, passed through from HIR unchanged; codegen emits an
    /// external LLVM declaration for each, keyed by the C symbol so a `Rvalue::Call` resolves.
    pub externs: Vec<hir::ExternFn>,
    /// External libraries to link (`-l<name>`), passed through from HIR; consumed by the driver.
    pub link_libs: Vec<String>,
    /// Struct layouts, indexed by the id in [`Ty::Struct`]; codegen builds LLVM struct
    /// types from these.
    pub structs: Vec<hir::StructDef>,
    /// Sum-type layouts, indexed by the id in [`Ty::Enum`]; codegen builds the tagged struct
    /// `{ i32 tag, … }` from each (variant payloads + `field_base`).
    pub enums: Vec<hir::EnumDef>,
    /// Tuple layouts, indexed by the id in [`Ty::Tuple`]; codegen builds an anonymous LLVM
    /// struct type from each element list.
    pub tuples: Vec<hir::TupleDef>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    /// Slots holding the incoming parameters, in order.
    pub params: Vec<Slot>,
    pub ret: Ty,
    /// Type of every slot, indexed by [`Slot`].
    pub slots: Vec<Ty>,
    /// Declared over-alignment of every slot (bytes, a validated power of two), indexed by
    /// [`Slot`]; `None` = the type's natural alignment. Set for an `align(N) data := [...]`
    /// binding (codegen over-aligns the alloca); temporary slots are always `None`.
    pub slot_align: Vec<Option<u32>>,
    /// Type of every temporary, indexed by [`ValueId`].
    pub value_tys: Vec<Ty>,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
}

impl Function {
    /// The type produced by an operand.
    pub fn operand_ty(&self, op: &Operand) -> Ty {
        match op {
            Operand::Const(Const::Int(_, ty)) => *ty,
            Operand::Const(Const::Float(_, ty)) => *ty,
            Operand::Const(Const::Char(_)) => Ty::Char,
            Operand::Const(Const::Bool(_)) => Ty::Bool,
            Operand::Const(Const::Unit) => Ty::Unit,
            Operand::Value(v) => self.value_tys[*v as usize],
            Operand::Arg(i) => self.slots[self.params[*i as usize] as usize],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    /// `v = rvalue` (a temporary). A `Unit`-typed rvalue (e.g. a void call) has no value.
    Let(ValueId, Rvalue),
    /// `slot <- operand`.
    Store(Slot, Operand),
    /// `slot.field <- operand` (struct field store; `slot` holds a struct).
    /// Store `Operand` into the (possibly nested) field of a struct slot addressed by the index
    /// `path` (length ≥ 1) — a GEP `[0, *path]`. A single-element path is a direct field.
    StoreField(Slot, Vec<u32>, Operand),
    /// `slot[index] <- value` (array element store).
    StoreIndex(Slot, Operand, Operand),
    /// `ptr[index] <- value` — store into a raw element pointer (the buffer of an owned
    /// `array<T>` being filled). The element type comes from `value`.
    PtrStore(Operand, Operand, Operand),
    /// `ptr[index] <- value`, exactly like [`Stmt::PtrStore`], but tagged with the `map_into` loop's
    /// **`out` alias scope** (`u32` = the loop's scope id) so codegen attaches `!alias.scope !{out}`
    /// / `!noalias !{in}`. Emitted only for the `dst` store of a `map_into` loop whose source is a
    /// slice with a proven-disjoint root (sema's alias gate), so the vectorizer may drop its runtime
    /// overlap guard. See [`Rvalue::SliceIndexNoalias`] (the matching source load).
    PtrStoreNoalias { ptr: Operand, index: Operand, value: Operand, scope: u32 },
    /// `s.store(i, v)` — store the `n` lanes of vector `value` into a `slice<T>` (`{ptr,len}`) at
    /// `index..index+n` (M6). Codegen GEPs the slice buffer to `&buf[index]` and emits a `<n x T>`
    /// store at the element alignment. Bounds are checked before this statement is emitted.
    VecStore { slice: Operand, index: Operand, value: Operand, elem: Ty, n: u32 },
    /// `slot[index].f0.f1.… <- value` (struct-array element nested-field store). The field `path`
    /// (length ≥ 1) walks the element struct to the leaf being written; codegen GEPs
    /// `[0, index, *pfield(path)]` into the `[N x %Struct]` slot (each level through the
    /// logical→physical `pfield` map). A depth-1 path is the plain element-field store.
    StoreElemField(Slot, Operand, Vec<u32>, Operand),
    /// `base[index].f0.f1.… <- value` for a `{ptr,len}` view of an owned, dynamic `array<Struct>`
    /// (`DynStructArray`). The write dual of [`Rvalue::IndexFieldPtr`]: extract the buffer pointer
    /// from `base` and GEP `%Struct, ptr, index, *field-path` (through the logical→physical `pfield`
    /// map). `value` is a scalar (POD) leaf — sema gates `str`/owned fields off, since a
    /// pointer-based per-element drop of the overwritten field is not modeled. Bounds are checked
    /// (by [`Stmt`]s emitted before this one) via the loaded view length.
    StoreElemFieldPtr { base: Operand, index: Operand, path: Vec<u32>, struct_id: u32, value: Operand },
    /// Store `value` into column `field` at row `index` of a `soa<Struct>` column-major buffer
    /// `base` (the [`Rvalue::SoaAlloc`] base pointer; `len` rows). The write counterpart of
    /// [`Rvalue::IndexColumn`] — codegen reuses its per-column `align_up` offset chain. Used by
    /// `to_soa` to scatter each AoS element's fields into their columns.
    StoreColumn { base: Operand, len: Operand, index: Operand, field: u32, struct_id: u32, value: Operand },
    /// End an arena, freeing all its allocations (the operand is the arena handle).
    ArenaEnd(Operand),
    /// `raw.free(p)` (unsafe): free a `raw` pointer from [`Rvalue::RawAlloc`]. Side-effecting, unit.
    RawFree(Operand),
    /// `raw.store(p, offset, v)` (unsafe): write the primitive scalar `value` at `ptr + offset` bytes.
    RawStore { ptr: Operand, offset: Operand, value: Operand },
    /// Run all deferred tasks of a `task_group` and clear the list (`wait()`). Operand = the
    /// task-group handle. ④b-1 runs them sequentially; ④b-2 joins threads.
    TgWait(Operand),
    /// End a `task_group`, freeing its region (the operand is the task-group handle).
    TgEnd(Operand),
    /// Null-initialise an owned-array slot (`{null, 0}`) so a later [`Stmt::Drop`] on a path
    /// that never allocated is a no-op (MMv2 slice 4 drop-flag-via-null-slot).
    DropFlagInit(Slot),
    /// Null one owned field (`{null, 0}`) of a tuple slot, after a partial field move (`a := t.0`)
    /// took its buffer — so the tuple's exit `Drop` frees null there, not the buffer now owned by
    /// the new binding. The other fields are untouched.
    NullTupleField(Slot, u32),
    /// Null one owned `string` field (`{null, 0}`) of a struct slot, after a partial field move
    /// (`n := u.name`) took its buffer — so the struct's recursive `Drop` frees null there, not the
    /// buffer now owned by the new binding. Depth-1 (a direct field of the slot's struct); the other
    /// fields are untouched.
    NullStructField(Slot, u32),
    /// Drop a free-standing owned `array<T>` slot: free its buffer (null-safe).
    Drop(Slot),
    /// Drop element `index` of a fixed Move-struct array slot: free that element's owned fields
    /// (recursively), before it is overwritten by a whole-element store (`us[i] = new`, Slice 4b).
    /// `u32` is the element struct id. Null-safe (a moved/unwritten element reads nulls).
    DropElem(Slot, Operand, u32),
    /// Drop one owned `string` leaf field (the `Vec<u32>` = field path, length ≥ 1) of element
    /// `index` of a fixed struct-array slot: free that field's buffer before it is overwritten by an
    /// element-field store (`us[i].name = new` / `us[i].addr.name = new`, Slice 4b). Null-safe.
    DropElemField(Slot, Operand, Vec<u32>),
    /// Free the buffer of a free-standing owned `array<T>` *value* (a `{ptr,len}` operand that
    /// is not backed by a slot — an unbound `.to_array()` temporary consumed in place). Used to
    /// free the materialized buffer right after the loop that consumes it (null-safe).
    DropValue(Operand),
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    Use(Operand),
    Load(Slot),
    Un(UnOp, Operand),
    /// `operand as to` — an explicit numeric/char conversion. `from`/`to` are concrete primitive
    /// scalars (int / float / char); codegen picks truncate / sign-or-zero-extend (int→int),
    /// `sitofp`/`uitofp` (int→float), `fpext`/`fptrunc` (float→float), or the saturating
    /// `fptosi`/`fptoui` (float→int, no UB on overflow / NaN).
    Cast { operand: Operand, from: Ty, to: Ty },
    Bin(BinOp, Operand, Operand),
    /// Explicit-overflow integer arithmetic (`core.math`): `op` is `Add`/`Sub`/`Mul` on the
    /// integer type `int_ty`. `Saturating` → the clamped result (`int_ty`); `Checked` → an
    /// `Option<int_ty>` (`None` on overflow). Lowers to the LLVM `{s,u}OP.sat` / `{s,u}OP.with.overflow`
    /// intrinsics (signedness from `int_ty`).
    IntArith { op: BinOp, mode: align_sema::ArithMode, int_ty: Ty, a: Operand, b: Operand },
    /// A scalar math builtin (`core.math`): `abs` (1 operand) / `min` / `max` (2). `ty` is the
    /// numeric operand/result type; lowers to the matching LLVM intrinsic (signedness/float from `ty`).
    MathOp { fn_: align_sema::MathFn, ty: Ty, operands: Vec<Operand> },
    Call(String, Vec<Operand>),
    /// The address of a top-level function as a value (`Ty::Fn`) — a function pointer.
    FnAddr(String),
    /// A capturing closure value: the lifted function `lifted` (which takes the captures as
    /// trailing parameters) plus the captured values. Codegen copies the captures into a
    /// frame-local environment and builds `{ thunk_ptr, env_ptr }`, where the thunk unpacks the
    /// env and forwards to `lifted`. `capture_tys` give the env layout.
    Closure { lifted: String, captures: Vec<Operand>, capture_tys: Vec<Ty> },
    /// An indirect call through a function-value `callee` (a `Ty::Fn` pointer). `param_tys`/`ret_ty`
    /// give codegen the LLVM function type for the indirect `call` (taken from the checked args /
    /// result type — no signature table needed).
    CallIndirect { callee: Operand, args: Vec<Operand>, param_tys: Vec<Ty>, ret_ty: Ty },
    /// Load a (possibly nested) field from the struct in `slot`, addressed by the index `path`
    /// (length ≥ 1) — a GEP `[0, *path]` then a load.
    Field(Slot, Vec<u32>),
    /// `cond ? a : b` — branchless select (LLVM `select`). `a`/`b` share a type; `cond` is `bool`.
    /// Used for branchless `where` reductions (`acc += cond ? value : identity`).
    Select { cond: Operand, a: Operand, b: Operand },
    /// Project one column of a `soa<Struct>` value in `slot` (a `{ ptr, len }` column-major buffer)
    /// as the `field`-th column's `slice<FieldTy>` — `{ ptr + len * prefix_bytes, len }`, where
    /// `prefix_bytes` (the sizes of the preceding fields) is computed in codegen from `struct_id`.
    SoaColumn { base: Slot, struct_id: u32, field: u32 },
    /// `Some(operand)` — build an `Option` aggregate (tag = Some).
    OptionSome(Operand),
    /// `None` — build an `Option` aggregate (tag = None); the type is the value's.
    OptionNone,
    /// Whether an `Option` operand is `Some` (its tag).
    OptionIsSome(Operand),
    /// The payload of an `Option` operand (valid only when it is `Some`).
    OptionUnwrap(Operand),
    /// `Ok(operand)` — build a `Result` aggregate (tag = Ok); the type is the value's.
    ResultOk(Operand),
    /// `Err(operand)` — build a `Result` aggregate (tag = Err); the type is the value's.
    ResultErr(Operand),
    /// Whether a `Result` operand is `Ok` (its tag).
    ResultIsOk(Operand),
    /// The ok payload of a `Result` operand (valid only when `Ok`).
    ResultUnwrapOk(Operand),
    /// The err payload of a `Result` operand (valid only when `Err`).
    ResultUnwrapErr(Operand),
    /// `Type.Variant(payload…)` — build a sum-type aggregate `{ i32 tag, … }`: store the variant
    /// tag in field 0 and each payload operand in this variant's fields.
    MakeEnum { enum_id: u32, variant: u32, payload: Vec<Operand> },
    /// Build the builtin `Error` aggregate `{ i32 tag, i32 code }` from **runtime** `tag`/`code`
    /// operands — the one way a std runtime errno-status becomes an `Error` ([`make_error_from_status`]).
    /// Unlike [`Self::MakeEnum`] (a compile-time variant), the tag is computed at runtime, so the
    /// category (`NotFound`/`Invalid`/`Denied`) vs `Code(errno)` is selected branchlessly.
    MakeError { enum_id: u32, tag: Operand, code: Operand },
    /// Whether a sum-type operand's tag equals `variant` (the `match`-arm test).
    EnumTagEq { enum_id: u32, scrutinee: Operand, variant: u32 },
    /// The `slot`-th payload field of a sum-type operand for `variant` (valid only on that variant).
    EnumPayload { enum_id: u32, variant: u32, slot: u32, operand: Operand },
    /// Open a new arena; the value is its handle.
    ArenaBegin,
    /// Open a `task_group`; the value is its handle (a `*TaskGroup`).
    TgBegin,
    /// Register a deferred task (`spawn`): snapshot the closure's captures into a fresh env in the
    /// task-group region, allocate the result slot there, and register the task. Yields the slot
    /// pointer (the `Task<R>` handle). `tg` = the task-group handle, `closure` = the `{fn, env}`
    /// value, `capture_tys` give the env layout (empty = non-capturing), `r` = the result scalar.
    SpawnTask { tg: Operand, closure: Operand, capture_tys: Vec<Ty>, r: Ty, fallible: bool },
    /// `wait()` as a value: join the task_group and yield its outcome. `fallible` → build
    /// `Result<(), Error>` from the runtime's first error code (`Ok(())` if `0`, else `Err(code)`);
    /// otherwise yields `()`.
    TgWaitResult { tg: Operand, fallible: bool },
    /// `heap.new(init)` in an arena: bump-allocate, store `init`, yield the `box` pointer.
    /// First operand is the arena handle, second is the initial value.
    HeapAlloc(Operand, Operand),
    /// `raw.alloc(size)` (unsafe): flat-heap-allocate `size` bytes, yield a `raw` byte pointer.
    /// Manually managed (freed by [`Stmt::RawFree`]); no arena handle, no auto-drop.
    RawAlloc(Operand),
    /// `raw.load(p, offset)` (unsafe): read the primitive `scalar` at `ptr + offset` bytes.
    RawLoad { ptr: Operand, offset: Operand, scalar: align_sema::Scalar },
    /// `raw.offset(p, n)` (unsafe): a new `raw` pointer `ptr + offset` bytes (pointer arithmetic).
    RawOffset { ptr: Operand, offset: Operand },
    /// Read (copy) the value out of a `box` operand.
    BoxGet(Operand),
    /// Deep-copy a `box` into a fresh allocation. First operand is the arena handle,
    /// second is the source box.
    BoxClone(Operand, Operand),
    /// `slot[index]` — load an array element.
    Index(Slot, Operand),
    /// `slot[index].field` — load a field of a struct-array element.
    IndexField(Slot, Operand, u32),
    /// Build a `vecN<T>` register value `<n x elem>` from its lane operands — an `insertelement`
    /// chain over a poison vector (M6). `elem`/`n` give the vector type.
    MakeVec { elems: Vec<Operand>, elem: Ty, n: u32 },
    /// Read lane `lane` of a vector operand (`extractelement`); the result is the element `elem`.
    VecExtract { vec: Operand, lane: u32, elem: Ty },
    /// Write `value` into lane `lane` of vector `vec` (`insertelement`), yielding the new vector
    /// (M6 `v[lane] = x`, which then re-stores into the vector local).
    VecInsert { vec: Operand, value: Operand, lane: u32 },
    /// `vec.sum_where(mask)` — masked horizontal sum (M6): `select(mask, vec, 0)` then add all `n`
    /// lanes, yielding the element scalar `elem`.
    VecSumWhere { vec: Operand, mask: Operand, elem: Ty, n: u32 },
    /// `dot(a, b)` — the dot product of two `vecN<T>` (M6): multiply lane-wise then add all `n`
    /// lanes, yielding the element scalar `elem`.
    VecDot { a: Operand, b: Operand, elem: Ty, n: u32 },
    /// `v.min()` / `v.max()` — the horizontal min/max of a `vecN<T>` (M6): fold the `n` lanes with
    /// the scalar min/max intrinsic, yielding the element scalar `elem`. `max` selects max vs min.
    VecMinMax { vec: Operand, elem: Ty, n: u32, max: bool },
    /// `v.sum()` — the horizontal sum of a `vecN<T>` (M6): add all `n` lanes, yielding the element
    /// scalar `elem` (the unmasked sibling of [`VecSumWhere`]).
    VecSum { vec: Operand, elem: Ty, n: u32 },
    /// Reduce a `mask` (`<N x i1>`) to a scalar `bool` that is true iff **any** lane is set
    /// (an OR-fold of the `n` lanes). Used by the vector `/`/`%` divisor guard:
    /// `any(divisor == 0)` → abort. Yields `Ty::Bool`.
    MaskAny { mask: Operand, n: u32 },
    /// `s.load(i)` — load `n` consecutive elements of a `slice<T>` (`{ptr,len}`) starting at `index`
    /// into a `<n x T>` vector (M6). Codegen GEPs `&buf[index]` and emits a `<n x T>` load. `align`
    /// is a *statically proven* load alignment in bytes (`Some(N)` only when the slice is a whole
    /// borrow of an `align(N)` binding and the address is a multiple of `N` — see
    /// `proven_vec_load_align`); `None` falls back to the element alignment. An over-stated `align`
    /// would be UB, so it defaults conservatively. Bounds are checked before this rvalue.
    VecLoad { slice: Operand, index: Operand, elem: Ty, n: u32, align: Option<u32> },
    /// `base[index].field` for a `{ptr,len}` view of struct `struct_id` (an owned, dynamic
    /// `array<Struct>`, MMv2 slice 8d-2). Like [`IndexField`] but addressed through the loaded
    /// buffer pointer (`getelementptr %Struct, ptr, index, field`) rather than a stack slot, so a
    /// fused pipeline (`users.where(.active).score.sum()`) can run over a runtime-length AoS.
    IndexFieldPtr { base: Operand, index: Operand, field: u32, struct_id: u32 },
    /// `base.field[index]` for a `soa<Struct>` view: `base` is the `{ptr,len}` column-major buffer,
    /// so column `field` begins at `ptr + len*prefix_bytes(field)` and element `index` is
    /// `column_base + index*field_size`. The SoA counterpart of [`Rvalue::IndexFieldPtr`] — a scan
    /// reads only the columns it touches.
    IndexColumn { base: Operand, index: Operand, field: u32, struct_id: u32 },
    /// `s[index]` — gather a **whole** `struct_id` value from a `soa<Struct>` (`{ptr,len}`
    /// column-major view) at `index`: load every column's element and build the struct aggregate (M6).
    /// The soa is primitive-only, so the gather copies — the result is a free Copy value.
    SoaGather { base: Operand, index: Operand, struct_id: u32 },
    /// `base[index]` — load a **whole** struct element of `struct_id` from a `{ptr,len}` view of
    /// an owned, dynamic `array<Struct>` (GEP `%Struct, ptr, index`, then load the aggregate). The
    /// field-less analogue of [`Rvalue::IndexFieldPtr`]; emitted by `map(f)` whose `f` consumes a
    /// struct element by value (a fixed stack `array<Struct>` uses [`Rvalue::Index`] instead).
    IndexPtr { base: Operand, index: Operand, struct_id: u32 },
    /// `(e0, e1, ...)` — build a tuple aggregate value of `tuple_id` from its element operands
    /// (an anonymous LLVM struct, by value). The whole-value analogue of a struct literal.
    MakeTuple { tuple_id: u32, elems: Vec<Operand> },
    /// `recv.N` — extract element `index` from a tuple value (by value).
    TupleIndex { tuple: Operand, index: u32 },
    /// Borrow array `slot` (length `n`) as a slice value `{ &slot[0], n }`.
    MakeSlice(Slot, i128),
    /// Bump-allocate `count` elements of type `elem` in the arena `handle`; yields the
    /// element pointer (used to build an owned `array<T>` via [`Rvalue::MakeDynArray`]).
    ArenaAlloc { handle: Operand, count: Operand, elem: Ty },
    /// Heap-allocate `count` elements of type `elem` (free-standing owned array, outside any
    /// arena). Yields the element pointer; freed by a later [`Stmt::Drop`].
    HeapAllocBuf { count: Operand, elem: Ty },
    /// Bump-allocate the **column-major buffer** for a `soa<Struct>` of `len` rows in the arena
    /// `handle`; yields the buffer base pointer. The total size is the end of the last column —
    /// codegen walks the same per-column `align_up` offset chain as [`Rvalue::IndexColumn`] from
    /// `struct_id`'s field sizes (`to_soa`).
    SoaAlloc { handle: Operand, len: Operand, struct_id: u32 },
    /// Build an owned `array<T>` value `{ ptr, len }` from a buffer pointer and a length.
    MakeDynArray { ptr: Operand, len: Operand },
    /// Column-oriented grouped aggregate (`group_by(.key).<op>(...)`): fold the i64 `vals` column by
    /// the i64 `keys` column into the caller `out_keys`/`out_vals` buffers (each sized to the column
    /// length), via the runtime `align_rt_group_{sum,min,max,count}_i64` per `op`. Yields the group
    /// count (i64). `keys`/`vals` are `{ptr,len}` slices (soa columns; `vals` is unused for `count`);
    /// `out_keys`/`out_vals` are buffer pointers (from [`Rvalue::HeapAllocBuf`]).
    GroupAgg { keys: Operand, vals: Operand, out_keys: Operand, out_vals: Operand, op: hir::GroupOp },
    /// `group_by(.str_key).{sum,min,max}(.i64_value)` / `.count()` over a `soa<Struct>` with a **str
    /// key column** — the columnar counterpart of [`Self::GroupAggStr`]. `keys` is the `str` key
    /// column (`{ptr,len}` over `[AlignStr]`), `vals` the i64 value column (both soa columns; `vals`
    /// is ignored for `count`). codegen extracts the two column base pointers and calls
    /// `align_rt_group_{sum,min,max,count}_str_cols`, which interns the `str` keys to dense ids and
    /// aggregates. `out_keys` is a buffer of `str` views (borrowing the soa's string storage),
    /// `out_vals` a buffer of i64 aggregates; yields the group count (i64).
    GroupAggStrCols { keys: Operand, vals: Operand, out_keys: Operand, out_vals: Operand, op: hir::GroupOp },
    /// `group_by(.str_key).{sum,min,max}(.i64_value)` / `.count()` over an AoS `array<Struct>` (the
    /// dictionary-id rail). `base` is the source struct-array slot (a `{ptr,len}` over `[%Struct]`);
    /// codegen derives the per-row stride and the `key_field`/`value_field` byte offsets from the
    /// struct layout and calls `align_rt_group_{sum,min,max,count}_str`, which interns the `str` keys
    /// to dense ids and aggregates the values per group. `out_keys` is a buffer of `str` views
    /// (`AlignStr`s borrowing `base`), `out_vals` a buffer of i64 aggregates; yields the group count
    /// (i64). `value_field` is `None` for `count` (no value column); `op` selects the runtime entry.
    GroupAggStr { base: Slot, struct_id: u32, key_field: u32, value_field: Option<u32>, op: hir::GroupOp, out_keys: Operand, out_vals: Operand },
    /// `group_by(.str_key).agg(sum(.a), max(.b), count(), …)` over an AoS `array<Struct>` — the
    /// **fused multi-aggregate** str rail. One pass interns each `str` key once and folds every
    /// aggregate in `aggs` into its own column (the `HashMap<&str,[i64;K]>` shape). codegen derives the
    /// per-row stride + the `key_field` and per-aggregate value-field byte offsets, builds the K-entry
    /// runtime spec table (`(val_off, op, out_vals)` each), and calls `align_rt_group_multi_str`.
    /// `out_keys` is a buffer of `str` views (borrowing `base`); `out_vals[j]` is aggregate `j`'s i64
    /// output column. Yields the group count (i64). `aggs[j].value_field` is `None` for `count`.
    GroupAggMultiStr { base: Slot, struct_id: u32, key_field: u32, aggs: Vec<(hir::GroupOp, Option<u32>)>, out_keys: Operand, out_vals: Vec<Operand> },
    /// `s.dict_encode(.key)` — intern the `str` `key_field` column of the AoS array-of-`struct_id` in
    /// slot `base` (codegen derives the per-row stride + key byte offset) into the caller `out_ids`
    /// (dense i64 ids, one per row) + `out_dict` (the `str` dictionary), via `align_rt_dict_encode_str`.
    /// Yields the dictionary size (distinct count, i64). `out_ids`/`out_dict` are [`Rvalue::HeapAllocBuf`]
    /// pointers.
    DictEncode { base: Slot, struct_id: u32, key_field: u32, out_ids: Operand, out_dict: Operand },
    /// Assemble a `dict_encoded` value from its three `{ptr,len}` slices `{ source, ids, dict }` (an
    /// anonymous 3-slice LLVM struct, by value). `source` borrows the AoS; `ids`/`dict` are owned.
    MakeDictEncoded { source: Operand, ids: Operand, dict: Operand },
    /// Extract one of a `dict_encoded` slot's three `{ptr,len}` slices by index (`0` = source AoS,
    /// `1` = ids `array<i64>`, `2` = dict `array<str>`) — a load + extract, yielding the slice value.
    DictField { base: Slot, idx: u32 },
    /// Gather the strided `i64` `field` column of the AoS array-of-`struct_id` `source` (`{ptr,len}`)
    /// into the contiguous buffer `out` (`align_rt_gather_i64`) — the value projection of an encoded
    /// `group_by`. Yields unit.
    GatherColumnI64 { source: Operand, struct_id: u32, field: u32, out: Operand },
    /// Label a dense-id column back to `str` views: `out[i] = dict[ids[i]]` over `n` ids
    /// (`align_rt_dict_lookup`) — the A2 result step. `ids`/`dict` are `{ptr,len}` slices, `out` a
    /// buffer pointer. Yields unit.
    DictLookup { ids: Operand, n: Operand, dict: Operand, out: Operand },
    /// `chunks(n)`: split the `{ptr,len}` slice `src` (element size `elem`) into length-`n`
    /// sub-slices, yielding an owned `array<slice<T>>` value `{ chunk_buf, count }` (via the
    /// runtime `align_rt_chunks`). The element slices borrow `src`.
    Chunks { src: Operand, n: Operand, elem: Ty },
    /// `par_map(f)` over a `{ptr,len}` source `src` with no prior stages — apply the Pure `func`
    /// to each element in parallel (runtime `align_rt_par_map` + a per-`func` thunk), materializing
    /// an owned `array<elem_out>` `{ out_buf, count }`. `elem_in` is the source element type (the
    /// `func` parameter — a scalar, or a `slice<T>` chunk); `elem_out` is `func`'s return.
    ParMapParallel { src: Operand, func: String, elem_in: Ty, elem_out: Ty },
    /// The `len` of a slice operand.
    SliceLen(Operand),
    /// The buffer `ptr` (field 0) of a slice / owned-array `{ptr,len}` operand — the raw element
    /// pointer, used to store back into the buffer (e.g. an in-place `sort`).
    SlicePtr(Operand),
    /// `slice[index]` — load a slice element (scalar).
    SliceIndex(Operand, Operand),
    /// `slice[index]`, exactly like [`Rvalue::SliceIndex`], but tagged with the `map_into` loop's
    /// **`in` alias scope** (`u32` = the loop's scope id) so codegen attaches `!alias.scope !{in}`
    /// / `!noalias !{out}`. The matching source load for [`Stmt::PtrStoreNoalias`]; the two share a
    /// scope id, and the `in`/`out` scopes are declared disjoint, which is what lets the vectorizer
    /// prove the loop's load and store never overlap.
    SliceIndexNoalias { slice: Operand, index: Operand, scope: u32 },
    /// `recv[start..end]` — build a borrowed sub-view `{ base.ptr + start, len }` of the `{ptr,len}`
    /// `base` (a `str` / `slice` / owned-array value). `start` offsets the base pointer by whole
    /// `elem`-sized steps (`u8` bytes for a `str`); `len` is the sub-view length (`end - start`,
    /// computed by the caller). The bounds (`0 <= start <= end <= base.len`) are checked before this.
    SubSlice { base: Operand, start: Operand, len: Operand, elem: Ty },
    /// A string literal — a `str` view `{ &bytes, len }` over a constant.
    StrLit(String),
    /// `str.clone()` — deep-copy a `str` operand's bytes into a fresh heap buffer, yielding an
    /// owned `string` `{ptr,len}`. The buffer is freed by a later [`Stmt::Drop`] of its slot.
    StrClone(Operand),
    /// `s.contains(n)` / `s.starts_with(p)` / `s.ends_with(s)` — a byte-oriented `str` predicate,
    /// yielding `bool` (`i1`). Both operands are `str` `{ptr,len}` views; backed by a runtime
    /// `memchr`-class scan. Pure read, no allocation.
    StrPredicate { kind: hir::StrPredKind, haystack: Operand, needle: Operand },
    /// `s.trim()` / `s.trim_start()` / `s.trim_end()` — yield a borrowed sub-`str` `{ptr,len}` of
    /// the receiver with ASCII whitespace stripped from one or both ends. Pure read, no allocation;
    /// the result aliases the receiver's bytes.
    StrTrim { kind: hir::StrTrimKind, recv: Operand },
    /// `builder()` / `builder(capacity)` — open a builder, yielding an opaque handle (MMv2 slice 7c).
    /// `capacity` (bytes) pre-sizes the backing buffer; 0 = default.
    BuilderNew { capacity: Operand },
    /// `b.write(s)` — append a `str` operand's bytes to the builder. Side-effecting; yields unit.
    BuilderWriteStr(Operand, Operand),
    /// `b.write_int(n)` — append a decimal integer (widened to `i64`) to the builder. Yields unit.
    BuilderWriteInt(Operand, Operand),
    /// `b.write_bool(v)` — append `true`/`false`. Yields unit.
    BuilderWriteBool(Operand, Operand),
    /// `b.write_char(c)` — append a `char`'s UTF-8. Yields unit.
    BuilderWriteChar(Operand, Operand),
    /// `b.write_float(x)` — append an `f32`/`f64` (codegen picks the width). Yields unit.
    BuilderWriteFloat(Operand, Operand),
    /// `b.write(s1); b.write_int(n); b.write(s2)` fused into one runtime call — the common
    /// `literal + int + literal` append sequence (e.g. a `reduce`-builder body). Operands are
    /// `(builder, str1, int, str2)`; codegen passes both `str`s as `ptr,len` and widens the int to
    /// `i64`. Produced by the [`fuse_builder_writes`] peephole, never by direct lowering. Yields unit.
    BuilderWriteStrIntStr(Operand, Operand, Operand, Operand),
    /// `b.to_string()` — finish the builder into an owned `string` `{ptr,len}` (a fresh heap
    /// buffer freed by a later [`Stmt::Drop`]), consuming the builder handle.
    BuilderToString(Operand),
    /// `template "..."` / `str + str` — build a `str` from pieces. The optional operand
    /// is the enclosing arena handle (the result lives there; `None` = leaked).
    Template(Vec<TemplatePiece>, Option<Operand>),
    /// `json.decode` into struct `struct_id`: parse the `str` `input` and fill the `out`
    /// struct slot. Yields an `i32` status (0 = ok). codegen builds the field table (names,
    /// type tags, byte offsets) and calls the runtime parser.
    JsonDecode { struct_id: u32, input: Operand, out: Slot },
    /// `json.decode` into an owned `array<elem>` (MMv2 slice 8c): parse a JSON array of scalars
    /// and write the materialized `{ptr,len}` into the `out` slot. Yields an `i32` status
    /// (0 = ok). `elem` is the element scalar (its kind/width gives the runtime element tag).
    JsonDecodeArray { elem: Ty, input: Operand, out: Slot },
    /// `json.decode` into an owned `array<Struct>` (MMv2 slice 8d): parse a JSON array of objects
    /// into a freshly heap-allocated AoS and write the materialized `{ptr,len}` (len = element
    /// count) into the `out` slot. Yields an `i32` status (0 = ok). codegen builds the same field
    /// table as [`JsonDecode`] plus the element stride, and calls the runtime parser.
    JsonDecodeStructArray { struct_id: u32, input: Operand, out: Slot },
    /// `json.decode` straight into a column-major `soa<Struct>` (the direct-fill rail): parse a JSON
    /// array of objects directly into arena-allocated columns — no AoS intermediate, no transpose —
    /// and write the soa `{ptr,len}` view (len = row count) into the `out` slot. Yields an `i32`
    /// status (0 = ok). `arena` is the enclosing arena handle the runtime bump-allocates the column
    /// buffer from. codegen builds the same field table as [`JsonDecode`] and passes `arena`.
    JsonDecodeSoa { struct_id: u32, input: Operand, out: Slot, arena: Operand },
    /// `fs.read_file(path)`: read the file named by the `str` `path` into a freshly heap-allocated
    /// owned `string`, writing its `{ptr,len}` into the `out` slot. Yields an `i32` status
    /// (0 = ok). The first `std.fs` surface.
    FsReadFile { path: Operand, out: Slot },
    /// `fs.open(path)`: open `path` for reading, writing the owned `reader` handle into `out`.
    /// Yields an `i32` errno-status (0 = ok; see [`make_error_from_status`]).
    ReaderOpen { path: Operand, out: Slot },
    /// `fs.create(path)`: create/truncate `path` for writing, writing the owned `writer` handle into
    /// `out`. Yields an `i32` errno-status (0 = ok).
    WriterCreate { path: Operand, out: Slot },
    /// `io.stdin` — a `reader` over fd 0 (an owned handle; std.io).
    ReaderStdin,
    /// `io.stdout` / `io.stderr` / `io.stdout.buffered()` — a `writer` over `fd` (1 = stdout,
    /// 2 = stderr), `buffered` selecting the accumulator. An owned handle (std.io).
    WriterStd { fd: i32, buffered: bool },
    /// `r.read(b)` — read up to `b`'s capacity into the `buffer` `b`, borrowing both reader and
    /// buffer. Yields an `i64`: bytes read (`0` = EOF) on success, or `-(status)` on error.
    ReaderRead(Operand, Operand),
    /// `w.write(x)` — append a `str`/`bytes` operand's bytes to a `writer`. Yields an `i32`
    /// errno-status (0 = ok).
    WriterWrite(Operand, Operand),
    /// `w.write(b)` — append a `builder`'s bytes to a `writer`, borrowing it. `i32` errno-status.
    WriterWriteBuilder(Operand, Operand),
    /// `w.flush()` — drain a `writer` to the OS, borrowing it. `i32` errno-status (0 = ok).
    WriterFlush(Operand),
    /// `io.copy(r, w)` — stream all of the `reader` operand into the `writer` operand through a
    /// fixed-size buffer (O(buffer) memory), borrowing both. Yields an `i64`: bytes transferred on
    /// success, or `-(status)` on error (same sign convention as [`Self::ReaderRead`]).
    IoCopy(Operand, Operand),
    /// `buffer(cap)` — open an owned byte buffer with read window `cap`, yielding an opaque handle.
    BufferNew(Operand),
    /// `b.bytes()` — a `slice<u8>` view `{ptr,len}` of the buffer's current contents (borrow).
    BufferBytes(Operand),
    /// `b.len()` — the buffer's current byte count (`i64`).
    BufferLen(Operand),
    /// `bytes.<scalar>_<le|be>(off)` — a binary scalar read from the `slice<u8>` operand at byte
    /// offset `off`. `scalar` is the result type (its width sets how many bytes are loaded); `be`
    /// selects big-endian. Bounds are checked (`emit_range_bounds_check`) **before** this rvalue, so
    /// the load itself is unguarded. Lowers to an alignment-1 load (+ a `bswap` for `be`; a float
    /// loads its bits then bitcasts).
    BytesRead { bytes: Operand, offset: Operand, scalar: Ty, be: bool },
    /// `buf.put_<scalar>_<le|be>(v)` — append the `value` operand's bytes to the growable `buffer`
    /// operand in the given byte order. `scalar` is `value`'s type (sets the width; a float is
    /// bit-reinterpreted); `be` selects big-endian. Grows the buffer.
    BufferPut { buffer: Operand, value: Operand, scalar: Ty, be: bool },
    /// `buf.append(data)` — append the raw `slice<u8>` operand `data` (copied) to the growable
    /// `buffer` operand, growing it.
    BufferAppend { buffer: Operand, data: Operand },
    /// `fs.write_file(path, data)` — write all of the `str`/`bytes` operand `data` to `path`, then
    /// close. Yields an `i32` errno-status (0 = ok).
    FsWriteFile { path: Operand, data: Operand },
    /// `fs.write_file(path, builder)` — the `builder`-source form (writes the builder's bytes).
    FsWriteFileBuilder { path: Operand, builder: Operand },
    /// `fs.exists(path)` — `1` if `path` exists, else `0` (an `i32` used directly as a `bool`; every
    /// error folds to `0`, so there is no status branch).
    FsExists { path: Operand },
    /// `fs.remove(path)` — delete the file at `path`. Yields an `i32` errno-status (0 = ok).
    FsRemove { path: Operand },
    /// `fs.read_dir(path)` — the entry names of directory `path` as an owned `array<string>`
    /// (`{ptr,len}`) written into `out`. Yields an `i32` errno-status (0 = ok).
    FsReadDir { path: Operand, out: Slot },
    /// `dns.resolve(host)` — the IP-address strings of `host` as an owned `array<string>`
    /// (`{ptr,len}`) written into `out`. Yields an `i32` status (0 = ok). Same shape as
    /// [`Rvalue::FsReadDir`].
    DnsResolve { host: Operand, out: Slot },
    /// `tcp.connect(host, port)` — resolve `host` and open a TCP connection to `port`, writing the
    /// owned `tcp_conn` handle (a bare pointer) into `out`. Yields an `i32` status (0 = ok, else the
    /// shared errno/status table; a bad port or bad host is `AL_INVALID`). Mirrors [`Rvalue::ReaderOpen`]
    /// (a handle payload written into an out slot), with a second `port` operand.
    TcpConnect { host: Operand, port: Operand, out: Slot },
    /// `c.reader()` — borrow an M9 `reader` over the `tcp_conn` operand's socket fd (`owns_fd:false`).
    /// Yields the reader handle pointer (like [`Rvalue::ReaderStdin`], but over the conn's fd).
    ConnReader(Operand),
    /// `c.writer()` — borrow an M9 (unbuffered) `writer` over the `tcp_conn` operand's socket fd
    /// (`owns_fd:false`). Yields the writer handle pointer.
    ConnWriter(Operand),
    /// `tcp.listen(host, port)` — resolve `host` (`AI_PASSIVE`) and bind+listen on `port`, writing the
    /// owned `tcp_listener` handle (a bare pointer) into `out`. Yields an `i32` status (0 = ok, else
    /// the shared errno/status table; a bad port/host is `AL_INVALID`). Mirrors [`Rvalue::TcpConnect`]
    /// with a `tcp_listener` handle payload.
    TcpListen { host: Operand, port: Operand, out: Slot },
    /// `l.accept()` — block for an inbound connection on the `tcp_listener` operand, writing the new
    /// owned `tcp_conn` handle into `out`. Yields an `i32` status (0 = ok, else the shared errno
    /// table). Mirrors [`Rvalue::TcpConnect`] but takes a listener operand instead of host/port.
    TcpAccept { listener: Operand, out: Slot },
    /// `udp.bind(host, port)` — resolve `host` (`AI_PASSIVE`) and open a `SOCK_DGRAM` socket bound to
    /// `port`, writing the owned `udp_socket` handle (a bare pointer) into `out`. Yields an `i32`
    /// status (0 = ok, else the shared errno/status table; a bad port/host is `AL_INVALID`). Mirrors
    /// [`Rvalue::TcpListen`] with a `udp_socket` handle payload.
    UdpBind { host: Operand, port: Operand, out: Slot },
    /// `u.send_to(data, host, port)` — resolve `host`/`port` (`SOCK_DGRAM`, per call) and `sendto` the
    /// byte view `data` as one datagram from the `udp_socket` operand's fd. Yields an `i64`: the bytes
    /// sent (`>= 0`, `Ok`) or `-(status)` on error (the [`Rvalue::ReaderRead`] sign convention).
    UdpSendTo { sock: Operand, data: Operand, host: Operand, port: Operand },
    /// `u.recv_from(buf)` — block for one inbound datagram on the `udp_socket` operand, filling the
    /// `buffer` operand up to its capacity (overwriting its length). Yields an `i64`: the bytes
    /// received (`>= 0`, `Ok`) or `-(status)` on error (the [`Rvalue::ReaderRead`] sign convention).
    UdpRecvFrom { sock: Operand, buffer: Operand },
    /// `process.spawn(cmd, args)` — `fork` + `execvp(cmd, argv)`. `cmd` is a `str` view (the lookup
    /// path); `args` is a str-view collection `{ptr,len}` (the child's full argv, incl. argv[0]). On
    /// success writes the owned `child` handle (a bare pointer) into `out`. Yields an `i32` status (0 =
    /// ok, else the shared errno/status table; a `fork` failure is the mapped errno, an interior-NUL /
    /// empty argv is `AL_INVALID`). An `execvp` failure is NOT reported here — the forked child
    /// `_exit(127)`s. Mirrors [`Rvalue::UdpBind`] with a `child` handle payload.
    ProcessSpawn { cmd: Operand, args: Operand, out: Slot },
    /// `ch.wait()` — block in `waitpid` for the `child` operand to exit, marking it reaped (through the
    /// borrow — the receiver is read, not consumed). Yields an `i64`: the exit code (`>= 0`:
    /// `WEXITSTATUS`, or `128 + signal` for a signal-killed child) on success, or `-(status)` on error
    /// (a double-wait / `waitpid` failure — the [`Rvalue::ReaderRead`] sign convention).
    ChildWait { child: Operand },
    /// `ch.kill(sig)` — send signal `sig` (an `i64`) to the `child` operand via libc `kill`. Yields an
    /// `i32` errno-status (0 = ok; a negative / out-of-range `sig`, or killing an already-`reaped` child,
    /// is `AL_INVALID`; `EPERM`/`ESRCH` map through the shared table). `child` is borrowed (read, not
    /// consumed — like [`Rvalue::ChildWait`]). Wrapped into `Result<(), Error>` by `lower_status_result`.
    ChildKill { child: Operand, sig: Operand },
    /// `process.exec(cmd, args)` — `execvp(cmd, argv)` **in the current process**. `cmd` is a `str` view
    /// (the lookup path); `args` is a str-view collection `{ptr,len}` (the new image's full argv, incl.
    /// argv[0]). On **success it replaces the image and never returns** — so this yields an `i32`
    /// errno-status only on failure (a bad `cmd`/`argv` is `AL_INVALID`, else the mapped `execvp` errno),
    /// wrapped into `Result<(), Error>` by `lower_status_result` (whose `Err` arm is the only observable
    /// one). **No cleanup is emitted** (unlike `process.exit`): `execvp` discards the address space, so
    /// pending `Drop`s / arena ends / buffered writers are inherently lost on success.
    ProcessExec { cmd: Operand, args: Operand },
    /// `fs.read_file_view(path)` — mmap the regular file `path` read-only into `arena`, writing the
    /// `str` view `{ptr,len}` into `out`. Yields an `i32` errno-status (0 = ok). The mapping is
    /// `munmap`ped at arena end (the region rule) — no `Drop`.
    FsReadFileView { path: Operand, arena: Operand, out: Slot },
    /// `fs.read_bytes_view(path)` — the binary sibling of [`Self::FsReadFileView`]: the same
    /// arena `mmap` (regular-file fast path + owned-copy fallback, `munmap` at arena end) minus the
    /// UTF-8 validation, writing the `bytes` (`slice<u8>`) view `{ptr,len}` into `out`. Yields an
    /// `i32` errno-status (0 = ok). No `Drop` — the view aliases the arena.
    FsReadBytesView { path: Operand, arena: Operand, out: Slot },
    /// `path.join(a, b)` — join two path fragments into a freshly heap-allocated owned `string`,
    /// returned by value as a `{ptr,len}` (like `str_clone`). Pure.
    PathJoin { a: Operand, b: Operand },
    /// `path.base`/`dir`/`ext(p)` — a zero-copy substring `str` view `{ptr,len}` of `p` (aliases its
    /// bytes, no allocation — like `StrTrim`), returned by value. `kind` selects the component. Pure.
    PathComponent { kind: hir::PathComponentKind, path: Operand },
    /// `path.normalize(p)` — lexically normalize `p` into a freshly heap-allocated owned `string`,
    /// returned by value as a `{ptr,len}`. Pure.
    PathNormalize { path: Operand },
    /// `env.get(name)` — write the owned `string` value `{ptr,len}` of environment variable `name`
    /// into `out` (or `{null,0}` if unset), returning an `i32` present flag (`1` = set, `0` = unset).
    /// The caller branches into `Some`/`None`. Impure.
    EnvGet { name: Operand, out: Slot },
    /// `env.set(name, value)` — set environment variable `name` to `value`. Yields an `i32`
    /// errno-status (0 = ok). Impure.
    EnvSet { name: Operand, value: Operand },
    /// `time.now()` — wall-clock UNIX-epoch nanoseconds (`CLOCK_REALTIME`), an `i64`. Impure.
    TimeNow,
    /// `time.instant()` — monotonic-clock nanoseconds (`CLOCK_MONOTONIC`), an `i64`. Impure.
    TimeInstant,
    /// `time.sleep(ns)` — suspend the thread for `ns` nanoseconds (negative = no-op). Yields no
    /// meaningful value (the expression's type is `()`); codegen emits the void call. Impure.
    TimeSleep { ns: Operand },
    /// `encoding.base64_encode`/`base64url_encode`/`hex_encode(data)` — encode the byte view `data`
    /// (`{ptr,len}`) into a freshly heap-allocated owned `string`, returned by value as a `{ptr,len}`
    /// (like `PathNormalize`). `kind` selects the alphabet. Pure.
    EncodingEncode { kind: hir::EncodingKind, data: Operand },
    /// `encoding.base64_decode`/`base64url_decode`/`hex_decode(s)` — decode the `str` view `input`
    /// (`{ptr,len}`) into an owned `buffer` handle written to `out`; yields an `i32` status
    /// (0 = ok, `AL_INVALID` -> `Error.Invalid`; see [`make_error_from_status`]). The caller branches
    /// `Ok(buffer)` / `Err`. Pure.
    EncodingDecode { kind: hir::EncodingKind, input: Operand, out: Slot },
    /// `compress.gzip_compress(data, level)` — compress the byte view `data` at `level` (an i64).
    /// The runtime writes an owned `buffer` handle into `out` and returns an i32 status (0 = ok;
    /// `AL_INVALID` -> `Error.Invalid`; `>= AL_CODE` -> `Error.Code`). An out-of-range level aborts
    /// in the runtime. Value = the i32 status; the wrapped buffer is owned (the local `Drop`s it).
    CompressCompress { kind: hir::CompressKind, data: Operand, level: Operand, out: Slot },
    /// `compress.gzip_decompress(data)` — inflate the gzip byte view `data`; the runtime writes an
    /// owned `buffer` handle into `out` and returns an i32 status (corrupt/truncated/bomb ->
    /// `AL_INVALID`). Value = the i32 status; the wrapped buffer is owned.
    CompressDecompress { kind: hir::CompressKind, data: Operand, out: Slot },
    /// `encoding.utf8_valid(b)` — whether the byte view `b` (`{ptr,len}`) is valid UTF-8, an `i32`
    /// used directly as a `bool` (`1`/`0`). Pure.
    Utf8Valid { data: Operand },
    /// `crypto.constant_time_equal(a, b)` — a constant-time byte-equality test over two byte views
    /// (each `{ptr,len}`), an `i32` used directly as a `bool` (`1` = equal, `0` = not, like
    /// [`Self::Utf8Valid`]). The input length is public (differing lengths → `0`); the runtime's
    /// equal-length compare is branchless (no early return). Pure.
    CryptoCtEqual { a: Operand, b: Operand },
    /// `crypto.random(out)` — fill the whole `buffer` `out` (its full capacity, by handle pointer)
    /// with OS CSPRNG bytes. Yields no value (the expression type is `()`); codegen emits the void
    /// call. A CSPRNG failure aborts in the runtime. Impure.
    CryptoRandom { out: Operand },
    /// `crypto.sha256(data)` / `crypto.sha512(data)` — the cryptographic digest of the byte view
    /// `data` (`{ptr,len}`), a fresh *owned* `array<u8>` of fixed length (32 / 64) returned by value
    /// as a `{ptr,len}` (like [`Self::RandSample`]). `algo` param-swaps the EVP digest. The bound
    /// local `Drop`-frees the array. Impure (a libcrypto call).
    CryptoHash { algo: hir::HashAlgo, data: Operand },
    /// `crypto.hmac_sha256(key, data)` — the 32-byte HMAC-SHA-256 tag of the byte views `key` / `data`
    /// (each `{ptr,len}`), a fresh *owned* `array<u8>` returned by value as a `{ptr,len}` (the
    /// [`Self::CryptoHash`] shape). The bound local `Drop`-frees the array. Impure (a libcrypto call).
    CryptoHmac { key: Operand, data: Operand },
    /// `crypto.hkdf_sha256(salt, ikm, info, len)` — derive `len` bytes with HKDF-SHA-256 over the byte
    /// views `salt` / `ikm` / `info` (each `{ptr,len}`). The runtime writes an owned `buffer` handle
    /// into `out` and returns an `i32` status (0 ok, `AL_INVALID` bad-`len`/rejected-params,
    /// `AL_CODE+n` engine failure — see [`make_error_from_status`]); the caller branches
    /// `Ok(<buffer>)` / `Err(<mapped>)` via [`emit_status_buffer_result`]. Impure.
    CryptoHkdf { salt: Operand, ikm: Operand, info: Operand, len: Operand, out: Slot },
    /// `crypto.{aes_gcm,chacha20_poly1305}_{seal,open}(key, nonce, input, aad)` — AEAD over the byte
    /// views `key` / `nonce` / `input` (plaintext on seal, `ciphertext || tag` on open) / `aad` (each
    /// `{ptr,len}`). `cipher` param-swaps the fetched `EVP_CIPHER`; `dir` picks seal vs open (codegen
    /// selects one of the four runtime entry points from the pair). The runtime writes an owned
    /// `buffer` handle into `out` and returns an `i32` status (0 ok; `AL_INVALID` → `Error.Invalid`
    /// for a bad key/nonce length, a too-short/corrupt open input, or an open auth failure — the
    /// single opaque failure, P2; `AL_CODE+n` → `Error.Code` only for a **seal** engine failure). The
    /// caller branches `Ok(<buffer>)` / `Err(<mapped>)` via [`emit_status_buffer_result`]. Impure.
    CryptoAead { cipher: hir::AeadCipher, dir: hir::AeadDir, key: Operand, nonce: Operand, input: Operand, aad: Operand, out: Slot },
    /// `crypto.argon2id(password, salt, params)` — Argon2id via OpenSSL's `EVP_KDF("ARGON2ID")`.
    /// `password` / `salt` are byte views (`{ptr,len}`); the four `i64` tuning knobs are the fields of
    /// the caller's `argon2_params` struct, read out at lowering (`m_cost` KiB, `t_cost` iterations,
    /// `parallelism` lanes, `len` output bytes). The runtime validates the public param bounds, writes
    /// an owned `buffer` handle into `out`, and returns an `i32` status (0 ok; `AL_INVALID` → a
    /// public-param violation or an engine param rejection e.g. a too-short salt; `AL_CODE+n` → a
    /// genuine engine failure — see [`make_error_from_status`]); the caller branches `Ok(<buffer>)` /
    /// `Err(<mapped>)` via [`emit_status_buffer_result`]. Impure. The six-operand payload is **boxed**
    /// (see [`Argon2Args`]) so this variant does not widen `Rvalue` — a wide variant inflates every
    /// `lower_expr` stack frame (each holds an `Rvalue` temporary), regressing the recursive
    /// `expr_depth` headroom (#296).
    CryptoArgon2(Box<Argon2Args>),
    /// `rand.seed()` / `rand.seed_with(s)` — initialize an `rng` (four `i64`s, Xoshiro256++) into the
    /// slot `out`. `seed` is `None` for the OS-seeded form (`getrandom`), `Some(s)` for the
    /// deterministic form. Yields no value (the caller `Load`s `out` for the `rng` aggregate).
    RandSeed { seed: Option<Operand>, out: Slot },
    /// `r.next()` — advance the rng in slot `rng` (in place, by pointer) and return the next `i64`.
    RandNext { rng: Slot },
    /// `r.range(lo, hi)` — a uniform `i64` in `[lo, hi)` from the rng in slot `rng` (advanced in
    /// place); `lo >= hi` aborts at runtime.
    RandRange { rng: Slot, lo: Operand, hi: Operand },
    /// `r.shuffle(out xs)` — Fisher-Yates the slice `xs` (`{ptr,len}`) in place, using (and
    /// advancing) the rng in slot `rng`. `elem` sizes each element (byte swaps). Yields no value.
    RandShuffle { rng: Slot, xs: Operand, elem: Ty },
    /// `r.sample(xs, k)` — draw `k` elements of `xs` (`{ptr,len}`) without replacement into a fresh
    /// owned `array<T>`, returned by value as a `{ptr,len}` (freed by the bound local's `Drop`).
    /// Uses/advances the rng in slot `rng`; `k < 0` or `k > len` aborts at runtime. `elem` sizes each
    /// element.
    RandSample { rng: Slot, xs: Operand, k: Operand, elem: Ty },
    /// `cli.command(name)` — allocate a `cli command` handle named `name` (a `str` `{ptr,len}`),
    /// returned by value as an opaque pointer (the bound local `Drop`-frees it). Pure.
    CliCommand { name: Operand },
    /// `c.flag_bool/str/i64(...)` — register a flag into the command handle `cmd` (an opaque
    /// pointer), mutating it in place. `kind` selects the runtime symbol; `name` is a `str`
    /// `{ptr,len}`; `default` is the `str` default `{ptr,len}` (`flag_str`) or the `i64` default
    /// (`flag_i64`), `None` for `flag_bool`. Yields no value. Pure.
    CliFlag { cmd: Operand, kind: hir::CliFlagKind, name: Operand, default: Option<Operand> },
    /// `c.parse(args)` — parse the argv `array<str>` `args` (`{ptr,len}` = an `AlignStr` buffer +
    /// count) against the command handle `cmd`, writing an owned `cli parsed` handle into `out` and
    /// returning an `i32` status (0 = ok, `AL_INVALID` -> `Error.Invalid`). The caller branches
    /// `Ok(parsed)` / `Err`. Pure.
    CliParse { cmd: Operand, args: Operand, out: Slot },
    /// `p.get_bool(name)` — the parsed `bool` for flag `name` (a `str` `{ptr,len}`) from the parsed
    /// handle `parsed`, an `i32` (1/0) used as a `bool`. Aborts at runtime on unregistered /
    /// wrong-kind. Pure.
    CliGetBool { parsed: Operand, name: Operand },
    /// `p.get_i64(name)` — the parsed `i64` for flag `name`. Aborts on unregistered / wrong-kind. Pure.
    CliGetI64 { parsed: Operand, name: Operand },
    /// `p.get_str(name)` — the parsed `str` **view** (`{ptr,len}`) for flag `name`, borrowing the
    /// parsed handle's storage (region-bound to `parsed`). Aborts on unregistered / wrong-kind. Pure.
    CliGetStr { parsed: Operand, name: Operand },
    /// `c.usage()` — render the command handle `cmd`'s flag table into a fresh owned `string`,
    /// returned by value as a `{ptr,len}` (the bound local `Drop`-frees it). Pure.
    CliUsage { cmd: Operand },
    /// `http.request(method, url)` — allocate an `http request` builder (opaque pointer), returned by
    /// value (the bound local `Drop`-frees it via `http_request_free`). `method`/`url` are `str`
    /// `{ptr,len}`. Pure.
    HttpRequest { method: Operand, url: Operand },
    /// `r.header(name, value)` — append a header to the request handle `req` (opaque pointer), in
    /// place. `name`/`value` are `str` `{ptr,len}`. Aborts at runtime on CR/LF/NUL (P6). No value. Pure.
    HttpHeader { req: Operand, name: Operand, value: Operand },
    /// `r.body(data)` — copy the byte view `data` (`{ptr,len}`) into the request handle `req`'s body.
    /// No value. Pure.
    HttpBody { req: Operand, data: Operand },
    /// `http.parse(data)` — parse the response byte view `data` (`{ptr,len}`) into an owned `http
    /// response` handle written to `out`, returning an `i32` status (0 = ok, `AL_INVALID` ->
    /// `Error.Invalid`). The caller branches `Ok(response)` / `Err`. Pure.
    HttpParse { data: Operand, out: Slot },
    /// `resp.status()` — the parsed status code (`i64`) of the response handle `resp`. Pure.
    HttpRespStatus { resp: Operand },
    /// `resp.header(name)` — a case-insensitive header lookup on `resp`, writing a `str` **view**
    /// `{ptr,len}` (region-bound to `resp`) to `out` and returning an `i32` present flag (1/0). Pure.
    HttpRespHeader { resp: Operand, name: Operand, out: Slot },
    /// `resp.body()` — the response body as a `slice<u8>` **view** `{ptr,len}` into `resp`'s buffer
    /// (region-bound to `resp`). Pure.
    HttpRespBody { resp: Operand },
    /// `http.client()` — allocate an `http client` handle (opaque pointer), returned by value (the
    /// bound local `Drop`-frees it via `http_client_free`). No operands. Pure (no I/O — the requests
    /// are impure). Slice 2 carries no pooled state.
    HttpClient,
    /// `cl.get(url)` — perform a `GET url` over a fresh connection: the runtime writes an owned `http
    /// response` handle to `out` and returns an `i32` status (0 = ok — a 4xx/5xx is still ok, status is
    /// data; else `AL_INVALID` / errno → `Error`). The caller branches `Ok(response)` / `Err`. Impure.
    HttpClientGet { client: Operand, url: Operand, out: Slot },
    /// `cl.post(url, body)` — perform a `POST url` with `body` (auto `Content-Length`). Same out-slot +
    /// i32-status contract as [`Rvalue::HttpClientGet`]. Impure.
    HttpClientPost { client: Operand, url: Operand, body: Operand, out: Slot },
    /// `cl.request(req)` — perform the fully-built request handle `req` (an opaque pointer, **moved
    /// in** — the runtime frees it, so the MIR nulls its source slot). Same out-slot + i32-status
    /// contract as [`Rvalue::HttpClientGet`]. Impure.
    HttpClientRequest { client: Operand, req: Operand, out: Slot },
}

/// One piece of a lowered `template`: a static run, or an interpolated value.
#[derive(Clone, Debug)]
pub enum TemplatePiece {
    Static(String),
    IntHole(Operand),
    StrHole(Operand),
    BoolHole(Operand),
    CharHole(Operand),
    /// A float hole; codegen picks f32/f64 from the operand's type.
    FloatHole(Operand),
    /// A `str` operand emitted as a JSON string literal (quoted + escaped). From `json.encode`.
    JsonStrHole(Operand),
}

#[derive(Clone, Debug)]
pub enum Operand {
    Const(Const),
    Value(ValueId),
    /// The i-th incoming function argument.
    Arg(u32),
}

/// The boxed operands of a [`Rvalue::CryptoArgon2`] — two byte views (`password` / `salt`) and the
/// four `i64` Argon2 tuning knobs read out of the caller's `argon2_params` struct (`m_cost` KiB,
/// `t_cost` iterations, `parallelism` lanes, `len` output bytes), plus the owned-`buffer` out slot.
/// Boxed to keep [`Rvalue`] narrow (see the variant doc).
#[derive(Clone, Debug)]
pub struct Argon2Args {
    pub password: Operand,
    pub salt: Operand,
    pub m_cost: Operand,
    pub t_cost: Operand,
    pub parallelism: Operand,
    pub len: Operand,
    pub out: Slot,
}

#[derive(Clone, Copy, Debug)]
pub enum Const {
    Int(i128, Ty),
    Float(f64, Ty),
    Char(u32),
    Bool(bool),
    /// The unit value `()`.
    Unit,
}

#[derive(Clone, Debug)]
pub enum Term {
    Goto(BlockId),
    Branch(Operand, BlockId, BlockId),
    Return(Option<Operand>),
    Unreachable,
}

/// typed HIR -> MIR.
pub fn lower_program(program: &hir::Program) -> Program {
    Program {
        fns: program
            .fns
            .iter()
            .map(|f| {
                let mut mf = lower_fn(f, &program.tuples, &program.structs);
                fuse_builder_writes(&mut mf);
                mf
            })
            .collect(),
        externs: program.externs.clone(),
        link_libs: program.link_libs.clone(),
        structs: program.structs.clone(),
        enums: program.enums.clone(),
        tuples: program.tuples.clone(),
    }
}

/// Identifies which builder a write targets, so a `write_str`/`write_int`/`write_str` triple can be
/// confirmed to act on the *same* builder. Each `b.<write>` re-loads the builder from its slot, so
/// the three writes carry distinct value ids that all resolve to the same `Load(slot)` — hence the
/// slot identity rather than operand identity.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BuilderKey {
    Slot(Slot),
    Arg(u32),
    Value(ValueId),
}

/// Resolve a builder operand to a [`BuilderKey`]. `loads` maps a value id to the slot it loads (when
/// the value was produced by `Load(slot)`), so repeated `Load(slot)` operands compare equal.
fn builder_key(op: &Operand, loads: &std::collections::HashMap<ValueId, Slot>) -> Option<BuilderKey> {
    match op {
        Operand::Value(v) => Some(loads.get(v).map(|s| BuilderKey::Slot(*s)).unwrap_or(BuilderKey::Value(*v))),
        Operand::Arg(i) => Some(BuilderKey::Arg(*i)),
        Operand::Const(_) => None,
    }
}

/// A statement is "movable" past the fused appends iff it has no observable side effect — only then
/// is deferring the first two appends to the third write's position sound. The builder-append code
/// only ever interleaves `Load`/`StrLit`/`Use` between the three writes, so this narrow whitelist
/// covers the real generated shape while staying conservative (anything else blocks the fusion).
fn is_movable_stmt(s: &Stmt) -> bool {
    matches!(
        s,
        Stmt::Let(_, Rvalue::Load(_)) | Stmt::Let(_, Rvalue::StrLit(_)) | Stmt::Let(_, Rvalue::Use(_))
    )
}

/// Peephole: fuse `b.write(str1); b.write_int(n); b.write(str2)` into one
/// [`Rvalue::BuilderWriteStrIntStr`] runtime call, removing two per-element FFI boundaries from the
/// builder hot path (the `reduce`-builder body). Narrow on purpose — only the `str,int,str` shape on
/// one builder, with nothing but pure operand materialization between the three writes.
fn fuse_builder_writes(f: &mut Function) {
    for block in &mut f.blocks {
        let loads: std::collections::HashMap<ValueId, Slot> = block
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Let(v, Rvalue::Load(slot)) => Some((*v, *slot)),
                _ => None,
            })
            .collect();

        // Indices of the str writes and int writes, with their resolved builder + payload operands.
        let mut removed: Vec<usize> = Vec::new();
        let mut fused: Vec<(usize, Rvalue)> = Vec::new();
        let n = block.stmts.len();
        let mut i = 0;
        while i < n {
            // Anchor on a `write(str)`.
            let (b1, s1) = match &block.stmts[i] {
                Stmt::Let(_, Rvalue::BuilderWriteStr(b, s)) => (b.clone(), s.clone()),
                _ => {
                    i += 1;
                    continue;
                }
            };
            // Bail unless the anchor builder resolves to a concrete key — never fuse on an unresolved
            // (`None`) builder, which would otherwise let two distinct unresolved builders match.
            let Some(key1) = builder_key(&b1, &loads) else {
                i += 1;
                continue;
            };
            // Find the next `write_int` on the same builder, allowing only movable statements between.
            let int_idx = find_next_write(&block.stmts, i + 1, n, &loads, key1, WriteShape::Int);
            let Some((j, n_op)) = int_idx else {
                i += 1;
                continue;
            };
            // Then a closing `write(str)` on the same builder, again only movable stmts between.
            let str_idx = find_next_write(&block.stmts, j + 1, n, &loads, key1, WriteShape::Str);
            let Some((k, s2_op)) = str_idx else {
                i += 1;
                continue;
            };
            // The builder operand of the third write is live at position `k` (where we emit the fused
            // call); reuse it so the call's receiver is in scope.
            let b3 = match &block.stmts[k] {
                Stmt::Let(_, Rvalue::BuilderWriteStr(b, _)) => b.clone(),
                _ => unreachable!("str_idx points at a BuilderWriteStr"),
            };
            fused.push((k, Rvalue::BuilderWriteStrIntStr(b3, s1, n_op, s2_op)));
            removed.push(i);
            removed.push(j);
            i = k + 1;
        }

        if removed.is_empty() {
            continue;
        }
        for (k, rv) in fused {
            if let Stmt::Let(_, slot) = &mut block.stmts[k] {
                *slot = rv;
            }
        }
        let drop: std::collections::HashSet<usize> = removed.into_iter().collect();
        let mut idx = 0;
        block.stmts.retain(|_| {
            let keep = !drop.contains(&idx);
            idx += 1;
            keep
        });
    }
}

#[derive(Clone, Copy)]
enum WriteShape {
    Str,
    Int,
}

/// Scan forward from `start` for the next builder write of `shape` on builder `key`, requiring every
/// statement in between to be [movable](is_movable_stmt) (else the appends can't be safely reordered
/// to one call). Returns the write's index and its payload operand (the str or the int).
fn find_next_write(
    stmts: &[Stmt],
    start: usize,
    end: usize,
    loads: &std::collections::HashMap<ValueId, Slot>,
    key: BuilderKey,
    shape: WriteShape,
) -> Option<(usize, Operand)> {
    for (offset, s) in stmts[start..end].iter().enumerate() {
        let idx = start + offset;
        match (shape, s) {
            (WriteShape::Int, Stmt::Let(_, Rvalue::BuilderWriteInt(b, n))) if builder_key(b, loads) == Some(key) => {
                return Some((idx, n.clone()));
            }
            (WriteShape::Str, Stmt::Let(_, Rvalue::BuilderWriteStr(b, s2))) if builder_key(b, loads) == Some(key) => {
                return Some((idx, s2.clone()));
            }
            _ if is_movable_stmt(s) => continue,
            // Any non-movable statement (another write, a call, a store, …) ends the search: the
            // pattern must be contiguous over movable statements only.
            _ => return None,
        }
    }
    None
}

struct BBuild {
    stmts: Vec<Stmt>,
    term: Option<Term>,
}

struct Builder {
    slots: Vec<Ty>,
    /// Per-slot over-alignment, parallel to `slots` (`None` for temporaries and plain locals).
    slot_align: Vec<Option<u32>>,
    value_tys: Vec<Ty>,
    blocks: Vec<BBuild>,
    cur: BlockId,
    /// The enclosing function's return type (so `?` can build the propagated Result).
    ret: Ty,
    /// Handles of the arenas currently open (innermost last); any exit out of them
    /// (`return`, `?`) must free them first.
    arenas: Vec<ValueId>,
    /// Handles of the `task_group`s currently open (innermost last); `spawn`/`wait` use the top.
    task_groups: Vec<ValueId>,
    /// Free-standing owned locals (heap `array<T>`) that must be freed at every function
    /// exit (MMv2 slice 4; `hir::Fn::drop_locals`). Their slots are null-initialised at
    /// entry, so a drop on a path that never allocated frees null (a no-op).
    drop_locals: Vec<Slot>,
    /// Tuple defs — to tell whether a `Ty::Tuple` slot is a Move tuple (holds an owned element),
    /// which `null_moved_source` must null on move so its exit `Drop` doesn't double-free.
    tuples: Vec<hir::TupleDef>,
    /// Struct defs — `to_soa` reads each field's type to scatter it into its column.
    structs: Vec<hir::StructDef>,
    /// Monotonic id for each `map_into` loop's alias scope (a fresh disjoint `in`/`out`
    /// scope pair per loop). Threaded into [`Rvalue::SliceIndexNoalias`] / [`Stmt::PtrStoreNoalias`]
    /// so codegen tags the source load and the `dst` store of the *same* loop with the same
    /// scoped-`noalias` metadata. Per-function (the Builder is per-function).
    alias_scope: u32,
    /// Stack of enclosing `loop` expressions (innermost last), for lowering `break`: its `exit`
    /// block, `result_slot` (where a `break` value is stored, `None` for a unit loop), and
    /// `iter_drops` (owned locals declared in the body, dropped each iteration / at each `break`).
    loops: Vec<LoopFrame>,
}

/// A `loop` being lowered — the target of a `break` inside its body. See [`Builder::loops`].
struct LoopFrame {
    exit: BlockId,
    result_slot: Option<Slot>,
    /// Owned free-standing locals declared inside the loop body (a subset of `drop_locals`). They
    /// are dropped and null-reset at the back-edge of each iteration and at each `break`, so a
    /// per-iteration allocation is freed once per pass instead of leaking when the slot is reused.
    iter_drops: Vec<Slot>,
}

impl Builder {
    /// Free every open arena (innermost first), join + free every open `task_group`, and drop
    /// every owned free-standing local — emitted before any exit that leaves these scopes.
    fn emit_exit_cleanup(&mut self) {
        for s in self.drop_locals.clone() {
            self.push(Stmt::Drop(s));
        }
        // An early exit out of a `task_group` still joins its tasks (structured concurrency) and
        // frees the region.
        let tgs = self.task_groups.clone();
        for h in tgs.into_iter().rev() {
            self.push(Stmt::TgWait(Operand::Value(h)));
            self.push(Stmt::TgEnd(Operand::Value(h)));
        }
        let handles = self.arenas.clone();
        for h in handles.into_iter().rev() {
            self.push(Stmt::ArenaEnd(Operand::Value(h)));
        }
    }
}

impl Builder {
    fn new_block(&mut self) -> BlockId {
        let id = self.blocks.len() as BlockId;
        self.blocks.push(BBuild {
            stmts: Vec::new(),
            term: None,
        });
        id
    }

    fn fresh_value(&mut self, ty: Ty) -> ValueId {
        let v = self.value_tys.len() as ValueId;
        self.value_tys.push(ty);
        v
    }

    /// A fresh alias-scope id for a `map_into` loop — its `in`/`out` scopes are declared disjoint
    /// per loop (codegen builds distinct metadata per id) so two `map_into` loops in one function
    /// never cross-constrain.
    fn fresh_alias_scope(&mut self) -> u32 {
        let s = self.alias_scope;
        self.alias_scope += 1;
        s
    }

    fn new_slot(&mut self, ty: Ty) -> Slot {
        let s = self.slots.len() as Slot;
        self.slots.push(ty);
        self.slot_align.push(None);
        s
    }

    fn push(&mut self, s: Stmt) {
        self.blocks[self.cur as usize].stmts.push(s);
    }

    fn terminate(&mut self, t: Term) {
        let b = &mut self.blocks[self.cur as usize];
        if b.term.is_none() {
            b.term = Some(t);
        }
    }

    fn is_terminated(&self) -> bool {
        self.blocks[self.cur as usize].term.is_some()
    }
}

fn lower_fn(f: &hir::Fn, tuples: &[hir::TupleDef], structs: &[hir::StructDef]) -> Function {
    let mut b = Builder {
        slots: f.locals.iter().map(|l| l.ty).collect(),
        slot_align: f.locals.iter().map(|l| l.align).collect(),
        value_tys: Vec::new(),
        blocks: Vec::new(),
        cur: 0,
        ret: f.ret,
        arenas: Vec::new(),
        task_groups: Vec::new(),
        drop_locals: f.drop_locals.clone(),
        tuples: tuples.to_vec(),
        structs: structs.to_vec(),
        alias_scope: 0,
        loops: Vec::new(),
    };
    let entry = b.new_block();
    b.cur = entry;

    // Slot index == HIR LocalId (locals are created in id order).
    let params: Vec<Slot> = f.params.clone();
    for (i, &slot) in params.iter().enumerate() {
        b.push(Stmt::Store(slot, Operand::Arg(i as u32)));
    }
    // Null-initialise each owned-drop slot so a drop on a path that never allocated frees
    // null (a no-op) instead of an uninitialised pointer. Parameters are excluded: they arrive
    // already initialised (owning a valid buffer), so zeroing them would clobber the argument
    // and leak the caller-transferred buffer.
    for s in b.drop_locals.clone() {
        if !params.contains(&s) {
            b.push(Stmt::DropFlagInit(s));
        }
    }

    let tail = lower_block(&mut b, &f.body);
    if !b.is_terminated() {
        // Fall-through end of the body: if the trailing value moves an owned local out (the
        // function returns it), null that local's slot so the exit cleanup frees null — the
        // caller now owns the buffer — then drop the remaining owned locals.
        if f.ret != Ty::Unit
            && let Some(v) = &f.body.value {
                null_moved_source(&mut b, v);
            }
        let tail = tail.filter(|_| f.ret != Ty::Unit);
        b.emit_exit_cleanup();
        match tail {
            Some(op) => b.terminate(Term::Return(Some(op))),
            None => b.terminate(Term::Return(None)),
        }
    }

    let blocks = b
        .blocks
        .into_iter()
        .enumerate()
        .map(|(id, bb)| Block {
            id: id as BlockId,
            stmts: bb.stmts,
            term: bb.term.unwrap_or(Term::Unreachable),
        })
        .collect();

    Function {
        name: f.name.clone(),
        params,
        ret: f.ret,
        slots: b.slots,
        slot_align: b.slot_align,
        value_tys: b.value_tys,
        blocks,
        entry,
    }
}

/// The type of the leaf field reached by a logical field `path` (length ≥ 1) through a chain of
/// nested structs rooted at `struct_id`. Each non-final field is a struct (sema's nested-access
/// walk guarantees it); the final field's type is returned. Used to decide the drop-of-old-value on
/// an element-field store (`us[i].addr.name = new` frees the old `string`).
fn field_path_leaf_ty(structs: &[hir::StructDef], struct_id: u32, path: &[u32]) -> Ty {
    let mut sid = struct_id;
    for (k, &f) in path.iter().enumerate() {
        let fty = structs[sid as usize].fields[f as usize].ty;
        if k + 1 == path.len() {
            return fty;
        }
        match fty {
            Ty::Struct(nid) => sid = nid,
            other => unreachable!("nested element-field path through non-struct {other:?}"),
        }
    }
    unreachable!("field_path_leaf_ty called with an empty path")
}

/// Null the slot of an owned `array<T>` local moved out at a (just-lowered) consuming site,
/// so its exit [`Stmt::Drop`] becomes a no-op `free(null)` and the buffer is freed once — by
/// the new owner. The moved expression is a bare `Local` (null its slot) or a block/arena whose
/// trailing value is the move (recurse into the tail). Other shapes (fresh temporaries like
/// `make()` / `.to_array()`) own no slot, and sema rejects moving a bound owned local out
/// through an `if`/`else` arm, so no other case reaches here. Restricted to free-standing owned
/// slots (`DynArray`, owned `string`) — `box<T>` is arena-regioned and never free-standing-dropped.
fn null_moved_source(b: &mut Builder, e: &hir::Expr) {
    match &e.kind {
        hir::ExprKind::Local(id) => {
            let moved = match b.slots.get(*id as usize) {
                Some(&ty) => {
                    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::String | Ty::Builder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient | Ty::DictEncoded(..))
                        || payload_is_move(ty)
                        // A Move tuple (holds an owned element) moved away must be nulled so its
                        // exit `Drop` frees nulls, not the buffers the new owner took.
                        || matches!(ty, Ty::Tuple(tid) if b.tuples[tid as usize].elems.iter().any(|s| s.is_move()))
                        // A Move struct (owns a `string`/owned field) moved away must be nulled too,
                        // so its exit `Drop` frees null, not the buffers the new owner took.
                        || matches!(ty, Ty::Struct(sid) if struct_is_move(sid, &b.structs))
                }
                None => false,
            };
            if moved {
                b.push(Stmt::DropFlagInit(*id));
            }
        }
        hir::ExprKind::Block(blk) | hir::ExprKind::Arena(blk) | hir::ExprKind::Unsafe(blk) => {
            if let Some(v) = &blk.value {
                null_moved_source(b, v);
            }
        }
        // `t.get()` moves an owned result out of the task; null the task slot so its exit `Drop`
        // doesn't double-free the buffer the gotten value now owns.
        hir::ExprKind::TaskGet(inner) => null_moved_source(b, inner),
        // A bound owned local moved into a wrapper (`return Ok(xs)` / `Some(xs)` / `Err(xs)`) is
        // consumed by the construction — see through the wrapper to null the source slot, else the
        // local's exit `Drop` double-frees the buffer now owned by the aggregate.
        hir::ExprKind::ResultOk(inner) | hir::ExprKind::ResultErr(inner) | hir::ExprKind::OptionSome(inner) => {
            null_moved_source(b, inner);
        }
        // A tuple literal moves each owned-local element into the tuple (its consumer — a
        // destructure target, or the returned tuple's caller — now owns the buffer), so null those
        // source slots, else both the source local and the new owner would free the same buffer.
        hir::ExprKind::Tuple { elems, .. } => {
            for el in elems {
                null_moved_source(b, el);
            }
        }
        // A partial field move (`a := t.0`) took the owned element's buffer; null that one field of
        // the tuple slot so the tuple's exit `Drop` frees null there, not the now-aliased buffer.
        hir::ExprKind::TupleIndex { recv, index } => {
            if let hir::ExprKind::Local(t) = &recv.kind {
                let owned = matches!(b.slots.get(*t as usize), Some(&Ty::Tuple(tid))
                    if b.tuples[tid as usize].elems.get(*index as usize).is_some_and(|s| s.is_move()));
                if owned {
                    b.push(Stmt::NullTupleField(*t, *index));
                }
            }
        }
        // A partial owned-field move out of a struct (`n := u.name`) took the `string` field's
        // buffer; null that depth-1 field of the struct slot so the struct's recursive `Drop` frees
        // null there, not the buffer the new binding now owns. (Sema allows this only for a depth-1
        // `string` field; deeper paths / Move-struct fields stay rejected, so `path` is `[idx]`.)
        hir::ExprKind::Field { root, path } if path.len() == 1 && e.ty == Ty::String => {
            b.push(Stmt::NullStructField(*root, path[0]));
        }
        _ => {}
    }
}

/// The slot backing an `rng` method receiver. Sema requires the receiver to be a bound **mut**
/// local, so its HIR is an [`hir::ExprKind::Local`] whose id is exactly the state slot the runtime
/// mutates in place (locals map 1:1 to slots). Any other shape is a sema bug.
fn rng_slot(rng: &hir::Expr) -> Slot {
    match &rng.kind {
        hir::ExprKind::Local(id) => *id,
        _ => unreachable!("an rng method receiver is a bound mut local (sema-checked)"),
    }
}

/// Lower a block; returns its trailing value operand if any. If a statement diverges
/// (e.g. `return`), the current block becomes terminated and the rest of the block —
/// including its trailing value — is dead code and is not lowered.
fn lower_block(b: &mut Builder, block: &hir::Block) -> Option<Operand> {
    for s in &block.stmts {
        lower_stmt(b, s);
        if b.is_terminated() {
            return None;
        }
    }
    block.value.as_ref().map(|e| lower_expr(b, e))
}

fn lower_stmt(b: &mut Builder, s: &hir::Stmt) {
    match s {
        hir::Stmt::Let { local, init } => match &init.kind {
            // A struct literal initializes its slot field by field; there is no scalar value to
            // bind. A nested struct-literal field is expanded in place (its leaves stored at the
            // extended path), so no intermediate struct value is materialized.
            hir::ExprKind::StructLit { .. } => store_value_at(b, *local, &mut Vec::new(), init),
            // An array literal stores its elements into the slot.
            hir::ExprKind::ArrayLit { elems, elem } => store_array_elems(b, *local, elems, *elem),
            _ => {
                let op = lower_expr(b, init);
                b.push(Stmt::Store(*local, op));
                // If the initializer moved an owned local, null its slot (drop-flag).
                null_moved_source(b, init);
            }
        },
        hir::Stmt::Assign { local, value, drop_old } => {
            // Compute the new value first (the RHS may read the old). Then, if reassigning an owned
            // local whose old value the RHS did not move out (`drop_old`, set by sema's move
            // analysis), free the buffer being overwritten — else it leaks. `drop_locals` excludes
            // arena-owned locals (the arena bulk-frees those). The slot is a valid buffer or null
            // (a prior move / the entry `DropFlagInit`), so the drop frees once or no-ops.
            let op = lower_expr(b, value);
            if drop_old.get() && b.drop_locals.contains(local) {
                b.push(Stmt::Drop(*local));
            }
            b.push(Stmt::Store(*local, op));
            null_moved_source(b, value);
        }
        hir::Stmt::AssignField { root, path, value } => {
            // `root.f0.… = value`. A struct-literal value is expanded in place at the path (its
            // leaves stored under the extended path); a scalar value is a single field store.
            // If the leaf being overwritten is an owned `string` field, free the OLD value first
            // (else it leaks — Slice 3 handled whole-struct reassign, not field-level) and null the
            // RHS's moved source so it isn't double-freed. (Slice 4b: an owned nested-struct value
            // `u.addr = Address{…}` still expands via `store_value_at` — its own owned fields are a
            // later slice; only a direct `string` leaf is drop-of-old'd here.)
            let drop_old_field = !matches!(value.kind, hir::ExprKind::StructLit { .. }) && field_ty_at(b, *root, path) == Ty::String;
            if drop_old_field {
                let old = b.fresh_value(Ty::String);
                b.push(Stmt::Let(old, Rvalue::Field(*root, path.clone())));
                b.push(Stmt::DropValue(Operand::Value(old)));
            }
            store_value_at(b, *root, &mut path.clone(), value);
            // Null the RHS's moved source *after* the store — `store_value_at` lowers `value`
            // internally, so nulling a variable RHS beforehand would store null. (The old value was
            // already freed above, before the overwrite.)
            if drop_old_field {
                null_moved_source(b, value);
            }
        }
        hir::Stmt::AssignIndex { base, index, value } => {
            // `base[index] = value` — bounds-checked element store (abort on out-of-range, like a
            // read). A `{ptr,len}` slice/owned-array writes through its buffer pointer; a fixed
            // stack array writes its slot directly.
            let idx = lower_expr(b, index);
            let val = lower_expr(b, value);
            let base_ty = b.slots[*base as usize];
            match base_ty {
                Ty::Slice(s) | Ty::DynArray(s) => {
                    let sv = b.fresh_value(base_ty);
                    b.push(Stmt::Let(sv, Rvalue::Load(*base)));
                    let len = b.fresh_value(i64_ty());
                    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(sv))));
                    emit_bounds_check(b, &idx, Operand::Value(len));
                    let ptr = b.fresh_value(Ty::Box(s));
                    b.push(Stmt::Let(ptr, Rvalue::SlicePtr(Operand::Value(sv))));
                    b.push(Stmt::PtrStore(Operand::Value(ptr), idx, val));
                }
                Ty::Array(_, n) => {
                    emit_bounds_check(b, &idx, Operand::Const(Const::Int(n as i128, i64_ty())));
                    b.push(Stmt::StoreIndex(*base, idx, val));
                }
                other => unreachable!("element assignment into non-array/slice {other:?}"),
            }
        }
        hir::Stmt::AssignElemField { base, index, path, struct_id, soa, value } => {
            // `base[index].f0.f1.… = value` — bounds-checked element-(nested-)field store (the write
            // counterpart of the `base[index].f0.f1.…` read). A `soa<Struct>` writes one column
            // (`StoreColumn`, the column-major `align_up` offset chain; soa columns are scalar, so
            // its path is always length 1); a fixed `array<Struct>` writes its slot element-field via
            // `StoreElemField` (a `[0,index,*path]` GEP); an owned dynamic `array<Struct>` writes
            // through the buffer pointer (`StoreElemFieldPtr`, a `[index,*path]` GEP).
            let idx = lower_expr(b, index);
            let val = lower_expr(b, value);
            if *soa {
                let soa_ty = b.slots[*base as usize];
                let sv = b.fresh_value(soa_ty);
                b.push(Stmt::Let(sv, Rvalue::Load(*base)));
                let len = b.fresh_value(i64_ty());
                b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(sv))));
                emit_bounds_check(b, &idx, Operand::Value(len));
                // The column buffer's element-pointer type is opaque, so the `Box` scalar is
                // irrelevant (matches `transpose_to_soa`) — use the first field's. A soa struct
                // always has ≥1 field (sema enforces non-empty).
                let first_field = b.structs[*struct_id as usize].fields.first().expect("a soa struct has at least one field");
                let first_scalar = align_sema::ty_to_scalar(first_field.ty).expect("soa field is a scalar");
                let ptr = b.fresh_value(Ty::Box(first_scalar));
                b.push(Stmt::Let(ptr, Rvalue::SlicePtr(Operand::Value(sv))));
                // A soa column is scalar, so sema restricts the path to a single field.
                b.push(Stmt::StoreColumn {
                    base: Operand::Value(ptr),
                    len: Operand::Value(len),
                    index: idx,
                    field: path[0],
                    struct_id: *struct_id,
                    value: val,
                });
            } else {
                match b.slots[*base as usize] {
                    // A fixed `array<Struct>` slot (sema restricts the receiver to a `mut` local).
                    Ty::StructArray(_, n) => {
                        emit_bounds_check(b, &idx, Operand::Const(Const::Int(n as i128, i64_ty())));
                        // An owned `string` leaf field being overwritten: free the OLD value first
                        // (else it leaks) and null the RHS's moved source. A scalar leaf needs
                        // neither. (Slice 4b.)
                        if field_path_leaf_ty(&b.structs, *struct_id, path) == Ty::String {
                            b.push(Stmt::DropElemField(*base, idx.clone(), path.clone()));
                            null_moved_source(b, value);
                        }
                        b.push(Stmt::StoreElemField(*base, idx, path.clone(), val));
                    }
                    // An owned, dynamic `array<Struct>` (`DynStructArray`) — a `{ptr,len}` view
                    // addressed through its buffer pointer. Load the view, bounds-check against its
                    // runtime length, then store the scalar leaf field via the pointer-based write
                    // dual of `IndexFieldPtr`. Sema restricts the leaf to a scalar (POD), so there is
                    // no old-value drop / moved-source concern (unlike the fixed `string`-field path).
                    Ty::DynStructArray(..) => {
                        let view_ty = b.slots[*base as usize];
                        let sv = b.fresh_value(view_ty);
                        b.push(Stmt::Let(sv, Rvalue::Load(*base)));
                        let len = b.fresh_value(i64_ty());
                        b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(sv))));
                        emit_bounds_check(b, &idx, Operand::Value(len));
                        b.push(Stmt::StoreElemFieldPtr {
                            base: Operand::Value(sv),
                            index: idx,
                            path: path.clone(),
                            struct_id: *struct_id,
                            value: val,
                        });
                    }
                    other => unreachable!("soa=false element-field assignment into {other:?}"),
                }
            }
        }
        hir::Stmt::AssignElem { base, index, struct_id, soa, value } => {
            // `base[index] = value` — bounds-checked whole-element store (the write counterpart of
            // the `base[index]` read / `s[i]` gather). A `soa<Struct>` scatters the value's fields
            // into their columns (`StoreColumn` per field); a fixed `array<Struct>` stores the whole
            // struct aggregate into element `index` (`StoreIndex`, a `[0,index]` GEP). The struct is
            // plain-old-data (sema gate), so the value is a Copy aggregate — no per-field drop.
            let idx = lower_expr(b, index);
            let val = lower_expr(b, value);
            if *soa {
                let soa_ty = b.slots[*base as usize];
                let sv = b.fresh_value(soa_ty);
                b.push(Stmt::Let(sv, Rvalue::Load(*base)));
                let len = b.fresh_value(i64_ty());
                b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(sv))));
                emit_bounds_check(b, &idx, Operand::Value(len));
                // Snapshot the field types once (each is Copy) so the scatter loop neither
                // re-indexes `b.structs` nor holds a borrow of `b` across its `b.push` calls.
                let field_tys: Vec<Ty> = b.structs[*struct_id as usize].fields.iter().map(|f| f.ty).collect();
                let first_scalar = align_sema::ty_to_scalar(*field_tys.first().expect("a soa struct has at least one field"))
                    .expect("soa field is a scalar");
                let ptr = b.fresh_value(Ty::Box(first_scalar));
                b.push(Stmt::Let(ptr, Rvalue::SlicePtr(Operand::Value(sv))));
                // Materialize the struct value into a temp slot, then read each field out and scatter
                // it into its column (columns are non-contiguous, so no single aggregate store).
                let tmp = b.new_slot(Ty::Struct(*struct_id));
                b.push(Stmt::Store(tmp, val));
                for (field, &fty) in field_tys.iter().enumerate() {
                    let field = field as u32;
                    let fv = b.fresh_value(fty);
                    b.push(Stmt::Let(fv, Rvalue::Field(tmp, vec![field])));
                    b.push(Stmt::StoreColumn {
                        base: Operand::Value(ptr),
                        len: Operand::Value(len),
                        index: idx.clone(),
                        field,
                        struct_id: *struct_id,
                        value: Operand::Value(fv),
                    });
                }
            } else {
                // A fixed `array<Struct>` slot: one aggregate store into element `index`.
                let n = match b.slots[*base as usize] {
                    Ty::StructArray(_, n) => n,
                    other => unreachable!("soa=false whole-element assignment into {other:?}"),
                };
                emit_bounds_check(b, &idx, Operand::Const(Const::Int(n as i128, i64_ty())));
                // A Move-struct element: free the *old* element's owned fields before overwriting it
                // (else its buffers leak), and null the RHS's moved source so its own drop is a no-op
                // (no double-free). A POD element needs neither. (Slice 4b.)
                if struct_is_move(*struct_id, &b.structs) {
                    b.push(Stmt::DropElem(*base, idx.clone(), *struct_id));
                    null_moved_source(b, value);
                }
                b.push(Stmt::StoreIndex(*base, idx, val));
            }
        }
        // `v[lane] = value` → `v = insertelement(v, value, lane)` (a vector is a register value).
        hir::Stmt::AssignVecLane { local, lane, value } => {
            let val = lower_expr(b, value);
            let vty = b.slots[*local as usize];
            let cur = b.fresh_value(vty);
            b.push(Stmt::Let(cur, Rvalue::Load(*local)));
            let newv = b.fresh_value(vty);
            b.push(Stmt::Let(newv, Rvalue::VecInsert { vec: Operand::Value(cur), value: val, lane: *lane }));
            b.push(Stmt::Store(*local, Operand::Value(newv)));
        }
        hir::Stmt::Return(value) => {
            let op = value.as_ref().map(|e| lower_expr(b, e));
            // A returned owned array is moved out: null its slot so the exit cleanup below frees
            // null (the caller now owns the buffer), then free open arenas / drop owned locals.
            if let Some(e) = value {
                null_moved_source(b, e);
            }
            b.emit_exit_cleanup();
            b.terminate(Term::Return(op));
            // The current block is now terminated; `lower_block` stops here, so no dead
            // block is created and callers can see the divergence via `is_terminated`.
        }
        hir::Stmt::Break(value) => {
            // `break e` ends the innermost loop: evaluate `e`, store it into the loop's result slot,
            // then jump to the loop's exit. A moved-out owned value has its source slot nulled so the
            // per-iteration drops below (and the exit cleanup) free null, not the transferred buffer.
            let op = value.as_ref().map(|e| lower_expr(b, e));
            let frame = b.loops.last().expect("`break` inside a `loop` (sema-checked)");
            let (result_slot, iter_drops) = (frame.result_slot, frame.iter_drops.clone());
            if let (Some(slot), Some(op)) = (result_slot, op) {
                b.push(Stmt::Store(slot, op));
            }
            if let Some(e) = value {
                null_moved_source(b, e);
            }
            // Drop this iteration's per-iteration owned locals (null-resetting so the function-exit
            // cleanup — these are still in `drop_locals` — frees null, not the freed buffer). The
            // moved-out `break` value was already nulled, so its `Drop` is a no-op. Sema forbids a
            // `break` inside an `arena`/`task_group` nested in the loop, so no region unwinding here.
            for s in &iter_drops {
                b.push(Stmt::Drop(*s));
                b.push(Stmt::DropFlagInit(*s));
            }
            let exit = b.loops.last().unwrap().exit;
            b.terminate(Term::Goto(exit));
        }
        hir::Stmt::LetTuple { locals, init, .. } => {
            // Evaluate the tuple once, then extract each bound element into its slot (`_` skipped).
            let tup = lower_expr(b, init);
            // If the tuple was built from owned source locals (`(x, y) := (a, b)`), null them: the
            // destructure targets now own the buffers, so the source slots must not also free them.
            null_moved_source(b, init);
            for (i, lid) in locals.iter().enumerate() {
                if let Some(lid) = lid {
                    let ety = b.slots[*lid as usize];
                    let v = b.fresh_value(ety);
                    b.push(Stmt::Let(v, Rvalue::TupleIndex { tuple: tup.clone(), index: i as u32 }));
                    b.push(Stmt::Store(*lid, Operand::Value(v)));
                }
            }
        }
        hir::Stmt::Expr(e) => {
            let _ = lower_expr(b, e);
        }
    }
}

fn lower_expr(b: &mut Builder, e: &hir::Expr) -> Operand {
    match &e.kind {
        hir::ExprKind::Unit => Operand::Const(Const::Unit),
        hir::ExprKind::Int(v) => Operand::Const(Const::Int(*v, e.ty)),
        hir::ExprKind::Float(v) => Operand::Const(Const::Float(*v, e.ty)),
        hir::ExprKind::Char(v) => Operand::Const(Const::Char(*v)),
        hir::ExprKind::Str(s) => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::StrLit(s.clone())));
            Operand::Value(v)
        }
        hir::ExprKind::Template(parts) => {
            let mut pieces = Vec::new();
            for p in parts {
                match p {
                    hir::TemplatePart::Text(s) => pieces.push(TemplatePiece::Static(s.clone())),
                    hir::TemplatePart::Hole(h) => {
                        let ty = h.ty;
                        let op = lower_expr(b, h);
                        pieces.push(match ty {
                            Ty::Str => TemplatePiece::StrHole(op),
                            Ty::Bool => TemplatePiece::BoolHole(op),
                            Ty::Char => TemplatePiece::CharHole(op),
                            Ty::Float(_) => TemplatePiece::FloatHole(op),
                            _ => TemplatePiece::IntHole(op),
                        });
                    }
                    hir::TemplatePart::JsonStr(h) => {
                        let op = lower_expr(b, h);
                        pieces.push(TemplatePiece::JsonStrHole(op));
                    }
                }
            }
            let arena = b.arenas.last().map(|h| Operand::Value(*h));
            let r = b.fresh_value(e.ty);
            b.push(Stmt::Let(r, Rvalue::Template(pieces, arena)));
            Operand::Value(r)
        }
        hir::ExprKind::JsonDecode { struct_id, input } => lower_json_decode(b, *struct_id, input, e.ty),
        hir::ExprKind::JsonDecodeArray { elem, input } => lower_json_decode_array(b, *elem, input, e.ty),
        hir::ExprKind::JsonDecodeStructArray { struct_id, input } => lower_json_decode_struct_array(b, *struct_id, input, e.ty),
        hir::ExprKind::JsonDecodeSoa { struct_id, input } => lower_json_decode_soa(b, *struct_id, input, e.ty),
        hir::ExprKind::FsReadFile { path } => lower_fs_read_file(b, path, e.ty),
        // `fs.open` / `fs.create` — the runtime writes the reader/writer handle into `out` and
        // returns an errno-status; wrap into `Result<reader/writer, Error>` (like `fs.read_file`).
        hir::ExprKind::ReaderOpen { path } => lower_open_handle(b, path, Ty::Reader, e.ty, |p, out| Rvalue::ReaderOpen { path: p, out }),
        hir::ExprKind::WriterCreate { path } => lower_open_handle(b, path, Ty::Writer, e.ty, |p, out| Rvalue::WriterCreate { path: p, out }),
        hir::ExprKind::ReaderStdin => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ReaderStdin));
            Operand::Value(v)
        }
        hir::ExprKind::WriterStd { fd, buffered } => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::WriterStd { fd: *fd, buffered: *buffered }));
            Operand::Value(v)
        }
        // `r.read(b)` yields `Result<i64, Error>` from the runtime's i64: a count `>= 0` is `Ok`,
        // a `< 0` value encodes `-(status)` for the `Err`.
        hir::ExprKind::ReaderRead { reader, buffer } => {
            let rop = lower_expr(b, reader);
            let bop = lower_expr(b, buffer);
            lower_reader_read(b, rop, bop, e.ty)
        }
        // `io.copy(r, w)` yields `Result<i64, Error>` from the runtime's i64 (bytes transferred, or
        // `-(status)` on error) — the same sign convention (and lowering) as `reader.read`.
        hir::ExprKind::IoCopy { reader, writer } => {
            let rop = lower_expr(b, reader);
            let wop = lower_expr(b, writer);
            let n = b.fresh_value(i64_ty());
            b.push(Stmt::Let(n, Rvalue::IoCopy(rop, wop)));
            lower_count_or_status_result(b, n, e.ty)
        }
        // `w.write(x)` / `w.flush()` yield `Result<(), Error>` from an i32 errno-status.
        hir::ExprKind::WriterWrite { writer, arg, builder } => {
            let wop = lower_expr(b, writer);
            let aop = lower_expr(b, arg);
            let mk: Box<dyn FnOnce(&mut Builder) -> ValueId> = if *builder {
                Box::new(move |b| { let v = b.fresh_value(status_ty()); b.push(Stmt::Let(v, Rvalue::WriterWriteBuilder(wop, aop))); v })
            } else {
                Box::new(move |b| { let v = b.fresh_value(status_ty()); b.push(Stmt::Let(v, Rvalue::WriterWrite(wop, aop))); v })
            };
            let code = mk(b);
            lower_status_result(b, code, e.ty)
        }
        hir::ExprKind::WriterFlush { writer } => {
            let wop = lower_expr(b, writer);
            let code = b.fresh_value(status_ty());
            b.push(Stmt::Let(code, Rvalue::WriterFlush(wop)));
            lower_status_result(b, code, e.ty)
        }
        hir::ExprKind::BufferNew { capacity } => {
            let cap = lower_expr(b, capacity);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BufferNew(cap)));
            Operand::Value(v)
        }
        hir::ExprKind::BufferBytes { buffer } => {
            let bop = lower_expr(b, buffer);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BufferBytes(bop)));
            Operand::Value(v)
        }
        hir::ExprKind::BufferLen { buffer } => {
            let bop = lower_expr(b, buffer);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BufferLen(bop)));
            Operand::Value(v)
        }
        hir::ExprKind::BytesRead { bytes, offset, be } => lower_bytes_read(b, bytes, offset, *be, e.ty),
        hir::ExprKind::BufferPut { buffer, value, be } => lower_buffer_put(b, buffer, value, *be),
        hir::ExprKind::BufferAppend { buffer, data } => lower_buffer_append(b, buffer, data),
        // `fs.write_file(path, data)` yields `Result<(), Error>` from an i32 errno-status (str/bytes
        // vs builder pick the runtime fn, like `writer.write`).
        hir::ExprKind::FsWriteFile { path, data, builder } => {
            let pop = lower_expr(b, path);
            let dop = lower_expr(b, data);
            let code = b.fresh_value(status_ty());
            let rv = if *builder {
                Rvalue::FsWriteFileBuilder { path: pop, builder: dop }
            } else {
                Rvalue::FsWriteFile { path: pop, data: dop }
            };
            b.push(Stmt::Let(code, rv));
            lower_status_result(b, code, e.ty)
        }
        // `fs.exists(path)` yields a plain `bool` — the runtime's `1`/`0` compared `!= 0` (every
        // error already folded to `0`, so there is no status branch).
        hir::ExprKind::FsExists { path } => {
            let pop = lower_expr(b, path);
            let c = b.fresh_value(status_ty());
            b.push(Stmt::Let(c, Rvalue::FsExists { path: pop }));
            let v = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(v, Rvalue::Bin(BinOp::Ne, Operand::Value(c), Operand::Const(Const::Int(0, status_ty())))));
            Operand::Value(v)
        }
        // `fs.remove(path)` yields `Result<(), Error>` from an i32 errno-status.
        hir::ExprKind::FsRemove { path } => {
            let pop = lower_expr(b, path);
            let code = b.fresh_value(status_ty());
            b.push(Stmt::Let(code, Rvalue::FsRemove { path: pop }));
            lower_status_result(b, code, e.ty)
        }
        // `fs.read_dir(path)` yields `Result<array<string>, Error>` — the same out-slot shape as
        // `fs.read_file`, with an owned `DynArray(String)` payload.
        hir::ExprKind::FsReadDir { path } => lower_fs_read_dir(b, path, e.ty),
        // `dns.resolve(host)` yields `Result<array<string>, Error>` — identical lowering to
        // `fs.read_dir` (owned `DynArray(String)` payload, deep-`Drop`), only the runtime call differs.
        hir::ExprKind::DnsResolve { host } => lower_dns_resolve(b, host, e.ty),
        hir::ExprKind::TcpConnect { host, port } => lower_tcp_connect(b, host, port, e.ty),
        // `c.reader()` / `c.writer()` wrap the conn's fd in a borrowed reader/writer (`owns_fd:false`).
        hir::ExprKind::ConnReader { conn } => {
            let c = lower_expr(b, conn);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ConnReader(c)));
            Operand::Value(v)
        }
        hir::ExprKind::ConnWriter { conn } => {
            let c = lower_expr(b, conn);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ConnWriter(c)));
            Operand::Value(v)
        }
        hir::ExprKind::TcpListen { host, port } => lower_tcp_listen(b, host, port, e.ty),
        hir::ExprKind::TcpAccept { listener } => lower_tcp_accept(b, listener, e.ty),
        hir::ExprKind::UdpBind { host, port } => lower_udp_bind(b, host, port, e.ty),
        hir::ExprKind::UdpSendTo { sock, data, host, port } => lower_udp_send_to(b, sock, data, host, port, e.ty),
        hir::ExprKind::UdpRecvFrom { sock, buffer } => lower_udp_recv_from(b, sock, buffer, e.ty),
        hir::ExprKind::ProcessSpawn { cmd, args } => lower_process_spawn(b, cmd, args, e.ty),
        hir::ExprKind::ChildWait { child } => lower_child_wait(b, child, e.ty),
        hir::ExprKind::ChildKill { child, sig } => lower_child_kill(b, child, sig, e.ty),
        hir::ExprKind::ProcessExec { cmd, args } => lower_process_exec(b, cmd, args, e.ty),
        // `fs.read_file_view(path)` yields `Result<str, Error>`, threading the enclosing arena so the
        // runtime registers the mmap for `munmap` at arena end.
        hir::ExprKind::FsReadFileView { path } => lower_fs_read_file_view(b, path, e.ty),
        hir::ExprKind::FsReadBytesView { path } => lower_fs_read_bytes_view(b, path, e.ty),
        // `path.join(a, b)` → an owned `string` `{ptr,len}` returned by value (like `str_clone`).
        hir::ExprKind::PathJoin { a, b: pb } => {
            let ao = lower_expr(b, a);
            let bo = lower_expr(b, pb);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::PathJoin { a: ao, b: bo }));
            Operand::Value(v)
        }
        // `path.base`/`dir`/`ext(p)` → a borrowed sub-`str` `{ptr,len}` of `p` returned by value.
        hir::ExprKind::PathComponent { kind, path } => {
            let po = lower_expr(b, path);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::PathComponent { kind: *kind, path: po }));
            Operand::Value(v)
        }
        // `path.normalize(p)` → an owned `string` `{ptr,len}` returned by value.
        hir::ExprKind::PathNormalize { path } => {
            let po = lower_expr(b, path);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::PathNormalize { path: po }));
            Operand::Value(v)
        }
        // `env.get(name)` → `Option<string>`: the runtime writes the owned value into `out` and
        // returns a present flag; branch `Some(<value>)` / `None`.
        hir::ExprKind::EnvGet { name } => lower_env_get(b, name, e.ty),
        // `env.set(name, value)` → `Result<(), Error>` from an i32 errno-status.
        hir::ExprKind::EnvSet { name, value } => {
            let no = lower_expr(b, name);
            let vo = lower_expr(b, value);
            let code = b.fresh_value(status_ty());
            b.push(Stmt::Let(code, Rvalue::EnvSet { name: no, value: vo }));
            lower_status_result(b, code, e.ty)
        }
        // `time.now()` / `time.instant()` → an `i64` returned by value.
        hir::ExprKind::TimeNow => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::TimeNow));
            Operand::Value(v)
        }
        hir::ExprKind::TimeInstant => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::TimeInstant));
            Operand::Value(v)
        }
        // `time.sleep(ns)` → `()`; emit the void call and yield unit.
        hir::ExprKind::TimeSleep { ns } => {
            let no = lower_expr(b, ns);
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::TimeSleep { ns: no }));
            Operand::Const(Const::Unit)
        }
        // `process.exit(code)` — the settled cleanup-then-exit path: evaluate `code`, run the current
        // function's pending cleanup (drops for live owned locals + `task_group`/arena ends — the
        // exact emission a `return` uses, so buffered writers flush in their `Drop`), then a diverging
        // runtime `align_rt_process_exit(code)` (void `-> !`, like `bounds_fail`). The block is then
        // `Unreachable`; `lower_block`/`lower_fn` observe `is_terminated` and emit no code after it
        // (#274 lesson). v1 gap: only the CURRENT frame's cleanup runs — full multi-frame unwind is
        // deferred (`docs/impl/std-design/process.md`).
        hir::ExprKind::ProcessExit { code } => {
            let c = lower_expr(b, code);
            b.emit_exit_cleanup();
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::Call("process_exit".to_string(), vec![c])));
            b.terminate(Term::Unreachable);
            Operand::Const(Const::Unit)
        }
        // `process.abort()` — the named escape hatch: NO cleanup emission, a bare diverging
        // `align_rt_process_abort()` (`_exit`), then `Unreachable`.
        hir::ExprKind::ProcessAbort => {
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::Call("process_abort".to_string(), vec![])));
            b.terminate(Term::Unreachable);
            Operand::Const(Const::Unit)
        }
        // `encoding.*_encode(data)` → an owned `string` `{ptr,len}` returned by value (like
        // `PathNormalize`); the runtime allocates the buffer, the bound local `Drop`-frees it.
        hir::ExprKind::EncodingEncode { kind, data } => {
            let d = lower_expr(b, data);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::EncodingEncode { kind: *kind, data: d }));
            Operand::Value(v)
        }
        // `encoding.*_decode(s)` → `Result<buffer, Error>`: the runtime writes an owned `buffer`
        // handle into `out` and returns an i32 status; branch `Ok(<buffer>)` / `Err(<mapped>)`.
        hir::ExprKind::EncodingDecode { kind, input } => lower_encoding_decode(b, *kind, input, e.ty),
        // `compress.gzip_compress(data, level)` / `gzip_decompress(data)` → `Result<buffer, Error>`:
        // the runtime writes an owned `buffer` handle into `out` + returns an i32 status; branch
        // `Ok(<buffer>)` / `Err(<mapped>)` via the shared status→result helper.
        hir::ExprKind::Compress { kind, data, level } => {
            let out = b.new_slot(Ty::Buffer);
            let d = lower_expr(b, data);
            let lv = lower_expr(b, level);
            let code = b.fresh_value(status_ty());
            b.push(Stmt::Let(code, Rvalue::CompressCompress { kind: *kind, data: d, level: lv, out }));
            emit_status_buffer_result(b, code, out, e.ty)
        }
        hir::ExprKind::Decompress { kind, data } => {
            let out = b.new_slot(Ty::Buffer);
            let d = lower_expr(b, data);
            let code = b.fresh_value(status_ty());
            b.push(Stmt::Let(code, Rvalue::CompressDecompress { kind: *kind, data: d, out }));
            emit_status_buffer_result(b, code, out, e.ty)
        }
        // `encoding.utf8_valid(b)` → the runtime returns an `i32` (1/0); compare `!= 0` to a `bool`
        // (the same i32→bool bridge as `fs.exists`).
        hir::ExprKind::Utf8Valid { data } => {
            let d = lower_expr(b, data);
            let c = b.fresh_value(status_ty());
            b.push(Stmt::Let(c, Rvalue::Utf8Valid { data: d }));
            let v = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(v, Rvalue::Bin(BinOp::Ne, Operand::Value(c), Operand::Const(Const::Int(0, status_ty())))));
            Operand::Value(v)
        }
        // `crypto.constant_time_equal(a, b)` → the runtime returns an `i32` (1/0); compare `!= 0` to
        // a `bool` (the same i32→bool bridge as `utf8_valid`). Both operands are byte views.
        hir::ExprKind::CryptoCtEqual { a, b: bb } => {
            let av = lower_expr(b, a);
            let bv = lower_expr(b, bb);
            let c = b.fresh_value(status_ty());
            b.push(Stmt::Let(c, Rvalue::CryptoCtEqual { a: av, b: bv }));
            let v = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(v, Rvalue::Bin(BinOp::Ne, Operand::Value(c), Operand::Const(Const::Int(0, status_ty())))));
            Operand::Value(v)
        }
        // `crypto.random(out)` → a void runtime call that fills the buffer in place; the expression
        // value is `()` (the same shape as `time.sleep`).
        hir::ExprKind::CryptoRandom { out } => {
            let o = lower_expr(b, out);
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::CryptoRandom { out: o }));
            Operand::Const(Const::Unit)
        }
        // `crypto.sha256(data)` / `crypto.sha512(data)` → a fresh owned `array<u8>` `{ptr,len}`
        // returned by value; the bound local `Drop`-frees it (same shape as `rand.sample`). The
        // runtime allocates the digest buffer + aborts on an engine failure.
        hir::ExprKind::CryptoHash { algo, data } => {
            let dv = lower_expr(b, data);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CryptoHash { algo: *algo, data: dv }));
            Operand::Value(v)
        }
        // `crypto.hmac_sha256(key, data)` / `crypto.hkdf_sha256(salt, ikm, info, len)` → out-of-line
        // helpers. Their bodies bind several locals; kept out of this recursive `match` so those slots
        // do not inflate `lower_expr`'s (per-recursion-level) stack frame in debug builds — a deep
        // expression tree recurses through this function, and rustc reserves every arm's locals up
        // front (the `expr_depth` headroom the #296 cap was measured against).
        hir::ExprKind::CryptoHmac { key, data } => lower_crypto_hmac(b, key, data, e.ty),
        hir::ExprKind::CryptoHkdf { salt, ikm, info, len } => lower_crypto_hkdf(b, salt, ikm, info, len, e.ty),
        hir::ExprKind::CryptoAead { cipher, dir, key, nonce, input, aad } => {
            lower_crypto_aead(b, *cipher, *dir, key, nonce, input, aad, e.ty)
        }
        hir::ExprKind::CryptoArgon2 { password, salt, params } => {
            lower_crypto_argon2(b, password, salt, params, e.ty)
        }
        // `rand.seed()` / `rand.seed_with(s)` → initialize the `rng` state into a temp slot (the
        // runtime writes through the pointer), then load the `[4 x i64]` aggregate as the value.
        hir::ExprKind::RandSeed | hir::ExprKind::RandSeedWith { .. } => {
            let seed = match &e.kind {
                hir::ExprKind::RandSeedWith { seed } => Some(lower_expr(b, seed)),
                _ => None,
            };
            let out = b.new_slot(Ty::Rng);
            let dummy = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(dummy, Rvalue::RandSeed { seed, out }));
            let v = b.fresh_value(Ty::Rng);
            b.push(Stmt::Let(v, Rvalue::Load(out)));
            Operand::Value(v)
        }
        // `r.next()` — the receiver is a bound mut local (sema-checked); its slot id *is* the rng
        // state, mutated in place by the runtime through a pointer.
        hir::ExprKind::RandNext { rng } => {
            let slot = rng_slot(rng);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::RandNext { rng: slot }));
            Operand::Value(v)
        }
        // `r.range(lo, hi)` — advance the rng, return a uniform i64 in `[lo, hi)`.
        hir::ExprKind::RandRange { rng, lo, hi } => {
            let slot = rng_slot(rng);
            let lo = lower_expr(b, lo);
            let hi = lower_expr(b, hi);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::RandRange { rng: slot, lo, hi }));
            Operand::Value(v)
        }
        // `r.shuffle(out xs)` — Fisher-Yates the slice in place; no value.
        hir::ExprKind::RandShuffle { rng, xs, elem } => {
            let slot = rng_slot(rng);
            let xv = lower_expr(b, xs);
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::RandShuffle { rng: slot, xs: xv, elem: *elem }));
            Operand::Const(Const::Unit)
        }
        // `r.sample(xs, k)` → a fresh owned `array<T>` `{ptr,len}` returned by value; the bound local
        // `Drop`-frees it. The runtime allocates + validates `k` (aborts on `k < 0` / `k > len`).
        hir::ExprKind::RandSample { rng, xs, k, elem } => {
            let slot = rng_slot(rng);
            let xv = lower_expr(b, xs);
            let kv = lower_expr(b, k);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::RandSample { rng: slot, xs: xv, k: kv, elem: *elem }));
            Operand::Value(v)
        }
        // `cli.command(name)` → an owned `cli command` handle returned by value (the bound local
        // `Drop`-frees it via `cli_command_free`).
        hir::ExprKind::CliCommand { name } => {
            let nm = lower_expr(b, name);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CliCommand { name: nm }));
            Operand::Value(v)
        }
        // `c.flag_*(...)` → register a flag into the command handle in place; no value. The receiver
        // is a bound local (sema-checked); its loaded pointer value is the handle the runtime mutates.
        hir::ExprKind::CliFlag { cmd, kind, name, default } => {
            let cop = lower_expr(b, cmd);
            let nm = lower_expr(b, name);
            let def = default.as_ref().map(|d| lower_expr(b, d));
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::CliFlag { cmd: cop, kind: *kind, name: nm, default: def }));
            Operand::Const(Const::Unit)
        }
        // `c.parse(args)` → `Result<parsed, Error>`: the runtime writes an owned `parsed` handle into
        // `out` and returns an i32 status; branch `Ok(<parsed>)` / `Err(<mapped>)`.
        hir::ExprKind::CliParse { cmd, args } => lower_cli_parse(b, cmd, args, e.ty),
        // `p.get_bool(name)` → the runtime returns an i32 (1/0); compare `!= 0` to a `bool`.
        hir::ExprKind::CliGetBool { parsed, name } => {
            let pop = lower_expr(b, parsed);
            let nm = lower_expr(b, name);
            let c = b.fresh_value(status_ty());
            b.push(Stmt::Let(c, Rvalue::CliGetBool { parsed: pop, name: nm }));
            let v = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(v, Rvalue::Bin(BinOp::Ne, Operand::Value(c), Operand::Const(Const::Int(0, status_ty())))));
            Operand::Value(v)
        }
        // `p.get_i64(name)` → the runtime returns the i64 value directly.
        hir::ExprKind::CliGetI64 { parsed, name } => {
            let pop = lower_expr(b, parsed);
            let nm = lower_expr(b, name);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CliGetI64 { parsed: pop, name: nm }));
            Operand::Value(v)
        }
        // `p.get_str(name)` → a `str` view `{ptr,len}` into the parsed handle's storage (region-bound
        // to `parsed`; not owned — no `Drop`).
        hir::ExprKind::CliGetStr { parsed, name } => {
            let pop = lower_expr(b, parsed);
            let nm = lower_expr(b, name);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CliGetStr { parsed: pop, name: nm }));
            Operand::Value(v)
        }
        // `c.usage()` → an owned `string` `{ptr,len}` returned by value (the bound local `Drop`-frees it).
        hir::ExprKind::CliUsage { cmd } => {
            let cop = lower_expr(b, cmd);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CliUsage { cmd: cop }));
            Operand::Value(v)
        }
        // `std.http` (Slice 1) — the seven request/response ops collapse into ONE `lower_expr` arm
        // delegating to a single out-of-line (`#[inline(never)]`) dispatcher, so this giant recursive
        // match grows by exactly one arm (not seven). A deep expression tree recurses through
        // `lower_expr`, and rustc reserves every arm's locals per level (the `expr_depth` #296 headroom
        // lesson — std.http is the last std module, so the cap's remaining headroom is thin).
        hir::ExprKind::HttpRequest { .. }
        | hir::ExprKind::HttpHeader { .. }
        | hir::ExprKind::HttpBody { .. }
        | hir::ExprKind::HttpParse { .. }
        | hir::ExprKind::HttpRespStatus { .. }
        | hir::ExprKind::HttpRespHeader { .. }
        | hir::ExprKind::HttpRespBody { .. }
        | hir::ExprKind::HttpClient
        | hir::ExprKind::HttpClientGet { .. }
        | hir::ExprKind::HttpClientPost { .. }
        | hir::ExprKind::HttpClientRequest { .. } => lower_http(b, e),
        hir::ExprKind::Bool(v) => Operand::Const(Const::Bool(*v)),
        hir::ExprKind::Local(id) => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Load(*id)));
            Operand::Value(v)
        }
        hir::ExprKind::Unary { op, expr } => {
            let a = lower_expr(b, expr);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Un(*op, a)));
            Operand::Value(v)
        }
        hir::ExprKind::Cast(inner) => {
            let from = inner.ty;
            let operand = lower_expr(b, inner);
            // A no-op cast (same type, e.g. `x as i32` where `x: i32`) is just the operand.
            if from == e.ty {
                return operand;
            }
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Cast { operand, from, to: e.ty }));
            Operand::Value(v)
        }
        hir::ExprKind::Binary { op, lhs, rhs } => {
            // `&&` / `||` short-circuit: the right operand is evaluated only when the left doesn't
            // already decide the result. Lower to a branch (not a strict `Rvalue::Bin`), so a guard
            // like `i < len && arr[i] > 0` doesn't evaluate `arr[i]` (and trap) when `i >= len`.
            if matches!(op, BinOp::And | BinOp::Or) {
                return lower_short_circuit(b, *op, lhs, rhs);
            }
            let l = lower_expr(b, lhs);
            let r = lower_expr(b, rhs);
            // `str + str` is concatenation, built like a two-piece template.
            if *op == BinOp::Add && lhs.ty == Ty::Str {
                let arena = b.arenas.last().map(|h| Operand::Value(*h));
                let v = b.fresh_value(e.ty);
                b.push(Stmt::Let(
                    v,
                    Rvalue::Template(vec![TemplatePiece::StrHole(l), TemplatePiece::StrHole(r)], arena),
                ));
                return Operand::Value(v);
            }
            // Integer `/` / `%` need a divisor guard: division by zero aborts, and signed
            // `INT_MIN / -1` (LLVM UB) wraps to the defined two's-complement result. `float`
            // division is IEEE (no guard).
            if matches!(op, BinOp::Div | BinOp::Rem) && matches!(lhs.ty, Ty::Int(_)) {
                return lower_int_div(b, *op, l, r, lhs.ty);
            }
            // A `vecN<T>` `/` / `%` carries the same divisor guard, lane-wise: any zero lane aborts,
            // and a signed `INT_MIN / -1` lane wraps. Float vectors are IEEE (no guard).
            if let (BinOp::Div | BinOp::Rem, Ty::Vec(s, n)) = (op, e.ty) {
                return lower_vec_div(b, *op, l, r, s, n, rhs.ty);
            }
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Bin(*op, l, r)));
            Operand::Value(v)
        }
        hir::ExprKind::IntArith { op, mode, lhs, rhs } => {
            let int_ty = lhs.ty;
            let a = lower_expr(b, lhs);
            let bb = lower_expr(b, rhs);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::IntArith { op: *op, mode: *mode, int_ty, a, b: bb }));
            Operand::Value(v)
        }
        hir::ExprKind::MathOp { fn_, operands } => {
            let ty = operands[0].ty;
            let ops: Vec<Operand> = operands.iter().map(|o| lower_expr(b, o)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MathOp { fn_: *fn_, ty, operands: ops }));
            Operand::Value(v)
        }
        hir::ExprKind::FnValue(name) => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::FnAddr(name.clone())));
            Operand::Value(v)
        }
        hir::ExprKind::Closure { lifted, captures } => {
            let capture_tys: Vec<Ty> = captures.iter().map(|c| c.ty).collect();
            let ops: Vec<Operand> = captures.iter().map(|c| lower_expr(b, c)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Closure { lifted: lifted.clone(), captures: ops, capture_tys }));
            Operand::Value(v)
        }
        hir::ExprKind::CallFnValue { callee, args } => {
            let c = lower_expr(b, callee);
            // The function type for the indirect call comes from the (sema-checked) arg types and
            // the call's result type — no signature table is threaded into MIR.
            let (param_tys, ops): (Vec<Ty>, Vec<Operand>) =
                args.iter().map(|a| (a.ty, lower_expr(b, a))).unzip();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CallIndirect { callee: c, args: ops, param_tys, ret_ty: e.ty }));
            Operand::Value(v)
        }
        hir::ExprKind::Call { func, args, .. } => {
            let ops = args.iter().map(|a| lower_expr(b, a)).collect();
            // A by-value owned-array argument is moved into the callee: null the caller's slot.
            // `print` / `hash64` / `hash128` only read their argument (they borrow the byte view),
            // so they must not null the source — it keeps living (matching the borrow in sema).
            if !matches!(func.as_str(), "print" | "hash64" | "hash128") {
                for a in args {
                    null_moved_source(b, a);
                }
            }
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Call(func.clone(), ops)));
            Operand::Value(v)
        }
        hir::ExprKind::If { cond, then, els } => lower_if(b, cond, then, els, e.ty),
        // Delegated to an out-of-line (`#[inline(never)]`) helper taking only `(b, e)` — no locals in
        // this arm — so it does not enlarge this giant recursive `lower_expr` frame (the `expr_depth`
        // headroom lesson: rustc reserves every arm's locals per level at opt-0).
        hir::ExprKind::Loop { .. } => lower_loop(b, e),
        // `Type.Variant(payload…)` — build the sum-type aggregate `{ i32 tag, … }`.
        hir::ExprKind::EnumValue { enum_id, variant, payload } => {
            let ops: Vec<Operand> = payload.iter().map(|p| lower_expr(b, p)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeEnum { enum_id: *enum_id, variant: *variant, payload: ops }));
            Operand::Value(v)
        }
        hir::ExprKind::Match { scrutinee, arms } => lower_match(b, scrutinee, arms, e.ty),
        hir::ExprKind::ResultMapErr { result, f } => lower_map_err(b, result, f, e.ty),
        hir::ExprKind::Field { root, path } => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Field(*root, path.clone())));
            Operand::Value(v)
        }
        hir::ExprKind::SoaColumn { base, struct_id, field } => {
            let v = b.fresh_value(e.ty); // slice<FieldTy>
            b.push(Stmt::Let(v, Rvalue::SoaColumn { base: *base, struct_id: *struct_id, field: *field }));
            Operand::Value(v)
        }
        hir::ExprKind::Tuple { tuple_id, elems } => {
            let ops: Vec<Operand> = elems.iter().map(|el| lower_expr(b, el)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeTuple { tuple_id: *tuple_id, elems: ops }));
            Operand::Value(v)
        }
        hir::ExprKind::TupleIndex { recv, index } => {
            let t = lower_expr(b, recv);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::TupleIndex { tuple: t, index: *index }));
            Operand::Value(v)
        }
        hir::ExprKind::IndexField { base, index, field } => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::IndexField(*base, index_const(*index as usize), *field)));
            Operand::Value(v)
        }
        hir::ExprKind::Block(blk) => {
            lower_block(b, blk).unwrap_or(Operand::Const(Const::Bool(false)))
        }
        // `unsafe {}` is a plain marker block at MIR level — no handle, no region. It lowers to its
        // inner block; the enforcement + impurity were handled in sema.
        hir::ExprKind::Unsafe(blk) => lower_block(b, blk).unwrap_or(Operand::Const(Const::Unit)),
        // `raw.alloc(size)` → a flat heap allocation yielding a `raw` byte pointer.
        hir::ExprKind::RawAlloc(size) => {
            let sz = lower_expr(b, size);
            let v = b.fresh_value(Ty::Raw);
            b.push(Stmt::Let(v, Rvalue::RawAlloc(sz)));
            Operand::Value(v)
        }
        // `raw.free(p)` → free the pointer (a side-effecting statement, like `ArenaEnd`); yields unit.
        hir::ExprKind::RawFree(ptr) => {
            let p = lower_expr(b, ptr);
            b.push(Stmt::RawFree(p));
            Operand::Const(Const::Unit)
        }
        // `raw.load(p, offset)` → read `scalar` at `p + offset` bytes.
        hir::ExprKind::RawLoad { ptr, offset, scalar } => {
            let p = lower_expr(b, ptr);
            let off = lower_expr(b, offset);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::RawLoad { ptr: p, offset: off, scalar: *scalar }));
            Operand::Value(v)
        }
        // `raw.store(p, offset, v)` → write `v` at `p + offset` bytes (a side-effecting statement).
        hir::ExprKind::RawStore { ptr, offset, value } => {
            let p = lower_expr(b, ptr);
            let off = lower_expr(b, offset);
            let val = lower_expr(b, value);
            b.push(Stmt::RawStore { ptr: p, offset: off, value: val });
            Operand::Const(Const::Unit)
        }
        // `raw.offset(p, n)` → a new `raw` pointer `p + n` bytes.
        hir::ExprKind::RawOffset { ptr, offset } => {
            let p = lower_expr(b, ptr);
            let off = lower_expr(b, offset);
            let v = b.fresh_value(Ty::Raw);
            b.push(Stmt::Let(v, Rvalue::RawOffset { ptr: p, offset: off }));
            Operand::Value(v)
        }
        // ④b: `task_group` opens a region owning each task's env + result slot, plus a deferred
        // task list. `spawn`/`wait` use the handle; the region is freed at scope end.
        hir::ExprKind::TaskGroup(blk) => {
            let handle = b.fresh_value(Ty::ArenaHandle);
            b.push(Stmt::Let(handle, Rvalue::TgBegin));
            b.task_groups.push(handle);
            let tail = lower_block(b, blk);
            b.task_groups.pop();
            if b.is_terminated() {
                Operand::Const(Const::Unit)
            } else {
                b.push(Stmt::TgEnd(Operand::Value(handle)));
                tail.unwrap_or(Operand::Const(Const::Unit))
            }
        }
        // ④b-1b (deferred): `spawn(closure)` snapshots the closure's captures into a fresh env in
        // the task-group region and registers the task; it runs at `wait`. The `Task<R>` handle is
        // the task's result slot. The closure's captures give the env layout.
        hir::ExprKind::Spawn { closure, fallible } => {
            let Ty::Task(s) = e.ty else { unreachable!("spawn result is a Task") };
            let r_ty = align_sema::scalar_to_ty(s);
            let capture_tys: Vec<Ty> = match &closure.kind {
                hir::ExprKind::Closure { captures, .. } => captures.iter().map(|c| c.ty).collect(),
                _ => Vec::new(),
            };
            let clos = lower_expr(b, closure);
            let tg = Operand::Value(*b.task_groups.last().expect("spawn outside a task_group"));
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::SpawnTask { tg, closure: clos, capture_tys, r: r_ty, fallible: *fallible }));
            Operand::Value(v)
        }
        // `t.get()` reads the result out of the task's slot.
        hir::ExprKind::TaskGet(inner) => {
            let bx = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BoxGet(bx)));
            Operand::Value(v)
        }
        // `wait()` — run all deferred tasks of the enclosing task_group.
        hir::ExprKind::Wait => {
            let tg = Operand::Value(*b.task_groups.last().expect("wait outside a task_group"));
            // A fallible group's `wait()` yields `Result<(), Error>` (built from the runtime's
            // error code); an infallible group's yields `()`.
            let fallible = matches!(e.ty, Ty::Result(..));
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::TgWaitResult { tg, fallible }));
            Operand::Value(v)
        }
        hir::ExprKind::OptionSome(inner) => {
            let op = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::OptionSome(op)));
            Operand::Value(v)
        }
        hir::ExprKind::OptionNone => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::OptionNone));
            Operand::Value(v)
        }
        hir::ExprKind::ElseUnwrap { opt, fallback } => lower_else_unwrap(b, opt, fallback, e.ty),
        hir::ExprKind::ResultOk(inner) => {
            let op = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ResultOk(op)));
            Operand::Value(v)
        }
        hir::ExprKind::ResultErr(inner) => {
            let op = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ResultErr(op)));
            Operand::Value(v)
        }
        hir::ExprKind::Try(inner) => lower_try(b, inner, e.ty),
        hir::ExprKind::Arena(blk) => {
            let handle = b.fresh_value(Ty::ArenaHandle);
            b.push(Stmt::Let(handle, Rvalue::ArenaBegin));
            b.arenas.push(handle);
            let tail = lower_block(b, blk);
            b.arenas.pop();
            if b.is_terminated() {
                // The body diverged (return/?): cleanup already ran on that path.
                Operand::Const(Const::Unit)
            } else {
                b.push(Stmt::ArenaEnd(Operand::Value(handle)));
                tail.unwrap_or(Operand::Const(Const::Unit))
            }
        }
        hir::ExprKind::HeapNew(inner) => {
            let init = lower_expr(b, inner);
            let handle = *b.arenas.last().expect("heap.new outside an arena (sema-checked)");
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::HeapAlloc(Operand::Value(handle), init)));
            Operand::Value(v)
        }
        hir::ExprKind::BoxGet(inner) => {
            let bx = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BoxGet(bx)));
            Operand::Value(v)
        }
        hir::ExprKind::BoxClone(inner) => {
            let src = lower_expr(b, inner);
            let handle = *b.arenas.last().expect("clone outside an arena (sema-checked)");
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BoxClone(Operand::Value(handle), src)));
            Operand::Value(v)
        }
        hir::ExprKind::StrClone(inner) => {
            // Deep-copy the `str` bytes into a fresh heap buffer, yielding an owned `string`
            // `{ptr,len}`. The slot it lands in is `Drop`-freed at scope exit (sema marks the
            // String local for drop), so no arena is needed.
            let src = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::StrClone(src)));
            Operand::Value(v)
        }
        hir::ExprKind::StrPredicate { kind, haystack, needle } => {
            let h = lower_expr(b, haystack);
            let n = lower_expr(b, needle);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::StrPredicate { kind: *kind, haystack: h, needle: n }));
            Operand::Value(v)
        }
        hir::ExprKind::StrTrim { kind, recv } => {
            let r = lower_expr(b, recv);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::StrTrim { kind: *kind, recv: r }));
            Operand::Value(v)
        }
        // Borrowing an owned `string` as a `str` (slice 7b) is a no-op at runtime: the two share
        // the `{ptr,len}` layout, so the loaded value is the view. The `string` is not moved (no
        // `null_moved_source`), so its owner still `Drop`-frees it.
        hir::ExprKind::StrBorrow(inner) => lower_expr(b, inner),
        hir::ExprKind::BuilderNew { capacity } => {
            let cap = match capacity {
                Some(c) => lower_expr(b, c),
                None => Operand::Const(Const::Int(0, i64_ty())),
            };
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BuilderNew { capacity: cap }));
            Operand::Value(v)
        }
        hir::ExprKind::BuilderWrite { builder, arg, kind } => {
            let bop = lower_expr(b, builder);
            let aop = lower_expr(b, arg);
            let v = b.fresh_value(Ty::Unit);
            let rv = match kind {
                hir::BuilderWriteKind::Str => Rvalue::BuilderWriteStr(bop, aop),
                hir::BuilderWriteKind::Int => Rvalue::BuilderWriteInt(bop, aop),
                hir::BuilderWriteKind::Bool => Rvalue::BuilderWriteBool(bop, aop),
                hir::BuilderWriteKind::Char => Rvalue::BuilderWriteChar(bop, aop),
                hir::BuilderWriteKind::Float => Rvalue::BuilderWriteFloat(bop, aop),
            };
            b.push(Stmt::Let(v, rv));
            Operand::Const(Const::Unit)
        }
        hir::ExprKind::BuilderToString(inner) => {
            let bop = lower_expr(b, inner);
            // The builder is consumed: null its slot so the exit `Drop` of an unfinished builder
            // is a no-op (`builder_free(null)`), and the finished `string` owns its own buffer.
            null_moved_source(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BuilderToString(bop)));
            Operand::Value(v)
        }
        hir::ExprKind::ArraySum { source, stages } => {
            let init = zero_of(e.ty);
            lower_array_reduce(b, source, stages, e.ty, init, Reducer::Sum)
        }
        hir::ExprKind::ArrayCount { source, stages } => {
            // i64 accumulator seeded at 0; each surviving element adds 1.
            let init = Operand::Const(Const::Int(0, i64_ty()));
            lower_array_reduce(b, source, stages, i64_ty(), init, Reducer::Count)
        }
        hir::ExprKind::ArrayReduce { source, stages, func, captures, init } => {
            let init_op = lower_expr(b, init);
            lower_array_reduce(b, source, stages, e.ty, init_op, Reducer::Fold { func: func.clone(), captures: captures.clone() })
        }
        hir::ExprKind::ArrayAnyAll { source, stages, func, captures, all } => {
            // bool accumulator: `all` seeds true (&&-fold), `any` seeds false (||-fold).
            let init = Operand::Const(Const::Bool(*all));
            lower_array_reduce(b, source, stages, Ty::Bool, init, Reducer::AnyAll { func: func.clone(), captures: captures.clone(), all: *all })
        }
        hir::ExprKind::ArrayMinMax { source, stages, is_max } => {
            // Seed with the element type's extreme so the running `min`/`max` is replaced by the
            // first element and an empty pipeline yields that extreme (the fold identity).
            let init = extreme_of(e.ty, *is_max);
            lower_array_reduce(b, source, stages, e.ty, init, Reducer::MinMax { is_max: *is_max })
        }
        hir::ExprKind::ArrayToArray { source, stages, elem } => {
            lower_array_collect(b, source, stages, *elem, CollectKind::Collect)
        }
        hir::ExprKind::ArrayToSoa { source, struct_id } => lower_array_to_soa(b, source, *struct_id),
        hir::ExprKind::ArrayMapInto { source, stages, dst, elem } => lower_array_map_into(b, source, stages, dst, *elem),
        hir::ExprKind::ArrayScan { source, stages, func, captures, init, elem } => {
            let init_op = lower_expr(b, init);
            lower_array_collect(b, source, stages, *elem, CollectKind::Scan { func: func.clone(), init: init_op, captures: captures.clone() })
        }
        hir::ExprKind::ArrayDot { a, b: bex, elem } => lower_array_dot(b, a, bex, *elem),
        hir::ExprKind::ArraySort { source, stages, elem } => lower_array_sort(b, source, stages, *elem, None),
        hir::ExprKind::ArraySortBy { source, stages, key_func, captures, key_ty, elem } => {
            lower_array_sort(b, source, stages, *elem, Some(SortKey { func: key_func.clone(), captures: captures.clone(), key_ty: *key_ty }))
        }
        hir::ExprKind::ArrayPartition { source, stages, func, captures, elem } => {
            let tuple_id = match e.ty {
                Ty::Tuple(id) => id,
                _ => unreachable!("partition result is a tuple"),
            };
            lower_array_partition(b, source, stages, *elem, func, captures, tuple_id)
        }
        hir::ExprKind::ArrayGroupAgg { base, struct_id, key_field, value_field, op, source } => {
            let tuple_id = match e.ty {
                Ty::Tuple(id) => id,
                _ => unreachable!("group_by aggregate result is a tuple"),
            };
            match source {
                hir::GroupSource::SoaI64 => lower_array_group_agg(b, *base, *struct_id, *key_field, *value_field, *op, tuple_id),
                hir::GroupSource::SoaStr => lower_array_group_str_cols(b, *base, *struct_id, *key_field, *value_field, *op, tuple_id),
                hir::GroupSource::AosStr => lower_array_group_str(b, *base, *struct_id, *key_field, *value_field, *op, tuple_id),
                hir::GroupSource::Encoded => lower_array_group_encoded(b, *base, *struct_id, *value_field, *op, tuple_id),
            }
        }
        hir::ExprKind::ArrayGroupAggMulti { base, struct_id, key_field, aggs, source } => {
            let tuple_id = match e.ty {
                Ty::Tuple(id) => id,
                _ => unreachable!("group_by multi-aggregate result is a tuple"),
            };
            match source {
                hir::GroupSource::AosStr => lower_array_group_multi_str(b, *base, *struct_id, *key_field, aggs, tuple_id),
                // sema restricts the fused multi-aggregate to the AoS str key (first cut).
                other => unreachable!("multi-aggregate group_by source {other:?} is sema-rejected"),
            }
        }
        hir::ExprKind::ArrayDictEncode { base, struct_id, key_field } => lower_dict_encode(b, *base, *struct_id, *key_field),
        hir::ExprKind::ArrayParMap { source, stages, func, captures, elem } => {
            // With no prior stages, a `{ptr,len}` (or fixed scalar-array) source, and no captures,
            // run in parallel via the runtime; otherwise (prior stages, struct-array source, or a
            // capturing lambda — the parallel thunk takes no capture context) fall back to the
            // sequential collect loop.
            let elem_in = match source.ty {
                Ty::Slice(s) | Ty::DynArray(s) | Ty::Array(s, _) => Some(align_sema::scalar_to_ty(s)),
                Ty::DynSliceArray(p) => Some(Ty::Slice(align_sema::prim_to_scalar(p))),
                _ => None,
            };
            if stages.is_empty() && captures.is_empty()
                && let Some(elem_in) = elem_in {
                    let src = match source.ty {
                        Ty::Slice(_) | Ty::DynArray(_) | Ty::DynSliceArray(_) => lower_expr(b, source),
                        _ => {
                            let (slot, n) = array_source_slot(b, source);
                            let sv = b.fresh_value(Ty::Slice(scalar_of(elem_in)));
                            b.push(Stmt::Let(sv, Rvalue::MakeSlice(slot, n)));
                            Operand::Value(sv)
                        }
                    };
                    // Free the source buffer if it is an owned temporary the runtime just consumed
                    // (same rule as `setup_source`: `chunks`/call results are always heap; the
                    // materializing terminals arena-allocate inside an arena and are bulk-freed).
                    let free_src = matches!(source.kind, hir::ExprKind::ArrayChunks { .. } | hir::ExprKind::Call { .. })
                        || (matches!(
                            source.kind,
                            hir::ExprKind::ArrayToArray { .. } | hir::ExprKind::ArrayScan { .. }
                                | hir::ExprKind::ArrayParMap { .. } | hir::ExprKind::ArraySort { .. } | hir::ExprKind::ArraySortBy { .. }
                        ) && b.arenas.is_empty());
                    let v = b.fresh_value(e.ty);
                    b.push(Stmt::Let(v, Rvalue::ParMapParallel { src: src.clone(), func: func.clone(), elem_in, elem_out: *elem }));
                    if free_src {
                        b.push(Stmt::DropValue(src));
                    }
                    return Operand::Value(v);
                }
            // Sequential fallback: append a `map(f)` stage (carrying any captures) and materialize
            // via the collect loop.
            let mut stages2 = stages.clone();
            stages2.push(hir::Stage { kind: hir::StageKind::Map { func: func.clone(), captures: captures.clone() }, out_ty: *elem });
            lower_array_collect(b, source, &stages2, *elem, CollectKind::Collect)
        }
        hir::ExprKind::ArrayChunks { source, n, elem } => {
            // Materialize the source as a `{ptr,len}` slice, then call the runtime chunker.
            let src = match source.ty {
                Ty::Slice(_) | Ty::DynArray(_) => lower_expr(b, source),
                _ => {
                    let (slot, len) = array_source_slot(b, source);
                    let sv = b.fresh_value(Ty::Slice(scalar_of(*elem)));
                    b.push(Stmt::Let(sv, Rvalue::MakeSlice(slot, len)));
                    Operand::Value(sv)
                }
            };
            let n_op = lower_expr(b, n);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Chunks { src, n: n_op, elem: *elem }));
            Operand::Value(v)
        }
        hir::ExprKind::ArrayToSlice(inner) => {
            let (slot, n) = array_source_slot(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeSlice(slot, n)));
            Operand::Value(v)
        }
        hir::ExprKind::Len(inner) => {
            // `str`/`slice` carry the length in their `{ ptr, len }` view.
            let sv = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::SliceLen(sv)));
            Operand::Value(v)
        }
        hir::ExprKind::Index { recv, index } => lower_index(b, recv, index, e.ty),
        hir::ExprKind::SliceRange { recv, start, end } => lower_slice_range(b, recv, start.as_deref(), end.as_deref(), e.ty),
        hir::ExprKind::ElemField { recv, index, path, struct_id } => {
            lower_index_field(b, recv, index, path, *struct_id, e.ty)
        }
        hir::ExprKind::ArrayLit { .. } => {
            unreachable!("array literal only appears as a let initializer or pipeline source")
        }
        // `select(mask, a, b)` → a vector `select` (`Rvalue::Select` with a vector mask cond).
        hir::ExprKind::Select { mask, a, b: bexpr } => {
            let cond = lower_expr(b, mask);
            let av = lower_expr(b, a);
            let bv = lower_expr(b, bexpr);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Select { cond, a: av, b: bv }));
            Operand::Value(v)
        }
        // `vec.sum_where(mask)` → masked horizontal sum. `e.ty` is the element scalar; the width is
        // recovered from the receiver's vector type.
        hir::ExprKind::VecSumWhere { vec, mask } => {
            let n = match vec.ty {
                Ty::Vec(_, n) => n,
                _ => unreachable!("sema types sum_where's receiver as a vector"),
            };
            let vv = lower_expr(b, vec);
            let mv = lower_expr(b, mask);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::VecSumWhere { vec: vv, mask: mv, elem: e.ty, n }));
            Operand::Value(v)
        }
        // `dot(a, b)` → vector multiply then a lane reduction. `e.ty` is the element; the width comes
        // from the operand vector type.
        hir::ExprKind::VecDot { a, b: bexpr } => {
            let n = match a.ty {
                Ty::Vec(_, n) => n,
                _ => unreachable!("sema types dot's operands as vectors"),
            };
            let av = lower_expr(b, a);
            let bv = lower_expr(b, bexpr);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::VecDot { a: av, b: bv, elem: e.ty, n }));
            Operand::Value(v)
        }
        // `v.min()` / `v.max()` → fold the lanes with the scalar min/max intrinsic.
        hir::ExprKind::VecMinMax { vec, max } => {
            let n = match vec.ty {
                Ty::Vec(_, n) => n,
                _ => unreachable!("sema types min/max's receiver as a vector"),
            };
            let vv = lower_expr(b, vec);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::VecMinMax { vec: vv, elem: e.ty, n, max: *max }));
            Operand::Value(v)
        }
        // `v.sum()` → add all lanes (the shared horizontal sum).
        hir::ExprKind::VecSum { vec } => {
            let n = match vec.ty {
                Ty::Vec(_, n) => n,
                _ => unreachable!("sema types sum's receiver as a vector"),
            };
            let vv = lower_expr(b, vec);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::VecSum { vec: vv, elem: e.ty, n }));
            Operand::Value(v)
        }
        // `s.load(i)` → bounds-checked `<n x T>` load from the slice buffer at `i..i+n`. If the slice
        // is a whole borrow of an `align(N)` binding and the address is provably N-aligned, tag the
        // load with that alignment (the aligned-vector-load fast path); else fall back to element
        // alignment. Computed from the *HIR* receiver before it is lowered to an opaque slice temp.
        hir::ExprKind::VecLoad { src, index, elem, n } => {
            let align = proven_vec_load_align(b, src, index, align_sema::scalar_to_ty(*elem));
            let sv = lower_expr(b, src);
            let idx = lower_expr(b, index);
            emit_vec_bounds_check(b, &sv, &idx, *n);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::VecLoad { slice: sv, index: idx, elem: align_sema::scalar_to_ty(*elem), n: *n, align }));
            Operand::Value(v)
        }
        // `s.store(i, v)` → bounds-checked `<n x T>` store into the slice buffer at `i..i+n`. Unit.
        hir::ExprKind::VecStore { dst, index, value, elem, n } => {
            let sv = lower_expr(b, dst);
            let idx = lower_expr(b, index);
            let val = lower_expr(b, value);
            emit_vec_bounds_check(b, &sv, &idx, *n);
            b.push(Stmt::VecStore { slice: sv, index: idx, value: val, elem: align_sema::scalar_to_ty(*elem), n: *n });
            Operand::Const(Const::Unit)
        }
        // A `vecN<T>` literal is a register value: build it via an insertelement chain (`MakeVec`).
        hir::ExprKind::VecLit { elems, elem } => {
            let ops: Vec<Operand> = elems.iter().map(|el| lower_expr(b, el)).collect();
            let n = ops.len() as u32;
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeVec { elems: ops, elem: align_sema::scalar_to_ty(*elem), n }));
            Operand::Value(v)
        }
        // A struct literal in value position (return/arg/assign): materialize it into a
        // temp slot field by field, then load the whole struct. (A `let` initializer stores
        // straight into its own slot — see `lower_stmt` — avoiding this copy.)
        hir::ExprKind::StructLit { .. } => {
            let slot = b.new_slot(e.ty);
            store_value_at(b, slot, &mut Vec::new(), e);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
    }
}

/// The i64 type used for array indices / loop counters.
fn i64_ty() -> Ty {
    Ty::Int(IntTy { bits: 64, signed: true })
}

/// The i32 status code a runtime builtin (`fs`/`json`/`io`) returns, before it is wrapped into
/// `Error.Code`.
fn status_ty() -> Ty {
    Ty::Int(IntTy { bits: 32, signed: true })
}

/// `x / -1` / `x % -1` (a *known* `-1` divisor): fold to the defined two's-complement result
/// directly — `x / -1 == 0 - x` (a wrapping negate, correct for every `x` including `INT_MIN`, which
/// wraps back to `INT_MIN`) and `x % -1 == 0`. No guard, no `select`.
fn fold_div_neg_one(b: &mut Builder, op: BinOp, l: Operand, ty: Ty) -> Operand {
    match op {
        BinOp::Div => {
            let w = b.fresh_value(ty);
            b.push(Stmt::Let(w, Rvalue::Bin(BinOp::Sub, Operand::Const(Const::Int(0, ty)), l)));
            Operand::Value(w)
        }
        _ => Operand::Const(Const::Int(0, ty)),
    }
}

/// Lower an integer `/` or `%` with its divisor guards (semantics live in MIR):
/// - `divisor == 0` aborts via `div_fail` (`-> !`, a cold edge), the settled "division by zero is
///   never silent" rule — a raw `sdiv`/`udiv` by zero is LLVM UB.
/// - signed `INT_MIN / -1` (and `% -1`) would also be LLVM UB; instead it wraps to the defined
///   two's-complement result (`x / -1 == -x`, `x % -1 == 0`), consistent with defined
///   two's-complement overflow. The raw div/rem is fed a `-1 → 1` remapped divisor so it never
///   hits the UB case, and a `select` restores the wrapped value on the `-1` path. Unsigned has
///   neither overflow case, so it only carries the zero guard.
///
/// A *constant* non-zero divisor (`x / 2`, `x % 10`, `x / -1`, …) is the common case: it needs no
/// runtime guard at all (the two UB cases are decidable at compile time), so it is lowered straight
/// to the raw op — or, for `-1`, folded — keeping the MIR (and unoptimized IR) lean.
fn lower_int_div(b: &mut Builder, op: BinOp, l: Operand, r: Operand, ty: Ty) -> Operand {
    let signed = matches!(ty, Ty::Int(IntTy { signed: true, .. }));
    // Constant-divisor fast path. `wrap_to_int` (sema) sign-extends signed constants, so a signed
    // `-1` divisor is stored as `-1` here (not a width-masked `0xFF…`); unsigned constants are
    // always `>= 0` (a negative literal under an unsigned type is a compile error).
    if let Operand::Const(Const::Int(val, _)) = &r {
        let val = *val;
        if val != 0 {
            if signed && val == -1 {
                return fold_div_neg_one(b, op, l, ty);
            }
            // Any other non-zero constant divisor: no UB (divisor is neither 0 nor -1), so emit the
            // raw div/rem with no guard.
            let v = b.fresh_value(ty);
            b.push(Stmt::Let(v, Rvalue::Bin(op, l, r)));
            return Operand::Value(v);
        }
        // A constant `0` divisor falls through to the runtime guard, which aborts. (A literal `/0`
        // is usually caught earlier by sema's constant folding; this keeps the abort semantics for
        // any constant `0` that does reach here.)
    }
    // divisor == 0 → report and abort (cold edge), the same shape as `emit_bounds_check`.
    let is_zero = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_zero, Rvalue::Bin(BinOp::Eq, r.clone(), Operand::Const(Const::Int(0, ty)))));
    let fail = b.new_block();
    let ok = b.new_block();
    b.terminate(Term::Branch(Operand::Value(is_zero), fail, ok));
    b.cur = fail;
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::Call("div_fail".to_string(), vec![])));
    b.terminate(Term::Unreachable);
    b.cur = ok;

    if !signed {
        // Unsigned: no `INT_MIN / -1` case; the divisor is now known non-zero.
        let v = b.fresh_value(ty);
        b.push(Stmt::Let(v, Rvalue::Bin(op, l, r)));
        return Operand::Value(v);
    }
    // Signed: fold away the `INT_MIN / -1` UB. `is_neg1` selects the wrapped result.
    let is_neg1 = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_neg1, Rvalue::Bin(BinOp::Eq, r.clone(), Operand::Const(Const::Int(-1, ty)))));
    // Divide by `1` instead of `-1` so the raw sdiv/srem never triggers UB; the select below
    // replaces its result on the `-1` path regardless.
    let safe = b.fresh_value(ty);
    b.push(Stmt::Let(
        safe,
        Rvalue::Select { cond: Operand::Value(is_neg1), a: Operand::Const(Const::Int(1, ty)), b: r },
    ));
    let raw = b.fresh_value(ty);
    b.push(Stmt::Let(raw, Rvalue::Bin(op, l.clone(), Operand::Value(safe))));
    // Wrapped result on the `-1` path: `x / -1 == 0 - x` (wraps at INT_MIN); `x % -1 == 0`.
    let wrapped = fold_div_neg_one(b, op, l, ty);
    let v = b.fresh_value(ty);
    b.push(Stmt::Let(v, Rvalue::Select { cond: Operand::Value(is_neg1), a: wrapped, b: Operand::Value(raw) }));
    Operand::Value(v)
}

/// Lower a `vecN<T>` `/` or `%` with the same lane-wise divisor guards as the scalar [`lower_int_div`]
/// — the SIMD mirror of that pass, so vector and scalar division share one semantics:
/// - **Any zero lane aborts.** `divisor == splat(0)` is an elementwise compare → a `<N x i1>` mask;
///   [`Rvalue::MaskAny`] reduces it to a scalar `bool`, and any set lane branches to `div_fail`
///   (`-> !`, a cold edge). A raw `sdiv`/`udiv` with a zero lane is LLVM UB.
/// - **A signed `INT_MIN / -1` lane wraps** to the defined two's-complement result (`x / -1 == -x`,
///   `x % -1 == 0`), lane-wise. As in the scalar path, each `-1` divisor lane is remapped to `1` so
///   the raw vector div/rem never hits UB, and a `select` restores the wrapped value on those lanes.
///   Unsigned/float vectors have neither overflow case (float is IEEE, no guard at all).
///
/// The divisor `r` may be a broadcast scalar (`v % k`); it is splatted to a `<N x T>` vector so the
/// guard's compares/selects are uniformly vector-typed. A **broadcast constant** divisor (`v / 16`,
/// `v % width` — the common SIMD-kernel case) takes the same guard-free fast path as the scalar
/// `lower_int_div` (a known non-zero, non-`-1` divisor has no UB). A constant *vector* divisor
/// (`v / [a,b,c,d]`) isn't inspectable as an `Operand` here, so it keeps the guard — which folds away
/// under the optimizer, and still (correctly) aborts on a constant zero lane.
fn lower_vec_div(b: &mut Builder, op: BinOp, l: Operand, r: Operand, s: align_sema::Scalar, n: u32, rhs_ty: Ty) -> Operand {
    let elem = align_sema::scalar_to_ty(s);
    let vec_ty = Ty::Vec(s, n);
    let signed = matches!(elem, Ty::Int(IntTy { signed: true, .. }));
    // Splat a constant scalar into a `<N x T>` vector value.
    let splat = |b: &mut Builder, val: i128| -> Operand {
        let v = b.fresh_value(vec_ty);
        b.push(Stmt::Let(v, Rvalue::MakeVec { elems: vec![Operand::Const(Const::Int(val, elem)); n as usize], elem, n }));
        Operand::Value(v)
    };
    // The wrapped result where the divisor is `-1`: `x / -1 == 0 - x` (wraps at INT_MIN), `x % -1 == 0`.
    let neg1_wrapped = |b: &mut Builder, l: Operand| -> Operand {
        if op == BinOp::Div {
            let z = splat(b, 0);
            let w = b.fresh_value(vec_ty);
            b.push(Stmt::Let(w, Rvalue::Bin(BinOp::Sub, z, l)));
            Operand::Value(w)
        } else {
            splat(b, 0)
        }
    };
    // Float vectors: IEEE lane-wise `frem`/`fdiv`, no guard (matches scalar float `%`/`/`).
    if matches!(elem, Ty::Float(_)) {
        let v = b.fresh_value(vec_ty);
        b.push(Stmt::Let(v, Rvalue::Bin(op, l, r)));
        return Operand::Value(v);
    }
    // Broadcast constant-divisor fast path (mirrors the scalar `lower_int_div` constant path). Only a
    // broadcast (scalar) divisor is a single `Operand::Const`; a constant vector is a `MakeVec` value.
    // A constant `0` divisor is left out (`None`) so it falls through to the runtime guard, which aborts.
    let broadcast_nonzero = match &r {
        Operand::Const(Const::Int(val, _)) if !matches!(rhs_ty, Ty::Vec(..)) && *val != 0 => Some(*val),
        _ => None,
    };
    if let Some(val) = broadcast_nonzero {
        if signed && val == -1 {
            return neg1_wrapped(b, l);
        }
        // Any other non-zero broadcast constant: no UB, emit the raw vector op unguarded.
        let v = b.fresh_value(vec_ty);
        b.push(Stmt::Let(v, Rvalue::Bin(op, l, r)));
        return Operand::Value(v);
    }
    // Ensure the divisor is a vector (splat a broadcast scalar) so the guard is uniformly vectorized.
    let rvec = if matches!(rhs_ty, Ty::Vec(..)) {
        r
    } else {
        let v = b.fresh_value(vec_ty);
        b.push(Stmt::Let(v, Rvalue::MakeVec { elems: vec![r; n as usize], elem, n }));
        Operand::Value(v)
    };
    let mask_ty = Ty::Mask(s, n);
    // any(divisor == 0) → report and abort (cold edge), the vector mirror of the scalar zero guard.
    // Compare vector-vs-vector (splat the `0`) so the MIR node is well-typed — a vector `Eq` takes two
    // `<N x T>` operands, not a bare scalar constant (codegen would splat it, but keep the IR honest).
    let zero_vec = splat(b, 0);
    let is_zero = b.fresh_value(mask_ty);
    b.push(Stmt::Let(is_zero, Rvalue::Bin(BinOp::Eq, rvec.clone(), zero_vec)));
    let any_zero = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(any_zero, Rvalue::MaskAny { mask: Operand::Value(is_zero), n }));
    let fail = b.new_block();
    let ok = b.new_block();
    b.terminate(Term::Branch(Operand::Value(any_zero), fail, ok));
    b.cur = fail;
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::Call("div_fail".to_string(), vec![])));
    b.terminate(Term::Unreachable);
    b.cur = ok;

    if !signed {
        // Unsigned: no `INT_MIN / -1` case; divisor lanes are now known non-zero.
        let v = b.fresh_value(vec_ty);
        b.push(Stmt::Let(v, Rvalue::Bin(op, l, rvec)));
        return Operand::Value(v);
    }
    // Signed: fold away the per-lane `INT_MIN / -1` UB (same shape as the scalar path, lane-wise).
    // Again compare vector-vs-vector (splat the `-1`) to keep the MIR node well-typed.
    let neg1_vec = splat(b, -1);
    let is_neg1 = b.fresh_value(mask_ty);
    b.push(Stmt::Let(is_neg1, Rvalue::Bin(BinOp::Eq, rvec.clone(), neg1_vec)));
    // Remap each `-1` lane to `1` so the raw vector div/rem never triggers UB; the select below
    // replaces the result on those lanes regardless.
    let one_vec = splat(b, 1);
    let safe = b.fresh_value(vec_ty);
    b.push(Stmt::Let(safe, Rvalue::Select { cond: Operand::Value(is_neg1), a: one_vec, b: rvec }));
    let raw = b.fresh_value(vec_ty);
    b.push(Stmt::Let(raw, Rvalue::Bin(op, l.clone(), Operand::Value(safe))));
    let wrapped = neg1_wrapped(b, l);
    let v = b.fresh_value(vec_ty);
    b.push(Stmt::Let(v, Rvalue::Select { cond: Operand::Value(is_neg1), a: wrapped, b: Operand::Value(raw) }));
    Operand::Value(v)
}

/// Emit the explicit bounds check for `recv[index]` (semantics live in MIR):
/// `if index < 0 || index >= len { bounds_fail(index, len); unreachable }`. Leaves `b.cur` at the
/// in-bounds block so the caller emits the element load. Out-of-bounds is a hard error (the
/// settled panic model — never a silent OOB read).
fn emit_bounds_check(b: &mut Builder, idx: &Operand, len: Operand) {
    let lo = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(lo, Rvalue::Bin(BinOp::Lt, idx.clone(), Operand::Const(Const::Int(0, i64_ty())))));
    let hi = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(hi, Rvalue::Bin(BinOp::Ge, idx.clone(), len.clone())));
    let oob = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(oob, Rvalue::Bin(BinOp::Or, Operand::Value(lo), Operand::Value(hi))));

    let fail = b.new_block();
    let ok = b.new_block();
    b.terminate(Term::Branch(Operand::Value(oob), fail, ok));

    // fail: report (index, len) and abort. `bounds_fail` is `-> !`, so the block is `Unreachable`.
    b.cur = fail;
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::Call("bounds_fail".to_string(), vec![idx.clone(), len])));
    b.terminate(Term::Unreachable);

    b.cur = ok;
}

/// The byte width (1/2/4/8) of a binary scalar read/written by [`Rvalue::BytesRead`] /
/// [`Rvalue::BufferPut`]. Sema restricts these to fixed-width int/float scalars, so any other type
/// is unreachable.
fn binary_scalar_width(scalar: Ty) -> i128 {
    match scalar {
        Ty::Int(IntTy { bits, .. }) | Ty::Float(FloatTy { bits }) => (bits / 8) as i128,
        _ => unreachable!("sema restricts a binary read/write scalar to a fixed-width int/float"),
    }
}

/// `bytes.<scalar>_<le|be>(off)` → a bounds-checked binary scalar read from a `slice<u8>` view.
/// The range check `0 <= off <= off+width <= len` aborts on violation (like `slice[i]`); the load
/// itself is then unguarded. `off + width` uses wrapping add, but the `start > end` arm of the
/// range check catches an overflowing `off`, so no out-of-range address is ever formed.
fn lower_bytes_read(b: &mut Builder, bytes: &hir::Expr, offset: &hir::Expr, be: bool, scalar: Ty) -> Operand {
    let width = binary_scalar_width(scalar);
    let sv = lower_expr(b, bytes);
    let off = lower_expr(b, offset);
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
    let end = b.fresh_value(i64_ty());
    b.push(Stmt::Let(end, Rvalue::Bin(BinOp::Add, off.clone(), Operand::Const(Const::Int(width, i64_ty())))));
    emit_range_bounds_check(b, &off, &Operand::Value(end), Operand::Value(len));
    let v = b.fresh_value(scalar);
    b.push(Stmt::Let(v, Rvalue::BytesRead { bytes: sv, offset: off, scalar, be }));
    Operand::Value(v)
}

/// `buf.put_<scalar>_<le|be>(v)` → append `v`'s bytes to the growable buffer. A unit-valued
/// side-effecting rvalue (the runtime grows the buffer); returns `()`.
fn lower_buffer_put(b: &mut Builder, buffer: &hir::Expr, value: &hir::Expr, be: bool) -> Operand {
    let scalar = value.ty;
    let bufop = lower_expr(b, buffer);
    let val = lower_expr(b, value);
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::BufferPut { buffer: bufop, value: val, scalar, be }));
    Operand::Const(Const::Unit)
}

/// `buf.append(data)` → copy a raw `slice<u8>` blob onto the growable buffer. Unit-valued.
fn lower_buffer_append(b: &mut Builder, buffer: &hir::Expr, data: &hir::Expr) -> Operand {
    let bufop = lower_expr(b, buffer);
    let dop = lower_expr(b, data);
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::BufferAppend { buffer: bufop, data: dop }));
    Operand::Const(Const::Unit)
}

/// `recv[index]` → a bounds-checked scalar element load. A scalar `array<T>` / `slice` loads
/// through its `{ptr,len}` value (`SliceIndex`); a fixed stack `array` loads through its slot
/// (`Index`).
fn lower_index(b: &mut Builder, recv: &hir::Expr, index: &hir::Expr, elem_ty: Ty) -> Operand {
    // `v[lane]` on a vector → `extractelement` (no bounds check: sema validated a constant lane).
    if let Ty::Vec(_, _) = recv.ty {
        let vv = lower_expr(b, recv);
        let lane = match &index.kind {
            hir::ExprKind::Int(v) => *v as u32,
            _ => unreachable!("sema requires a constant vector lane index"),
        };
        let v = b.fresh_value(elem_ty);
        b.push(Stmt::Let(v, Rvalue::VecExtract { vec: vv, lane, elem: elem_ty }));
        return Operand::Value(v);
    }
    // `s[i]` on a `soa<Struct>` → gather the whole struct from the columns at `i` (bounds-checked).
    if let Ty::Soa(struct_id) = recv.ty {
        let sv = lower_expr(b, recv);
        let idx = lower_expr(b, index);
        let len = b.fresh_value(i64_ty());
        b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
        emit_bounds_check(b, &idx, Operand::Value(len));
        let v = b.fresh_value(elem_ty);
        b.push(Stmt::Let(v, Rvalue::SoaGather { base: sv, index: idx, struct_id }));
        return Operand::Value(v);
    }
    let idx = lower_expr(b, index);
    // The length, and whether the element loads from a `{ptr,len}` value or a stack slot.
    enum Src {
        Slice(Operand),
        Slot(Slot),
    }
    let (src, len): (Src, Operand) = match recv.ty {
        // A `{ptr,len}` value: scalar `slice`/owned `array` loads a scalar element; an
        // `array<slice<T>>` (`chunks` result) loads a whole `slice<T>` element; an owned dynamic
        // `array<Struct>` loads a whole struct element (all by `elem_ty` via `SliceIndex`).
        Ty::Slice(_) | Ty::DynArray(_) | Ty::DynSliceArray(_) | Ty::DynStructArray(..) => {
            let sv = lower_expr(b, recv);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            (Src::Slice(sv), Operand::Value(len))
        }
        _ => {
            // A fixed `array<T>` (sema restricted `recv` to a literal / local).
            let (slot, n) = array_source_slot(b, recv);
            (Src::Slot(slot), Operand::Const(Const::Int(n, i64_ty())))
        }
    };
    emit_bounds_check(b, &idx, len);
    let v = b.fresh_value(elem_ty);
    match src {
        Src::Slice(sv) => b.push(Stmt::Let(v, Rvalue::SliceIndex(sv, idx))),
        Src::Slot(slot) => b.push(Stmt::Let(v, Rvalue::Index(slot, idx))),
    }
    Operand::Value(v)
}

/// Bounds for `start..end`: `0 <= start`, `start <= end`, `end <= len`. Any violation aborts via
/// `range_fail(start, end, len)` (`-> !`), which reports the whole range — a single (index, len)
/// pair can't describe an inverted range whose bounds are each individually valid.
/// Bounds for a vector load/store of `n` lanes at `idx`: `0 <= idx` and `idx + n <= len`. Reuses
/// the range check with `start = idx`, `end = idx + n` (the slice's length is its `SliceLen`).
fn emit_vec_bounds_check(b: &mut Builder, slice: &Operand, idx: &Operand, n: u32) {
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(slice.clone())));
    let end = b.fresh_value(i64_ty());
    b.push(Stmt::Let(end, Rvalue::Bin(BinOp::Add, idx.clone(), Operand::Const(Const::Int(n as i128, i64_ty())))));
    emit_range_bounds_check(b, idx, &Operand::Value(end), Operand::Value(len));
}

fn emit_range_bounds_check(b: &mut Builder, start: &Operand, end: &Operand, len: Operand) {
    let neg = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(neg, Rvalue::Bin(BinOp::Lt, start.clone(), Operand::Const(Const::Int(0, i64_ty())))));
    let inv = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(inv, Rvalue::Bin(BinOp::Gt, start.clone(), end.clone())));
    let over = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(over, Rvalue::Bin(BinOp::Gt, end.clone(), len.clone())));
    let e1 = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(e1, Rvalue::Bin(BinOp::Or, Operand::Value(neg), Operand::Value(inv))));
    let oob = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(oob, Rvalue::Bin(BinOp::Or, Operand::Value(e1), Operand::Value(over))));

    let fail = b.new_block();
    let ok = b.new_block();
    b.terminate(Term::Branch(Operand::Value(oob), fail, ok));

    b.cur = fail;
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::Call("range_fail".to_string(), vec![start.clone(), end.clone(), len])));
    b.terminate(Term::Unreachable);

    b.cur = ok;
}

/// `recv[start..end]` → a borrowed sub-view `{ ptr + start, end - start }` with a range bounds
/// check. The base `{ptr,len}` comes from the receiver (a fixed `array<T>` borrows to a slice
/// first; `str`/`slice`/owned-array are already `{ptr,len}`). `result_ty` is the view type — `str`
/// (byte-stride pointer offset) or `slice<T>` (element-stride).
fn lower_slice_range(b: &mut Builder, recv: &hir::Expr, start: Option<&hir::Expr>, end: Option<&hir::Expr>, result_ty: Ty) -> Operand {
    // The element type driving the pointer-offset stride: a `u8` byte for a `str`, else the element.
    let elem = match result_ty {
        Ty::Str => Ty::Int(IntTy { bits: 8, signed: false }),
        Ty::Slice(s) => align_sema::scalar_to_ty(s),
        _ => unreachable!("slice range result is str or slice"),
    };
    // Base `{ptr,len}` value.
    let base = match recv.ty {
        Ty::Array(s, _) => {
            let (slot, n) = array_source_slot(b, recv);
            let v = b.fresh_value(Ty::Slice(s));
            b.push(Stmt::Let(v, Rvalue::MakeSlice(slot, n)));
            Operand::Value(v)
        }
        _ => lower_expr(b, recv),
    };
    let base_len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(base_len, Rvalue::SliceLen(base.clone())));
    let start_op = match start {
        Some(s) => lower_expr(b, s),
        None => Operand::Const(Const::Int(0, i64_ty())),
    };
    let end_op = match end {
        Some(e) => lower_expr(b, e),
        None => Operand::Value(base_len),
    };
    emit_range_bounds_check(b, &start_op, &end_op, Operand::Value(base_len));
    let new_len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(new_len, Rvalue::Bin(BinOp::Sub, end_op, start_op.clone())));
    let v = b.fresh_value(result_ty);
    b.push(Stmt::Let(v, Rvalue::SubSlice { base, start: start_op, len: Operand::Value(new_len), elem }));
    Operand::Value(v)
}

/// `recv[index].field` for a struct array (MMv2 slice 8f) → a bounds-checked element-field load.
/// A fixed stack `array<Struct>` uses the slot-based `IndexField`; an owned dynamic
/// `array<Struct>` uses the pointer-based `IndexFieldPtr` (same addressing as a fused pipeline
/// projection). Only the one field (a scalar or a `str` view) is loaded — no whole-struct copy.
fn lower_index_field(b: &mut Builder, recv: &hir::Expr, index: &hir::Expr, path: &[u32], struct_id: u32, leaf_ty: Ty) -> Operand {
    let idx = lower_expr(b, index);
    // Set the element-field address up the same way the fused pipeline does (one shared seam,
    // `lower_field_access`): a fixed `array<Struct>` is slot-addressed, an owned dynamic
    // `array<Struct>` is a `{ptr,len}` value addressed by pointer. Differs from the pipeline only
    // in needing an explicit bounds check (the loop's counter is in-bounds by construction).
    let (struct_view, slice_val, slot, len) = match recv.ty {
        Ty::DynStructArray(_, layout) => {
            let sv = lower_expr(b, recv);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            (Some((struct_id, layout)), Some(sv), 0, Operand::Value(len))
        }
        // `s[i].field` on a soa — a column-major `{ptr,len}` view; the shared seam reads the one
        // column directly as `IndexColumn`, no whole-struct gather.
        Ty::Soa(_) => {
            let sv = lower_expr(b, recv);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            (Some((struct_id, Layout::Soa)), Some(sv), 0, Operand::Value(len))
        }
        _ => {
            // A fixed `array<Struct>` slot (sema restricted `recv` to a literal / local).
            let (slot, n) = array_source_slot(b, recv);
            (None, None, slot, Operand::Const(Const::Int(n, i64_ty())))
        }
    };
    emit_bounds_check(b, &idx, len);
    // Load the element's first field via the shared seam. For a depth-1 path that *is* the leaf; for
    // a nested path (`arr[i].a.x`) it is the intermediate sub-struct, which we materialize to a temp
    // slot and then project the remaining field path out of (reusing the slot-field GEP) — so the
    // pipeline's single-field seam stays untouched.
    let first_ty = if path.len() == 1 { leaf_ty } else { b.structs[struct_id as usize].fields[path[0] as usize].ty };
    let first = lower_field_access(b, struct_view, &slice_val, slot, &idx, path[0], first_ty);
    if path.len() == 1 {
        return Operand::Value(first);
    }
    let tmp = b.new_slot(first_ty);
    b.push(Stmt::Store(tmp, Operand::Value(first)));
    let leaf = b.fresh_value(leaf_ty);
    b.push(Stmt::Let(leaf, Rvalue::Field(tmp, path[1..].to_vec())));
    Operand::Value(leaf)
}

fn index_const(i: usize) -> Operand {
    Operand::Const(Const::Int(i as i128, i64_ty()))
}

/// The statically provable byte alignment of a `.load(index)` on `src` — the aligned-vector-load
/// fast path unlocked by an `align(N) data := [...]` binding. Returns `Some(N)` only when we can
/// prove the load address lands on an `N`-byte boundary; otherwise `None` (codegen then uses the
/// element's natural alignment, always correct). Over-stating a load's alignment is UB, so every
/// step is conservative:
///   1. `src` is a *whole-array borrow* (`a[..]`, or an implicit array→slice coercion) of a **local**
///      bound with `align(N)`, so the slice's buffer pointer *is* the `N`-aligned array base.
///   2. the borrow's start offset (from an `a[start..]`) and the load `index` are both compile-time
///      constants.
///   3. `(start + index) * sizeof(elem)` is a non-negative multiple of `N`: element `start + index`
///      lands on an `N`-boundary from the aligned base.
///
/// A slice that crossed a function boundary (a `slice<T>` parameter) carries no such provenance, so
/// it always falls through to element alignment — never a wrong over-alignment.
fn proven_vec_load_align(b: &Builder, src: &hir::Expr, index: &hir::Expr, elem: Ty) -> Option<u32> {
    // Peel a whole-array borrow to (underlying array expr, element start offset).
    let (arr, start): (&hir::Expr, i128) = match &src.kind {
        hir::ExprKind::ArrayToSlice(inner) => (inner, 0),
        hir::ExprKind::SliceRange { recv, start, end: _ } => {
            let s = match start {
                None => 0,
                Some(e) => const_int_expr(e)?,
            };
            (recv, s)
        }
        _ => return None,
    };
    // Only a bare local carries a binding alignment (slot index == LocalId).
    let hir::ExprKind::Local(id) = &arr.kind else { return None };
    let n = (*b.slot_align.get(*id as usize)?)?;
    let idx = const_int_expr(index)?;
    let elem_bytes = ty_byte_size(elem)?;
    let byte_off = start.checked_add(idx)?.checked_mul(elem_bytes)?;
    // `n` is a validated power of two (so never 0), but guard the modulus anyway — a stray 0 here
    // would panic, and the repo standard is defense-in-depth on divisor/width zero (cf. `w == 0`).
    if n != 0 && byte_off >= 0 && byte_off % i128::from(n) == 0 {
        Some(n)
    } else {
        None
    }
}

/// A compile-time integer value of a HIR expression, if it is an integer literal (the only const
/// form a `.load` index / slice start takes today). Anything else → `None` (conservative).
fn const_int_expr(e: &hir::Expr) -> Option<i128> {
    match &e.kind {
        hir::ExprKind::Int(v) => Some(*v),
        _ => None,
    }
}

/// Byte size of a primitive scalar type (int/float), for a vector element-offset computation. A
/// non-primitive element is never a vector lane, so `None` (no fast path).
fn ty_byte_size(ty: Ty) -> Option<i128> {
    match ty {
        Ty::Int(it) => Some(i128::from((it.bits / 8).max(1))),
        Ty::Float(ft) => Some(i128::from(ft.bits / 8)),
        _ => None,
    }
}

/// Zero of a numeric scalar type (the additive identity for `sum`). `ty` is always `Int` or `Float`
/// — sema's `check_array_sum` rejects a non-numeric `sum` element (`is_numeric()`, `align_sema`), so
/// the `_` arm (which would produce a nonsensical `Int(0)` of a non-numeric type) is unreachable for
/// a well-typed program; it stands in for every integer width.
fn zero_of(ty: Ty) -> Operand {
    match ty {
        Ty::Float(_) => Operand::Const(Const::Float(0.0, ty)),
        _ => Operand::Const(Const::Int(0, ty)),
    }
}

/// Fold a `where` predicate into a reduction loop's running `mask` (`mask && pred`). Branchless for
/// *every* reducer: the loop stays branch-free and the reducer `select`s each masked-out lane to its
/// identity (`sum`/`count` → 0, `min` → +∞, `max` → −∞, `any` → false, `all` → true) or, for generic
/// `reduce` (no identity for a user `f`), leaves the accumulator unchanged (`acc = mask ? f(acc,v) :
/// acc`). Branch-free is what lets LLVM vectorize and maps 1:1 onto a scalable-ISA predicated tail
/// (`05 §5`). The materializing collect path (`to_array`/`scan`) keeps a real skip-branch — it must
/// not append a masked-out element — so it does not use this helper.
fn accumulate_mask(b: &mut Builder, mask: Option<Operand>, pred: Operand) -> Operand {
    match mask {
        None => pred,
        Some(m) => {
            let v = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(v, Rvalue::Bin(BinOp::And, m, pred)));
            Operand::Value(v)
        }
    }
}

/// The seed / masked-out identity for a `min` (`is_max = false`) / `max` (`is_max = true`) fold: the
/// element type's largest / smallest value, so the first (surviving) element always replaces it and a
/// masked-out lane can never win. Floats use ±infinity. `ty` is always `Int` or `Float` — sema's
/// `check_array_min_max` rejects a non-numeric `min`/`max` element (`is_numeric()`, which excludes
/// `Char`/`Bool` — `align_sema`), so both callers (the fold seed and the `where` mask-select) only
/// ever pass a numeric type. The `_` arm is therefore unreachable for a well-typed program; it must
/// not be relied on as a real identity for a non-numeric type (it would be a wrong `Int(0)`).
fn extreme_of(ty: Ty, is_max: bool) -> Operand {
    match ty {
        Ty::Float(_) => {
            let v = if is_max { f64::NEG_INFINITY } else { f64::INFINITY };
            Operand::Const(Const::Float(v, ty))
        }
        Ty::Int(IntTy { bits, signed }) => {
            // `min` seeds with the type max; `max` seeds with the type min.
            let v: i128 = if is_max {
                // type minimum
                if signed { -(1i128 << (bits - 1)) } else { 0 }
            } else {
                // type maximum
                if signed { (1i128 << (bits - 1)) - 1 } else { (1i128 << bits) - 1 }
            };
            Operand::Const(Const::Int(v, ty))
        }
        // Unreachable: sema guarantees a numeric element (see the doc comment).
        _ => Operand::Const(Const::Int(0, ty)),
    }
}

/// Resolve an array-typed source expression to a slot holding it (materializing a
/// literal), returning `(slot, length)`.
fn array_source_slot(b: &mut Builder, source: &hir::Expr) -> (Slot, i128) {
    match &source.kind {
        hir::ExprKind::ArrayLit { elems, elem } => {
            let slot = b.new_slot(source.ty);
            store_array_elems(b, slot, elems, *elem);
            (slot, elems.len() as i128)
        }
        hir::ExprKind::Local(id) => {
            let n = match source.ty {
                Ty::Array(_, n) | Ty::StructArray(_, n) => n as i128,
                _ => 0,
            };
            (*id, n)
        }
        _ => unreachable!("array source must be a literal or a local in M4"),
    }
}

/// Store an array literal's elements into `slot`: scalar arrays write each element by
/// index; struct arrays write each element's fields (the elements are struct literals).
/// Store `value` into `slot` at the field `path`, expanding a struct literal in place: a nested
/// `StructLit` field recurses with the path extended by the field index, so only leaf scalars are
/// stored (no intermediate struct value is materialized). A non-literal value is one field store.
/// The type of the field reached from struct slot `slot` by the field-index `path` (each step
/// indexes the current struct's fields). `Ty::Error` if the walk leaves a struct (shouldn't happen
/// on a well-typed path).
fn field_ty_at(b: &Builder, slot: Slot, path: &[u32]) -> Ty {
    let mut ty = b.slots[slot as usize];
    for &f in path {
        let Ty::Struct(sid) = ty else { return Ty::Error };
        match b.structs[sid as usize].fields.get(f as usize) {
            Some(field) => ty = field.ty,
            None => return Ty::Error,
        }
    }
    ty
}

fn store_value_at(b: &mut Builder, slot: Slot, path: &mut Vec<u32>, value: &hir::Expr) {
    match &value.kind {
        hir::ExprKind::StructLit { fields, .. } => {
            for (i, fe) in fields.iter().enumerate() {
                path.push(i as u32);
                store_value_at(b, slot, path, fe);
                path.pop();
            }
        }
        _ => {
            let op = lower_expr(b, value);
            b.push(Stmt::StoreField(slot, path.clone(), op));
        }
    }
}

fn store_array_elems(b: &mut Builder, slot: Slot, elems: &[hir::Expr], elem: Ty) {
    if matches!(elem, Ty::Struct(_)) {
        for (i, e) in elems.iter().enumerate() {
            if let hir::ExprKind::StructLit { fields, .. } = &e.kind {
                for (j, fe) in fields.iter().enumerate() {
                    let v = lower_expr(b, fe);
                    b.push(Stmt::StoreElemField(slot, index_const(i), vec![j as u32], v));
                }
            }
        }
    } else {
        for (i, e) in elems.iter().enumerate() {
            let v = lower_expr(b, e);
            b.push(Stmt::StoreIndex(slot, index_const(i), v));
        }
    }
}

/// `src.map(f).where(p)….{sum,reduce}` → one loop folding the post-stage elements into
/// an accumulator. `fold` is the binary reducer (`None` = `+`), `init` seeds the
/// accumulator (type `acc_ty`). Stages run inline (fusion); a failing `where` skips to
/// the increment, so no intermediate array is built.
/// How a fused pipeline's surviving elements combine into the result.
enum Reducer {
    /// `sum`: `acc + element`.
    Sum,
    /// `count`: `acc + 1` (element value ignored).
    Count,
    /// `reduce(init, f)`: `f(acc, element)`. `captures` are a lifted lambda's captured values,
    /// passed after the `(acc, element)` arguments.
    Fold { func: String, captures: Vec<hir::Expr> },
    /// `any(p)` / `all(p)`: `acc || p(element)` / `acc && p(element)`. `captures` as `Fold`.
    AnyAll { func: String, captures: Vec<hir::Expr>, all: bool },
    /// `min` / `max`: keep `element` when it is smaller / larger than `acc`.
    MinMax { is_max: bool },
}

/// The set-up of a pipeline source: a stack array (slot + const length), a struct array
/// (slot), or a `{ptr,len}`-shaped value — a `slice` or an owned `array` (operand + runtime
/// length). Shared by the reducing and collecting loops.
struct SrcSetup {
    slot: Slot,
    slice_val: Option<Operand>,
    bound: Operand,
    scalar_slot: bool,
    /// `Some((struct_id, layout))` when the source is an owned, dynamic `array<Struct>` — a
    /// `{ptr,len}` view (`slice_val`) addressed by pointer + index for field projection (MMv2
    /// slice 8d-2). The loop keeps it index-addressed (no up-front element load) and projects
    /// fields via the layout seam `lower_field_access`. The layout is carried (not discarded) so
    /// it reaches that seam — adding `Layout::Soa` then forces a match there.
    struct_view: Option<(u32, Layout)>,
    /// An unbound free-standing owned-array temporary that this source materialized in place
    /// (`[..].to_array().sum()` with no arena): its `{ptr,len}` value, to be freed by the
    /// consuming loop once done. `None` for slots, slices, bound locals, and arena temporaries
    /// (the latter are bulk-freed by the arena).
    temp_free: Option<Operand>,
}

/// The arguments for a stage function call: the element, then any captured values (a lifted
/// lambda's captured enclosing locals, passed by value). Captures are lowered each iteration —
/// they reference loop-invariant enclosing locals, so LLVM hoists the loads out of the loop.
fn stage_call_args(b: &mut Builder, arg: Operand, captures: &[hir::Expr]) -> Vec<Operand> {
    let mut args = Vec::with_capacity(1 + captures.len());
    args.push(arg);
    for c in captures {
        args.push(lower_expr(b, c));
    }
    args
}

fn setup_source(b: &mut Builder, source: &hir::Expr) -> SrcSetup {
    match source.ty {
        // `slice<T>`, owned `array<T>`, and `array<slice<T>>` (a `chunks` result, element =
        // `slice<T>`) all share the `{ptr,len}` layout and runtime length.
        Ty::Slice(_) | Ty::DynArray(_) | Ty::DynSliceArray(_) => {
            let sv = lower_expr(b, source);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            // A source that *owns* a fresh free-standing buffer nothing else holds must be freed
            // by the consuming loop: a `.to_array()` / `.scan()` materialization, or a call
            // returning an owned `array<T>` (`make().sum()` — ownership transferred to us). A
            // bound `Local`
            // and a struct `Field` are borrows (freed by the owner's exit `Drop`), and arena
            // temporaries are bulk-freed, so none of those are freed here. `Block`/`If` sources
            // may *borrow* a bound local in a branch (e.g. `(if c { ys } else { zs }).sum()`), so
            // blanket-freeing them would double-free — they are left as a sound, bounded leak.
            // `chunks` (runtime `align_rt_chunks`) and a function's owned-array return are *always*
            // heap-allocated, so they must be freed even inside an `arena {}` (the arena's bulk-free
            // doesn't cover them). The materializing terminals instead arena-allocate when inside an
            // arena (bulk-freed there), so the loop frees them only outside one.
            let always_heap = matches!(
                source.kind,
                hir::ExprKind::ArrayChunks { .. } | hir::ExprKind::Call { .. }
            );
            let arena_if_in_arena = matches!(
                source.kind,
                hir::ExprKind::ArrayToArray { .. } | hir::ExprKind::ArrayScan { .. }
                    | hir::ExprKind::ArrayParMap { .. } | hir::ExprKind::ArraySort { .. } | hir::ExprKind::ArraySortBy { .. }
            );
            let temp_free =
                (always_heap || (arena_if_in_arena && b.arenas.is_empty())).then(|| sv.clone());
            SrcSetup { slot: 0, slice_val: Some(sv), bound: Operand::Value(len), scalar_slot: false, struct_view: None, temp_free }
        }
        // An owned, dynamic `array<Struct>`: a `{ptr,len}` view addressed by pointer for field
        // projection (slice 8d-2). It is a bound local borrow (sema requires a variable source),
        // so nothing is freed by the loop — the owner's exit `Drop` frees the buffer.
        Ty::DynStructArray(id, layout) => {
            let sv = lower_expr(b, source);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            SrcSetup { slot: 0, slice_val: Some(sv), bound: Operand::Value(len), scalar_slot: false, struct_view: Some((id, layout)), temp_free: None }
        }
        // A `soa<Struct>` view: a `{ptr,len}` column-major buffer. Same `{ptr,len}` handling as an
        // owned struct array, but the `Layout::Soa` struct-view makes field access column-addressed.
        Ty::Soa(id) => {
            let sv = lower_expr(b, source);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            SrcSetup { slot: 0, slice_val: Some(sv), bound: Operand::Value(len), scalar_slot: false, struct_view: Some((id, Layout::Soa)), temp_free: None }
        }
        _ => {
            let (slot, n) = array_source_slot(b, source);
            SrcSetup {
                slot,
                slice_val: None,
                bound: Operand::Const(Const::Int(n, i64_ty())),
                scalar_slot: matches!(source.ty, Ty::Array(..)),
                struct_view: None,
                temp_free: None,
            }
        }
    }
}

/// The **single layout seam** for struct-array element-field addressing — the one place that
/// turns `arr[i].field` into a load, shared by the fused pipeline (8d-2) and surface indexing
/// (8f). A stack-slot (fixed) `array<Struct>` is always AoS and uses the slot-based
/// [`Rvalue::IndexField`]; an owned dynamic `array<Struct>` view (`struct_view = Some((id,
/// layout))`) carries its [`Layout`] here. The `match layout` below is the SoA hook: today only
/// `Layout::Aos` (the pointer-based [`Rvalue::IndexFieldPtr`], `element, field` GEP); when
/// `Layout::Soa` (`soa array<T>`) lands at M6, this match goes non-exhaustive — a compile error
/// that points exactly here, the one site SoA's column-array indexing must branch in.
fn lower_field_access(
    b: &mut Builder,
    struct_view: Option<(u32, Layout)>,
    slice_val: &Option<Operand>,
    slot: Slot,
    index: &Operand,
    field: u32,
    out_ty: Ty,
) -> ValueId {
    let v = b.fresh_value(out_ty);
    match struct_view {
        Some((struct_id, layout)) => match layout {
            Layout::Aos => b.push(Stmt::Let(
                v,
                Rvalue::IndexFieldPtr {
                    base: slice_val.clone().expect("a struct-view source has a {ptr,len} value"),
                    index: index.clone(),
                    field,
                    struct_id,
                },
            )),
            // SoA: address column `field` then element `index` within it
            // (`column_base(field) + index`), reading only the touched column.
            Layout::Soa => b.push(Stmt::Let(
                v,
                Rvalue::IndexColumn {
                    base: slice_val.clone().expect("a soa source has a {ptr,len} value"),
                    index: index.clone(),
                    field,
                    struct_id,
                },
            )),
        },
        None => b.push(Stmt::Let(v, Rvalue::IndexField(slot, index.clone(), field))),
    }
    v
}

/// Load a **whole** struct element `src[index]` for a `map(f)` whose `f` consumes the struct by
/// value (the whole-element companion of [`lower_field_access`]). A fixed stack `array<Struct>`
/// (`struct_view == None`) loads the aggregate straight from its slot ([`Rvalue::Index`]); an
/// owned dynamic `array<Struct>` view loads through the buffer pointer ([`Rvalue::IndexPtr`]). The
/// `match layout` mirrors the field seam: `Layout::Soa` (M6) makes it non-exhaustive here too.
fn lower_struct_elem(
    b: &mut Builder,
    struct_view: Option<(u32, Layout)>,
    slice_val: &Option<Operand>,
    slot: Slot,
    index: &Operand,
    struct_id: u32,
) -> ValueId {
    let v = b.fresh_value(Ty::Struct(struct_id));
    match struct_view {
        Some((sid, layout)) => match layout {
            Layout::Aos => b.push(Stmt::Let(
                v,
                Rvalue::IndexPtr {
                    base: slice_val.clone().expect("a struct-view source has a {ptr,len} value"),
                    index: index.clone(),
                    struct_id: sid,
                },
            )),
            // Loading a whole struct out of a `soa` would gather every column at `index`. The first
            // soa cut allows only field projection / `where(.field)`, so sema rejects a whole-struct
            // stage over soa and this is unreachable.
            Layout::Soa => unreachable!("whole-struct access over a soa source is rejected in sema"),
        },
        None => b.push(Stmt::Let(v, Rvalue::Index(slot, index.clone()))),
    }
    v
}

fn lower_array_reduce(
    b: &mut Builder,
    source: &hir::Expr,
    stages: &[hir::Stage],
    acc_ty: Ty,
    init: Operand,
    reducer: Reducer,
) -> Operand {
    let elem_ty = acc_ty;
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src, struct_view, temp_free } = setup_source(b, source);

    let acc = b.new_slot(acc_ty);
    b.push(Stmt::Store(acc, init));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < len
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound)));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: address element i, run the stages, accumulate.
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    // A scalar array or a slice loads the element up front; a struct array (stack slot or a
    // `{ptr,len}` `array<Struct>` view) stays addressed by index until a `.field` projection.
    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
            _ => elem_ty,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::SliceIndex(sv.clone(), index.clone())));
        Some(Operand::Value(x))
    } else if scalar_slot_src {
        let src_elem = match source.ty {
            Ty::Array(s, _) => align_sema::scalar_to_ty(s),
            _ => elem_ty,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::Index(slot, index.clone())));
        Some(Operand::Value(x))
    } else {
        None
    };

    // Branchless `where` for *every* reducer: rather than a per-element branch that skips the
    // accumulate (it doesn't auto-vectorize, and defeats a scalable-ISA predicated tail), AND the
    // predicates into a `mask` and let the reducer `select` each masked-out lane to its identity.
    // The mask is `None` when the pipeline has no `where`.
    let mut mask: Option<Operand> = None;

    for stage in stages {
        match &stage.kind {
            hir::StageKind::Project { field } => {
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                // A scalar element is already loaded; a struct element consumed whole (a
                // `map(f)` with no prior `.field`) is loaded here by index.
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func, captures } => {
                // A scalar element is already loaded; a whole struct element (a struct-consuming
                // predicate, no prior projection) is loaded here by index. `where` keeps the
                // element, so `cur` is left unchanged either way.
                let arg = match &cur {
                    Some(a) => a.clone(),
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("where with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), call_args)));
                mask = Some(accumulate_mask(b, mask, Operand::Value(pred)));
            }
            hir::StageKind::WhereField { field } => {
                // Predicate on a struct element's (bool) field; the element is unchanged.
                let pred = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, Ty::Bool);
                mask = Some(accumulate_mask(b, mask, Operand::Value(pred)));
            }
        }
    }
    let a = b.fresh_value(acc_ty);
    b.push(Stmt::Let(a, Rvalue::Load(acc)));
    // Every reducer is branchless. When there is a `where` (`mask` is `Some`), each masked-out lane
    // contributes the reducer's identity so it cannot change the result — additive `0` (`sum`/
    // `count`), `+∞`/`−∞` (`min`/`max`, exactly the fold seed), `false`/`true` (`any`/`all`) — or, for
    // generic `reduce`, the accumulator is left unchanged (`acc = mask ? f(acc,v) : acc`), since a
    // user-supplied `f` has no identity (`init` is the starting accumulator, not an identity).
    //
    // NB: a `where` mask no longer *guards* the reducer's own function. With the branchless form the
    // reducer's `f`/predicate `p` (and any stage after the `where`) runs on masked-out elements too,
    // its contribution then discarded. That matches the shipped `sum`/`count` branchless form (a
    // post-`where` `map` already ran on every element) and is the deliberate cost of a vectorizable,
    // predication-ready loop (pipeline functions are pure — a masked-out element cannot differ
    // observably). See `05 §5`.
    let next: Operand = match &reducer {
        // `count`: acc + (mask ? 1 : 0).
        Reducer::Count => {
            let one = index_const(1);
            let inc = match &mask {
                Some(m) => {
                    let s = b.fresh_value(acc_ty);
                    b.push(Stmt::Let(s, Rvalue::Select { cond: m.clone(), a: one, b: index_const(0) }));
                    Operand::Value(s)
                }
                None => one,
            };
            let n = b.fresh_value(acc_ty);
            b.push(Stmt::Let(n, Rvalue::Bin(BinOp::Add, Operand::Value(a), inc)));
            Operand::Value(n)
        }
        // `sum`: acc + (mask ? cur : 0).
        Reducer::Sum => {
            let cur = cur.expect("sum needs a scalar element");
            let contribution = match &mask {
                Some(m) => {
                    let s = b.fresh_value(acc_ty);
                    b.push(Stmt::Let(s, Rvalue::Select { cond: m.clone(), a: cur, b: zero_of(acc_ty) }));
                    Operand::Value(s)
                }
                None => cur,
            };
            let n = b.fresh_value(acc_ty);
            b.push(Stmt::Let(n, Rvalue::Bin(BinOp::Add, Operand::Value(a), contribution)));
            Operand::Value(n)
        }
        // `reduce`: acc = mask ? f(acc, cur) : acc — accumulator-select (a masked-out lane leaves
        // the accumulator unchanged, so the result is the fold over the surviving elements, seeded
        // with `init`). No `where` → the fold result is the accumulator directly.
        Reducer::Fold { func, captures } => {
            let cur = cur.expect("reduce needs a scalar element");
            let mut args = vec![Operand::Value(a), cur];
            for c in captures {
                args.push(lower_expr(b, c));
            }
            let folded = b.fresh_value(acc_ty);
            b.push(Stmt::Let(folded, Rvalue::Call(func.clone(), args)));
            match &mask {
                Some(m) => {
                    let n = b.fresh_value(acc_ty);
                    b.push(Stmt::Let(n, Rvalue::Select { cond: m.clone(), a: Operand::Value(folded), b: Operand::Value(a) }));
                    Operand::Value(n)
                }
                None => Operand::Value(folded),
            }
        }
        // `any`/`all`: t = p(cur); acc = acc || (mask ? t : false)  /  acc && (mask ? t : true).
        // A full ||/&&-fold (no early exit) — the branchless, vectorizable shape.
        Reducer::AnyAll { func, captures, all } => {
            let cur = cur.expect("any/all needs a scalar element");
            let t = b.fresh_value(Ty::Bool);
            let args = stage_call_args(b, cur, captures);
            b.push(Stmt::Let(t, Rvalue::Call(func.clone(), args)));
            let contribution = match &mask {
                // masked-out contributes the fold identity: `any` (||) → false, `all` (&&) → true.
                Some(m) => {
                    let s = b.fresh_value(Ty::Bool);
                    b.push(Stmt::Let(s, Rvalue::Select { cond: m.clone(), a: Operand::Value(t), b: Operand::Const(Const::Bool(*all)) }));
                    Operand::Value(s)
                }
                None => Operand::Value(t),
            };
            let op = if *all { BinOp::And } else { BinOp::Or };
            let n = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(n, Rvalue::Bin(op, Operand::Value(a), contribution)));
            Operand::Value(n)
        }
        // `min`/`max`: acc = (cur `op` acc) ? cur : acc — the branchless min/max reduction idiom
        // LLVM recognizes (`llvm.{s,u}{min,max}` / `llvm.{min,max}imum`). The comparison is the same
        // one the former per-element branch used (`Lt` for min, `Gt` for max, floats → ordered
        // `OLT`/`OGT`), so NaN handling is unchanged: an ordered compare with a NaN operand is false,
        // so the running best is kept and NaN elements are skipped, exactly as before. A `where`
        // first selects each masked-out lane to the type extreme that can never win (`min` → type
        // max / `+∞`, `max` → type min / `−∞`), which is exactly the fold seed (`extreme_of`) — so an
        // all-masked selection still returns that seed, unchanged from the branch form.
        Reducer::MinMax { is_max } => {
            let cur = cur.expect("min/max needs a scalar element");
            let cur = match &mask {
                Some(m) => {
                    let s = b.fresh_value(acc_ty);
                    b.push(Stmt::Let(s, Rvalue::Select { cond: m.clone(), a: cur, b: extreme_of(acc_ty, *is_max) }));
                    Operand::Value(s)
                }
                None => cur,
            };
            let op = if *is_max { BinOp::Gt } else { BinOp::Lt };
            let cmp = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(cmp, Rvalue::Bin(op, cur.clone(), Operand::Value(a))));
            let n = b.fresh_value(acc_ty);
            b.push(Stmt::Let(n, Rvalue::Select { cond: Operand::Value(cmp), a: cur, b: Operand::Value(a) }));
            Operand::Value(n)
        }
    };
    b.push(Stmt::Store(acc, next));
    b.terminate(Term::Goto(cont));

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    b.cur = exit;
    let r = b.fresh_value(elem_ty);
    b.push(Stmt::Let(r, Rvalue::Load(acc)));
    // Free a free-standing `.to_array()` temporary now that the fold has consumed it. The
    // result `r` is a scalar accumulator independent of the buffer, so this is safe.
    if let Some(tmp) = temp_free {
        b.push(Stmt::DropValue(tmp));
    }
    Operand::Value(r)
}

/// What a materializing collect loop appends per surviving element.
enum CollectKind {
    /// `to_array`: append the element itself.
    Collect,
    /// `scan(init, f)`: thread an accumulator (`acc = f(acc, element)`, seeded with `init`) and
    /// append the running accumulator. `captures` are a lifted lambda's captured values, passed
    /// after the `(acc, element)` arguments.
    Scan { func: String, init: Operand, captures: Vec<hir::Expr> },
}

/// `source.….to_array()` / `.scan(init, f)` — the fused loop, but each surviving element is
/// appended to a freshly allocated buffer (arena-bump inside an arena, else heap) instead of
/// folded into a scalar. Yields an owned `array<T>` value `{ ptr, len }` where `len` is the
/// survivor count. (MMv2 slice 3 `to_array`; slice 5 adds `scan`.)
fn lower_array_collect(b: &mut Builder, source: &hir::Expr, stages: &[hir::Stage], elem: Ty, kind: CollectKind) -> Operand {
    // Inside an arena → bump-allocate (bulk-freed); otherwise → free-standing heap (dropped).
    let arena = b.arenas.last().copied();
    // A collect source can itself be a fresh unbound owned temporary (`make().map(f).to_array()`
    // — `make()` returns an owned array nothing else holds). The copy loop consumes it into the
    // new output buffer, so free that source temporary at the exit (the result is a separate
    // buffer). `temp_free` is None for slots / bound locals / arena temporaries.
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src, struct_view, temp_free } = setup_source(b, source);

    // Output buffer: `bound` (upper-bound = source length) elements. map/where never grow
    // the count, so the buffer never needs to be resized.
    let out_ptr = b.fresh_value(Ty::Box(scalar_of(elem)));
    let alloc = match arena {
        Some(h) => Rvalue::ArenaAlloc { handle: Operand::Value(h), count: bound.clone(), elem },
        // KNOWN LIMITATION (deferred): a free-standing `.to_array()` that is consumed as an
        // unbound temporary (`[..].to_array().sum()`) is never bound to a `drop_local`, so its
        // buffer is leaked. Sound (no UAF) and bounded; the "complete drop coverage" slice will
        // either bind such temporaries to synthetic drop slots or fuse the terminal so no
        // materialization happens. Arena mode is unaffected (bulk-freed).
        None => Rvalue::HeapAllocBuf { count: bound.clone(), elem },
    };
    b.push(Stmt::Let(out_ptr, alloc));

    // `acc` is the running output index (= final length); `iv` is the source index.
    let acc = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc, Operand::Const(Const::Int(0, i64_ty()))));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));
    // `scan` threads an accumulator (output element type) seeded with `init`.
    let scan_acc = match &kind {
        CollectKind::Scan { init, .. } => {
            let s = b.new_slot(elem);
            b.push(Stmt::Store(s, init.clone()));
            Some(s)
        }
        CollectKind::Collect => None,
    };

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < len
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound)));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: address element i, run the stages, append survivors.
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::SliceIndex(sv.clone(), index.clone())));
        Some(Operand::Value(x))
    } else if scalar_slot_src {
        let src_elem = match source.ty {
            Ty::Array(s, _) => align_sema::scalar_to_ty(s),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::Index(slot, index.clone())));
        Some(Operand::Value(x))
    } else {
        None
    };

    for stage in stages {
        match &stage.kind {
            hir::StageKind::Project { field } => {
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                // A scalar element is already loaded; a struct element consumed whole (a
                // `map(f)` with no prior `.field`) is loaded here by index.
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func, captures } => {
                // A scalar element is already loaded; a whole struct element (a struct-consuming
                // predicate, no prior projection) is loaded here by index. `where` keeps the
                // element, so `cur` is left unchanged either way.
                let arg = match &cur {
                    Some(a) => a.clone(),
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("where with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), call_args)));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                let pred = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, Ty::Bool);
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }

    // append: out_ptr[out_idx] = <value>; out_idx += 1. For `to_array` the value is the
    // element; for `scan` it is the updated accumulator `acc = f(acc, element)`.
    let cur = cur.expect("to_array/scan needs a scalar element");
    let value = match (&kind, scan_acc) {
        (CollectKind::Scan { func, captures, .. }, Some(acc_slot)) => {
            let prev = b.fresh_value(elem);
            b.push(Stmt::Let(prev, Rvalue::Load(acc_slot)));
            let folded = b.fresh_value(elem);
            let mut args = vec![Operand::Value(prev), cur];
            for c in captures {
                args.push(lower_expr(b, c));
            }
            b.push(Stmt::Let(folded, Rvalue::Call(func.clone(), args)));
            b.push(Stmt::Store(acc_slot, Operand::Value(folded)));
            Operand::Value(folded)
        }
        _ => cur,
    };
    let out_idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(out_idx, Rvalue::Load(acc)));
    b.push(Stmt::PtrStore(Operand::Value(out_ptr), Operand::Value(out_idx), value));
    let next = b.fresh_value(i64_ty());
    b.push(Stmt::Let(next, Rvalue::Bin(BinOp::Add, Operand::Value(out_idx), index_const(1))));
    b.push(Stmt::Store(acc, Operand::Value(next)));
    b.terminate(Term::Goto(cont));

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    // exit: build the owned array { out_ptr, out_idx }.
    b.cur = exit;
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::Load(acc)));
    let arr = b.fresh_value(Ty::DynArray(scalar_of(elem)));
    b.push(Stmt::Let(arr, Rvalue::MakeDynArray { ptr: Operand::Value(out_ptr), len: Operand::Value(len) }));
    // Free the source temporary now its elements have been copied into the new buffer.
    if let Some(tmp) = temp_free {
        b.push(Stmt::DropValue(tmp));
    }
    Operand::Value(arr)
}

/// Abort unless `have == want` — the cold-edge guard `map_into` emits so `dst.len() == source.len()`
/// holds before the fused loop writes into the caller's buffer. Same shape as [`emit_bounds_check`]:
/// a branch to a `-> !` reporting block (`Unreachable`), continuing in the `ok` block.
fn emit_len_eq_check(b: &mut Builder, have: Operand, want: Operand) {
    let ne = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(ne, Rvalue::Bin(BinOp::Ne, have.clone(), want.clone())));
    let fail = b.new_block();
    let ok = b.new_block();
    b.terminate(Term::Branch(Operand::Value(ne), fail, ok));
    b.cur = fail;
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::Call("len_mismatch_fail".to_string(), vec![have, want])));
    b.terminate(Term::Unreachable);
    b.cur = ok;
}

/// `source.….map_into(dst)` — write each post-stage element into the caller's writable slice `dst`
/// (a `to_array` sibling that reuses caller storage). One fused counted loop stores `dst[i] =
/// f(source[i])`; sema restricts the pipeline to length-preserving stages (`map`/`.field`), so the
/// loop is a clean `for i in 0..len` with no survivor counter. The runtime first asserts
/// `dst.len() == source.len()` (abort otherwise). When the source reads a runtime slice buffer, the
/// source load and `dst` store are tagged with the loop's disjoint `in`/`out` alias scopes
/// ([`Rvalue::SliceIndexNoalias`]/[`Stmt::PtrStoreNoalias`]) — sema proved `dst` is disjoint from the
/// source — so the vectorizer drops its runtime overlap guard. Yields `()`.
fn lower_array_map_into(b: &mut Builder, source: &hir::Expr, stages: &[hir::Stage], dst: &hir::Expr, elem: Ty) -> Operand {
    let SrcSetup { slot, slice_val, bound, scalar_slot, struct_view, temp_free } = setup_source(b, source);
    // `map_into` reads its source (never consumes it), so there is never a fresh owned source
    // buffer to free here. Sema's alias gate restricts the source to a named array/slice, a
    // sub-slice of one, or a fixed array literal — never a fn-returned owned `array<T>` or a nested
    // materializing terminal — so `setup_source` cannot hand back a `temp_free`. Assert the
    // invariant rather than silently leak (or double-free) if that gate ever loosens.
    debug_assert!(temp_free.is_none(), "map_into source must not own a fresh buffer (sema alias gate)");
    // The source load is scope-tagged only when it reads a runtime slice buffer (`SliceIndex`); a
    // fixed stack-array source (`Index`) can't alias a caller slice, so it needs no metadata.
    let emit_noalias = slice_val.is_some();
    let scope = b.fresh_alias_scope();

    // Destination `{ptr,len}`: its buffer pointer (store target) and length (for the guard).
    let dst_val = lower_expr(b, dst);
    let dst_ptr = b.fresh_value(Ty::Box(scalar_of(elem)));
    b.push(Stmt::Let(dst_ptr, Rvalue::SlicePtr(dst_val.clone())));
    let dst_len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(dst_len, Rvalue::SliceLen(dst_val)));

    // Runtime length check: `dst.len() == source.len()`, else abort (cold edge).
    emit_len_eq_check(b, Operand::Value(dst_len), bound.clone());

    // for i in 0..len: dst[i] = <post-stage element i>.
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));
    let header = b.new_block();
    let body = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound.clone())));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    // Load the source element (scalar sources); struct sources defer to the first Project stage.
    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        let load = if emit_noalias {
            Rvalue::SliceIndexNoalias { slice: sv.clone(), index: index.clone(), scope }
        } else {
            Rvalue::SliceIndex(sv.clone(), index.clone())
        };
        b.push(Stmt::Let(x, load));
        Some(Operand::Value(x))
    } else if scalar_slot {
        let src_elem = match source.ty {
            Ty::Array(s, _) => align_sema::scalar_to_ty(s),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::Index(slot, index.clone())));
        Some(Operand::Value(x))
    } else {
        None
    };

    // Apply the length-preserving stages (sema rejects `where`, so there is no skip branch).
    for stage in stages {
        match &stage.kind {
            hir::StageKind::Project { field } => {
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { .. } | hir::StageKind::WhereField { .. } => {
                unreachable!("map_into rejects filtering `where` stages in sema")
            }
        }
    }

    let value = cur.expect("map_into needs a scalar element");
    let store = if emit_noalias {
        Stmt::PtrStoreNoalias { ptr: Operand::Value(dst_ptr), index: index.clone(), value, scope }
    } else {
        Stmt::PtrStore(Operand::Value(dst_ptr), index.clone(), value)
    };
    b.push(store);

    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, index, index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    b.cur = exit;
    Operand::Const(Const::Unit)
}

/// `arr.to_soa()` — transpose an AoS `array<Struct>` source into a column-major `soa<Struct>`
/// view. One fused loop reads each element and scatters its fields into their columns (the write
/// counterpart of a soa field scan). The buffer is arena-bump-allocated (sema requires an arena),
/// and the result `{ ptr, len }` view is region-tied to it. The construction primitive that makes
/// `soa<T>` usable in pure Align — build once, then a multi-column scan touches only the fields
/// it reads.
fn lower_array_to_soa(b: &mut Builder, source: &hir::Expr, struct_id: u32) -> Operand {
    // The source is a whole-struct array (no stages), so `struct_view`/`slot` address its elements;
    // `bound` is the row count `len` (a constant for a fixed array, a runtime value otherwise).
    let SrcSetup { slot, slice_val, bound, struct_view, .. } = setup_source(b, source);
    transpose_to_soa(b, struct_view, &slice_val, slot, bound, struct_id)
}

/// Transpose an AoS source (already set up for element addressing) into an arena-allocated
/// column-major `soa<Struct>`: allocate the column buffer for `len` rows, then run one fused loop
/// reading each element's fields (`lower_field_access`) and scattering them into their columns
/// (`StoreColumn`). Returns the `{ptr,len}` soa view. Shared by `to_soa` (a literal/local array
/// source) and `json.decode → soa` (a decoded AoS value). The buffer is bump-allocated in the
/// innermost arena (sema requires one), so the view is region-tied to it and bulk-freed.
fn transpose_to_soa(
    b: &mut Builder,
    struct_view: Option<(u32, Layout)>,
    slice_val: &Option<Operand>,
    slot: Slot,
    len: Operand,
    struct_id: u32,
) -> Operand {
    let handle = *b.arenas.last().expect("to_soa outside an arena (sema-checked)");
    let field_tys: Vec<Ty> = b.structs[struct_id as usize].fields.iter().map(|f| f.ty).collect();

    // Allocate the column-major buffer (`len` rows). The element-pointer type is opaque, so the
    // `Box` scalar is irrelevant — use the first field's. A soa struct always has ≥1 field (sema).
    let first_ty = *field_tys.first().expect("a soa struct has at least one field");
    let first_scalar = align_sema::ty_to_scalar(first_ty).expect("soa field is a scalar");
    let buf = b.fresh_value(Ty::Box(first_scalar));
    b.push(Stmt::Let(buf, Rvalue::SoaAlloc { handle: Operand::Value(handle), len: len.clone(), struct_id }));

    // for i in 0..len: scatter element i's fields into their columns.
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));
    let header = b.new_block();
    let body = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), len.clone())));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);
    for (field, fty) in field_tys.iter().enumerate() {
        let v = lower_field_access(b, struct_view, slice_val, slot, &index, field as u32, *fty);
        b.push(Stmt::StoreColumn {
            base: Operand::Value(buf),
            len: len.clone(),
            index: index.clone(),
            field: field as u32,
            struct_id,
            value: Operand::Value(v),
        });
    }
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, index.clone(), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    // exit: build the soa `{ ptr, len }` view over the column buffer.
    b.cur = exit;
    let soa = b.fresh_value(Ty::Soa(struct_id));
    b.push(Stmt::Let(soa, Rvalue::MakeDynArray { ptr: Operand::Value(buf), len }));
    Operand::Value(soa)
}

/// `json.decode(input)` into a `soa<Struct>` (runway step 2) — decode the JSON array of objects
/// **directly** into arena-allocated column-major buffers via [`Rvalue::JsonDecodeSoa`]: the runtime
/// counts the rows (so the column offsets can be computed), allocates the columns from the enclosing
/// arena, and fills them in one value-parse pass — no AoS intermediate, no transpose. Mirrors
/// [`lower_json_decode_struct_array`]'s Ok/Err branch. The soa columns are all primitive scalars
/// (sema-enforced), so the result is self-contained — bound to the arena, not the input.
fn lower_json_decode_soa(b: &mut Builder, struct_id: u32, input: &hir::Expr, result_ty: Ty) -> Operand {
    let soa_ty = Ty::Soa(struct_id);
    let out = b.new_slot(soa_ty);
    let inp = lower_expr(b, input);
    // The column buffer is arena-bump-allocated (sema requires `json.decode → soa` inside an arena),
    // so the runtime needs the innermost arena handle.
    let arena = *b.arenas.last().expect("json.decode → soa outside an arena (sema-checked)");
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecodeSoa { struct_id, input: inp, out, arena: Operand::Value(arena) }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the soa `{ptr,len}` view (the column buffer is arena-tied — no Drop) and wrap it.
    b.cur = ok_bb;
    let soa = b.fresh_value(soa_ty);
    b.push(Stmt::Let(soa, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(soa))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code (the out slot was zeroed → no buffer allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `s.group_by(.key).<op>(…)` — column-oriented grouped aggregate over a `soa<Struct>` local. Reads
/// the key column (and the value column for sum/min/max — `count` has none) as `{ptr,len}` slices
/// (`SoaColumn`), heap-allocates two owned output buffers sized to the column length, calls the
/// runtime hash-aggregate for the op, and builds the result tuple `(array<i64>, array<i64>)`
/// (distinct keys, per-key aggregate). The output arrays are owned (heap, `Drop`-freed) so the tuple
/// can escape; the runtime's internal table is its own concern.
fn lower_array_group_agg(b: &mut Builder, base: u32, struct_id: u32, key_field: u32, value_field: Option<u32>, op: hir::GroupOp, tuple_id: u32) -> Operand {
    let i64s = scalar_of(i64_ty());
    let islice = Ty::Slice(i64s);
    // The key column (always) and the value column (sum/min/max). `count` has no value column, so it
    // reuses the key column as the (codegen-ignored) `vals` operand — the runtime `count` entry point
    // takes no values.
    let key_col = b.fresh_value(islice);
    b.push(Stmt::Let(key_col, Rvalue::SoaColumn { base, struct_id, field: key_field }));
    let val_col = match value_field {
        Some(vf) => {
            let v = b.fresh_value(islice);
            b.push(Stmt::Let(v, Rvalue::SoaColumn { base, struct_id, field: vf }));
            v
        }
        None => key_col,
    };
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(key_col))));
    // Output buffers (owned heap, sized at the column length = an upper bound on the group count).
    let out_keys = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_keys, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: i64_ty() }));
    let out_vals = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_vals, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: i64_ty() }));
    // Aggregate → group count.
    let count = b.fresh_value(i64_ty());
    b.push(Stmt::Let(
        count,
        Rvalue::GroupAgg {
            keys: Operand::Value(key_col),
            vals: Operand::Value(val_col),
            out_keys: Operand::Value(out_keys),
            out_vals: Operand::Value(out_vals),
            op,
        },
    ));
    // Build the two owned result arrays and the tuple.
    let karr = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(karr, Rvalue::MakeDynArray { ptr: Operand::Value(out_keys), len: Operand::Value(count) }));
    let varr = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(varr, Rvalue::MakeDynArray { ptr: Operand::Value(out_vals), len: Operand::Value(count) }));
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems: vec![Operand::Value(karr), Operand::Value(varr)] }));
    Operand::Value(tup)
}

/// `s.group_by(.str_key).{sum,min,max}(.i64_value)` / `.count()` over a `soa<Struct>` with a **str
/// key column**. The columnar counterpart of [`lower_array_group_str`]: it extracts the key column
/// (a `slice<str>`) and value column (a `slice<i64>`) via [`Rvalue::SoaColumn`] like the i64-key soa
/// path, but interns the `str` keys — the two contiguous columns feed [`Rvalue::GroupAggStrCols`]
/// (`align_rt_group_*_str_cols`). Result tuple `(array<str>, array<i64>)`; the owned key buffer's
/// `str` elements borrow the soa's string storage, so the tuple is region-tied to the source (sema).
fn lower_array_group_str_cols(b: &mut Builder, base: u32, struct_id: u32, key_field: u32, value_field: Option<u32>, op: hir::GroupOp, tuple_id: u32) -> Operand {
    let strs = scalar_of(Ty::Str);
    let i64s = scalar_of(i64_ty());
    // The str key column (always) and the i64 value column (sum/min/max). `count` has no value
    // column, so it reuses the key column as the (runtime-ignored) `vals` operand.
    let key_col = b.fresh_value(Ty::Slice(strs));
    b.push(Stmt::Let(key_col, Rvalue::SoaColumn { base, struct_id, field: key_field }));
    let val_col = match value_field {
        Some(vf) => {
            let v = b.fresh_value(Ty::Slice(i64s));
            b.push(Stmt::Let(v, Rvalue::SoaColumn { base, struct_id, field: vf }));
            v
        }
        None => key_col,
    };
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(key_col))));
    // Output buffers (owned heap): `str`-view keys + i64 aggregates, each sized at the row count.
    let out_keys = b.fresh_value(Ty::Box(strs));
    b.push(Stmt::Let(out_keys, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: Ty::Str }));
    let out_vals = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_vals, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: i64_ty() }));
    // Intern + aggregate over the two columns → group count.
    let count = b.fresh_value(i64_ty());
    b.push(Stmt::Let(
        count,
        Rvalue::GroupAggStrCols {
            keys: Operand::Value(key_col),
            vals: Operand::Value(val_col),
            out_keys: Operand::Value(out_keys),
            out_vals: Operand::Value(out_vals),
            op,
        },
    ));
    // Build the two owned result arrays and the tuple.
    let karr = b.fresh_value(Ty::DynArray(strs));
    b.push(Stmt::Let(karr, Rvalue::MakeDynArray { ptr: Operand::Value(out_keys), len: Operand::Value(count) }));
    let varr = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(varr, Rvalue::MakeDynArray { ptr: Operand::Value(out_vals), len: Operand::Value(count) }));
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems: vec![Operand::Value(karr), Operand::Value(varr)] }));
    Operand::Value(tup)
}

/// `s.group_by(.str_key).sum(.i64_value)` over an AoS `array<Struct>` — the dictionary-id rail.
/// Loads `base`'s `{ptr,len}` for the row count, heap-allocates a `str`-view key buffer + an i64 sum
/// buffer (each sized at the row count), interns + sums via [`Rvalue::GroupAggStr`], and builds the
/// result tuple `(array<str>, array<i64>)`. The key buffer is owned (heap, `Drop`-freed) but its
/// elements are `str` views borrowing `base`, so the tuple is region-tied to the source (sema).
fn lower_array_group_str(b: &mut Builder, base: u32, struct_id: u32, key_field: u32, value_field: Option<u32>, op: hir::GroupOp, tuple_id: u32) -> Operand {
    let strs = scalar_of(Ty::Str);
    let i64s = scalar_of(i64_ty());
    // Load the AoS array to get its length (an upper bound on the group count).
    let arr = b.fresh_value(Ty::DynStructArray(struct_id, Layout::Aos));
    b.push(Stmt::Let(arr, Rvalue::Load(base)));
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(arr))));
    // Output buffers (owned heap): `str`-view keys + i64 sums, each sized at the row count.
    let out_keys = b.fresh_value(Ty::Box(strs));
    b.push(Stmt::Let(out_keys, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: Ty::Str }));
    let out_vals = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_vals, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: i64_ty() }));
    // Intern + aggregate → group count.
    let count = b.fresh_value(i64_ty());
    b.push(Stmt::Let(
        count,
        Rvalue::GroupAggStr {
            base,
            struct_id,
            key_field,
            value_field,
            op,
            out_keys: Operand::Value(out_keys),
            out_vals: Operand::Value(out_vals),
        },
    ));
    // Build the two owned result arrays and the tuple.
    let karr = b.fresh_value(Ty::DynArray(strs));
    b.push(Stmt::Let(karr, Rvalue::MakeDynArray { ptr: Operand::Value(out_keys), len: Operand::Value(count) }));
    let varr = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(varr, Rvalue::MakeDynArray { ptr: Operand::Value(out_vals), len: Operand::Value(count) }));
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems: vec![Operand::Value(karr), Operand::Value(varr)] }));
    Operand::Value(tup)
}

/// `s.group_by(.str_key).agg(sum(.a), max(.b), count(), …)` over an AoS `array<Struct>` — the fused
/// multi-aggregate str rail. Loads `base`'s `{ptr,len}` for the row count, heap-allocates the
/// `str`-view key buffer + one i64 buffer per aggregate (each sized at the row count), runs the
/// single-pass [`Rvalue::GroupAggMultiStr`] (intern key once, fold K accumulators), and builds the
/// result tuple `(array<str>, array<i64> × K)`. The key buffer's `str` elements borrow `base`, so the
/// tuple is region-tied to the source (sema).
fn lower_array_group_multi_str(b: &mut Builder, base: u32, struct_id: u32, key_field: u32, aggs: &[hir::GroupAgg1], tuple_id: u32) -> Operand {
    let strs = scalar_of(Ty::Str);
    let i64s = scalar_of(i64_ty());
    // Load the AoS array to get its length (an upper bound on the group count).
    let arr = b.fresh_value(Ty::DynStructArray(struct_id, Layout::Aos));
    b.push(Stmt::Let(arr, Rvalue::Load(base)));
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(arr))));
    // Owned `str`-view key buffer + one owned i64 output column per aggregate, each sized at the row
    // count (the upper bound on the group count).
    let out_keys = b.fresh_value(Ty::Box(strs));
    b.push(Stmt::Let(out_keys, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: Ty::Str }));
    let out_vals: Vec<ValueId> = aggs
        .iter()
        .map(|_| {
            let v = b.fresh_value(Ty::Box(i64s));
            b.push(Stmt::Let(v, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: i64_ty() }));
            v
        })
        .collect();
    // Fused one-pass aggregate → group count.
    let count = b.fresh_value(i64_ty());
    b.push(Stmt::Let(
        count,
        Rvalue::GroupAggMultiStr {
            base,
            struct_id,
            key_field,
            aggs: aggs.iter().map(|a| (a.op, a.value_field)).collect(),
            out_keys: Operand::Value(out_keys),
            out_vals: out_vals.iter().map(|v| Operand::Value(*v)).collect(),
        },
    ));
    // Build the result tuple: distinct keys + one owned array per aggregate column.
    let karr = b.fresh_value(Ty::DynArray(strs));
    b.push(Stmt::Let(karr, Rvalue::MakeDynArray { ptr: Operand::Value(out_keys), len: Operand::Value(count) }));
    let mut elems = vec![Operand::Value(karr)];
    for v in &out_vals {
        let varr = b.fresh_value(Ty::DynArray(i64s));
        b.push(Stmt::Let(varr, Rvalue::MakeDynArray { ptr: Operand::Value(*v), len: Operand::Value(count) }));
        elems.push(Operand::Value(varr));
    }
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems }));
    Operand::Value(tup)
}

/// `s.dict_encode(.key)` — build a `dict_encoded` value (the A2 reuse rail). Loads `base`'s AoS
/// `{ptr,len}` (the borrowed source slice + row count), heap-allocates a dense-id i64 buffer (one per
/// row) + a `str` dictionary buffer, interns via [`Rvalue::DictEncode`], and assembles the 3-slice
/// value `{ source, ids, dict }`. `source` borrows `base`; `ids`/`dict` are owned (freed by the
/// value's `Drop`). The encoded value is region-tied to `base` (sema).
fn lower_dict_encode(b: &mut Builder, base: u32, struct_id: u32, key_field: u32) -> Operand {
    let strs = scalar_of(Ty::Str);
    let i64s = scalar_of(i64_ty());
    // Load the source AoS `{ptr,len}` (the borrowed source view + the row count).
    let arr = b.fresh_value(Ty::DynStructArray(struct_id, Layout::Aos));
    b.push(Stmt::Let(arr, Rvalue::Load(base)));
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(arr))));
    // Owned outputs: a dense id per row, and the dictionary (<= row count distinct keys).
    let out_ids = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_ids, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: i64_ty() }));
    let out_dict = b.fresh_value(Ty::Box(strs));
    b.push(Stmt::Let(out_dict, Rvalue::HeapAllocBuf { count: Operand::Value(len), elem: Ty::Str }));
    // Intern → dictionary size (distinct count).
    let count = b.fresh_value(i64_ty());
    b.push(Stmt::Let(count, Rvalue::DictEncode { base, struct_id, key_field, out_ids: Operand::Value(out_ids), out_dict: Operand::Value(out_dict) }));
    // ids length = row count; dict length = distinct count.
    let ids = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(ids, Rvalue::MakeDynArray { ptr: Operand::Value(out_ids), len: Operand::Value(len) }));
    let dict = b.fresh_value(Ty::DynArray(strs));
    b.push(Stmt::Let(dict, Rvalue::MakeDynArray { ptr: Operand::Value(out_dict), len: Operand::Value(count) }));
    // Assemble the 3-slice `dict_encoded` value.
    let enc = b.fresh_value(Ty::DictEncoded(struct_id, key_field));
    b.push(Stmt::Let(enc, Rvalue::MakeDictEncoded { source: Operand::Value(arr), ids: Operand::Value(ids), dict: Operand::Value(dict) }));
    Operand::Value(enc)
}

/// `e.group_by(.key).<op>(.value)` over a `dict_encoded` value `base` — the A2 reuse path. Extracts the
/// encoded value's three slices, gathers the chosen i64 value column out of the borrowed AoS into a
/// contiguous buffer, runs the dense-id [`Rvalue::GroupAgg`] over `(ids, vals)` (reusing the
/// precomputed interning), then labels the distinct dense ids back to `str` keys through the dictionary
/// ([`Rvalue::DictLookup`]). Builds the same result tuple `(array<str>, array<i64>)` as the A1 str-key
/// path. The gathered value column and the distinct-id scratch buffer are freed in place.
fn lower_array_group_encoded(b: &mut Builder, base: u32, struct_id: u32, value_field: Option<u32>, op: hir::GroupOp, tuple_id: u32) -> Operand {
    let strs = scalar_of(Ty::Str);
    let i64s = scalar_of(i64_ty());
    // Extract the encoded value's slices: source AoS (borrowed), ids (dense column), dict.
    let source = b.fresh_value(Ty::DynStructArray(struct_id, Layout::Aos));
    b.push(Stmt::Let(source, Rvalue::DictField { base, idx: 0 }));
    let ids = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(ids, Rvalue::DictField { base, idx: 1 }));
    let dict = b.fresh_value(Ty::DynArray(strs));
    b.push(Stmt::Let(dict, Rvalue::DictField { base, idx: 2 }));
    // n = row count = ids length.
    let n = b.fresh_value(i64_ty());
    b.push(Stmt::Let(n, Rvalue::SliceLen(Operand::Value(ids))));
    // Gather the i64 value column from the borrowed AoS into a contiguous buffer. `count` has no
    // value column → reuse the `ids` slice as the (codegen-ignored) `vals` operand and skip the gather.
    let (vals_op, vals_scratch) = match value_field {
        Some(vf) => {
            let buf = b.fresh_value(Ty::Box(i64s));
            b.push(Stmt::Let(buf, Rvalue::HeapAllocBuf { count: Operand::Value(n), elem: i64_ty() }));
            let g = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(g, Rvalue::GatherColumnI64 { source: Operand::Value(source), struct_id, field: vf, out: Operand::Value(buf) }));
            let varr = b.fresh_value(Ty::DynArray(i64s));
            b.push(Stmt::Let(varr, Rvalue::MakeDynArray { ptr: Operand::Value(buf), len: Operand::Value(n) }));
            (Operand::Value(varr), Some(varr))
        }
        None => (Operand::Value(ids), None),
    };
    // Aggregate over the dense ids → distinct ids (scratch) + per-group aggregates (kept).
    let out_ids = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_ids, Rvalue::HeapAllocBuf { count: Operand::Value(n), elem: i64_ty() }));
    let out_vals = b.fresh_value(Ty::Box(i64s));
    b.push(Stmt::Let(out_vals, Rvalue::HeapAllocBuf { count: Operand::Value(n), elem: i64_ty() }));
    let count = b.fresh_value(i64_ty());
    b.push(Stmt::Let(
        count,
        Rvalue::GroupAgg { keys: Operand::Value(ids), vals: vals_op, out_keys: Operand::Value(out_ids), out_vals: Operand::Value(out_vals), op },
    ));
    // Label the distinct dense ids back to `str` keys through the dictionary.
    let out_keys = b.fresh_value(Ty::Box(strs));
    b.push(Stmt::Let(out_keys, Rvalue::HeapAllocBuf { count: Operand::Value(count), elem: Ty::Str }));
    let lk = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(lk, Rvalue::DictLookup { ids: Operand::Value(out_ids), n: Operand::Value(count), dict: Operand::Value(dict), out: Operand::Value(out_keys) }));
    // Build the result tuple `(array<str>, array<i64>)`.
    let karr = b.fresh_value(Ty::DynArray(strs));
    b.push(Stmt::Let(karr, Rvalue::MakeDynArray { ptr: Operand::Value(out_keys), len: Operand::Value(count) }));
    let varr = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(varr, Rvalue::MakeDynArray { ptr: Operand::Value(out_vals), len: Operand::Value(count) }));
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems: vec![Operand::Value(karr), Operand::Value(varr)] }));
    // Free the transient buffers (the gathered value column + the distinct-id scratch); the result
    // owns the labels + aggregate buffers (freed by the tuple's `Drop`).
    if let Some(varr) = vals_scratch {
        b.push(Stmt::DropValue(Operand::Value(varr)));
    }
    let dense = b.fresh_value(Ty::DynArray(i64s));
    b.push(Stmt::Let(dense, Rvalue::MakeDynArray { ptr: Operand::Value(out_ids), len: Operand::Value(count) }));
    b.push(Stmt::DropValue(Operand::Value(dense)));
    Operand::Value(tup)
}

/// `source.….partition(p)` — one fused loop that splits the surviving scalar elements into two
/// owned arrays (predicate true, then false) and returns them as a tuple `(array<T>, array<T>)`.
/// Mirrors [`lower_array_collect`] but with two buffers + a per-element predicate branch at the
/// append point. Each buffer is sized at the source length (an upper bound).
fn lower_array_partition(
    b: &mut Builder,
    source: &hir::Expr,
    stages: &[hir::Stage],
    elem: Ty,
    pred_func: &str,
    pred_captures: &[hir::Expr],
    tuple_id: u32,
) -> Operand {
    let arena = b.arenas.last().copied();
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src, struct_view, temp_free } = setup_source(b, source);

    // Two output buffers, each an upper-bound `bound` elements (a split never grows the count).
    let alloc_buf = |b: &mut Builder| {
        let p = b.fresh_value(Ty::Box(scalar_of(elem)));
        let alloc = match arena {
            Some(h) => Rvalue::ArenaAlloc { handle: Operand::Value(h), count: bound.clone(), elem },
            // Unbound free-standing buffers leak if the result tuple is never destructured (same
            // bounded caveat as `to_array`); destructured into owned locals, they are freed once.
            None => Rvalue::HeapAllocBuf { count: bound.clone(), elem },
        };
        b.push(Stmt::Let(p, alloc));
        p
    };
    let out_a = alloc_buf(b);
    let out_b = alloc_buf(b);
    let acc_a = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc_a, Operand::Const(Const::Int(0, i64_ty()))));
    let acc_b = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc_b, Operand::Const(Const::Int(0, i64_ty()))));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < len
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound.clone())));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: address element i, run the stages.
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::SliceIndex(sv.clone(), index.clone())));
        Some(Operand::Value(x))
    } else if scalar_slot_src {
        let src_elem = match source.ty {
            Ty::Array(s, _) => align_sema::scalar_to_ty(s),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::Index(slot, index.clone())));
        Some(Operand::Value(x))
    } else {
        None
    };

    for stage in stages {
        match &stage.kind {
            hir::StageKind::Project { field } => {
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func, captures } => {
                let arg = match &cur {
                    Some(a) => a.clone(),
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("where with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), call_args)));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                let pred = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, Ty::Bool);
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }

    // Split: pred = p(element); true → out_a[acc_a++] = element, false → out_b[acc_b++] = element.
    let cur = cur.expect("partition needs a scalar element");
    let pred = b.fresh_value(Ty::Bool);
    let pred_args = stage_call_args(b, cur.clone(), pred_captures);
    b.push(Stmt::Let(pred, Rvalue::Call(pred_func.to_string(), pred_args)));
    let to_a = b.new_block();
    let to_b = b.new_block();
    b.terminate(Term::Branch(Operand::Value(pred), to_a, to_b));

    let append = |b: &mut Builder, buf: ValueId, acc: Slot| {
        let oi = b.fresh_value(i64_ty());
        b.push(Stmt::Let(oi, Rvalue::Load(acc)));
        b.push(Stmt::PtrStore(Operand::Value(buf), Operand::Value(oi), cur.clone()));
        let n = b.fresh_value(i64_ty());
        b.push(Stmt::Let(n, Rvalue::Bin(BinOp::Add, Operand::Value(oi), index_const(1))));
        b.push(Stmt::Store(acc, Operand::Value(n)));
        b.terminate(Term::Goto(cont));
    };
    b.cur = to_a;
    append(b, out_a, acc_a);
    b.cur = to_b;
    append(b, out_b, acc_b);

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    // exit: build the two owned arrays and the result tuple `(array<T>, array<T>)`.
    b.cur = exit;
    let la = b.fresh_value(i64_ty());
    b.push(Stmt::Let(la, Rvalue::Load(acc_a)));
    let arr_a = b.fresh_value(Ty::DynArray(scalar_of(elem)));
    b.push(Stmt::Let(arr_a, Rvalue::MakeDynArray { ptr: Operand::Value(out_a), len: Operand::Value(la) }));
    let lb = b.fresh_value(i64_ty());
    b.push(Stmt::Let(lb, Rvalue::Load(acc_b)));
    let arr_b = b.fresh_value(Ty::DynArray(scalar_of(elem)));
    b.push(Stmt::Let(arr_b, Rvalue::MakeDynArray { ptr: Operand::Value(out_b), len: Operand::Value(lb) }));
    if let Some(tmp) = temp_free {
        b.push(Stmt::DropValue(tmp));
    }
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems: vec![Operand::Value(arr_a), Operand::Value(arr_b)] }));
    Operand::Value(tup)
}

/// `source.….sort()` — materialize the surviving elements into an owned `array<T>` (the
/// `to_array` collect loop), then sort that buffer ascending in place with insertion sort.
/// Reads use `SliceIndex` over the `{ptr,len}` value; writes use `PtrStore` through its buffer
/// pointer (`SlicePtr`). Returns the same owned array. O(n²) — fine for the small arrays this
/// first cut targets; a faster sort is a follow-up.
/// A `sort_by_key` key: the per-element key function, its captures, and the key type. The
/// insertion sort compares `key(a) > key(b)` instead of `a > b`.
struct SortKey {
    func: String,
    captures: Vec<hir::Expr>,
    key_ty: Ty,
}

fn lower_array_sort(b: &mut Builder, source: &hir::Expr, stages: &[hir::Stage], elem: Ty, sort_key: Option<SortKey>) -> Operand {
    let arr = lower_array_collect(b, source, stages, elem, CollectKind::Collect);
    // Lower the key function's captures ONCE before the loop — they are loop-invariant, so
    // re-lowering them inside the per-comparison block would emit redundant loads on the hot path
    // (and LICM is not run). `key_of` reuses these pre-lowered operands.
    let lowered_captures: Vec<Operand> = match &sort_key {
        Some(sk) => sk.captures.iter().map(|c| lower_expr(b, c)).collect(),
        None => Vec::new(),
    };
    // Compute the sort key of an element value (`key(elem)` for `sort_by_key`, else the element).
    let key_of = |b: &mut Builder, v: Operand| -> Operand {
        match &sort_key {
            Some(sk) => {
                let kc = b.fresh_value(sk.key_ty);
                let mut args = Vec::with_capacity(1 + lowered_captures.len());
                args.push(v);
                args.extend(lowered_captures.iter().cloned());
                b.push(Stmt::Let(kc, Rvalue::Call(sk.func.clone(), args)));
                Operand::Value(kc)
            }
            None => v,
        }
    };
    let ptr = b.fresh_value(Ty::Box(scalar_of(elem)));
    b.push(Stmt::Let(ptr, Rvalue::SlicePtr(arr.clone())));
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(arr.clone())));

    // i = 1; while i < len { key = arr[i]; j = i-1; while j >= 0 && arr[j] > key { arr[j+1] =
    // arr[j]; j-- }; arr[j+1] = key; i++ }.
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(1, i64_ty()))));
    let jv = b.new_slot(i64_ty());

    let outer = b.new_block();
    let outer_body = b.new_block();
    let inner = b.new_block();
    let cmp_bb = b.new_block();
    let shift = b.new_block();
    let place = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(outer));

    // outer: while i < len
    b.cur = outer;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let ocond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(ocond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), Operand::Value(len))));
    b.terminate(Term::Branch(Operand::Value(ocond), outer_body, exit));

    // outer_body: key = arr[i]; j = i - 1.
    b.cur = outer_body;
    let i_cur = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_cur, Rvalue::Load(iv)));
    let key = b.fresh_value(elem);
    b.push(Stmt::Let(key, Rvalue::SliceIndex(arr.clone(), Operand::Value(i_cur))));
    // The sort key of the element being inserted (invariant across the inner loop).
    let key_cmp = key_of(b, Operand::Value(key));
    let j0 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(j0, Rvalue::Bin(BinOp::Sub, Operand::Value(i_cur), index_const(1))));
    b.push(Stmt::Store(jv, Operand::Value(j0)));
    b.terminate(Term::Goto(inner));

    // inner: while j >= 0 (then test arr[j] > key in cmp_bb).
    b.cur = inner;
    let j_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(j_val, Rvalue::Load(jv)));
    let jge0 = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(jge0, Rvalue::Bin(BinOp::Ge, Operand::Value(j_val), index_const(0))));
    b.terminate(Term::Branch(Operand::Value(jge0), cmp_bb, place));

    // cmp_bb: if arr[j] > key, shift; else place.
    b.cur = cmp_bb;
    let aj = b.fresh_value(elem);
    b.push(Stmt::Let(aj, Rvalue::SliceIndex(arr.clone(), Operand::Value(j_val))));
    // Compare keys: `key(arr[j]) > key(element)` (for a plain sort, the keys are the elements).
    let aj_cmp = key_of(b, Operand::Value(aj));
    let gt = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(gt, Rvalue::Bin(BinOp::Gt, aj_cmp, key_cmp.clone())));
    b.terminate(Term::Branch(Operand::Value(gt), shift, place));

    // shift: arr[j+1] = arr[j]; j -= 1.
    b.cur = shift;
    let jp1 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(jp1, Rvalue::Bin(BinOp::Add, Operand::Value(j_val), index_const(1))));
    b.push(Stmt::PtrStore(Operand::Value(ptr), Operand::Value(jp1), Operand::Value(aj)));
    let jdec = b.fresh_value(i64_ty());
    b.push(Stmt::Let(jdec, Rvalue::Bin(BinOp::Sub, Operand::Value(j_val), index_const(1))));
    b.push(Stmt::Store(jv, Operand::Value(jdec)));
    b.terminate(Term::Goto(inner));

    // place: arr[j+1] = key; i += 1. `jv` is unchanged between `inner` (which dominates `place`)
    // and here — only `shift` writes it, and `shift` loops back to `inner` — so `j_val` from
    // `inner` is still current; reuse it instead of re-loading.
    b.cur = place;
    let jf1 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(jf1, Rvalue::Bin(BinOp::Add, Operand::Value(j_val), index_const(1))));
    b.push(Stmt::PtrStore(Operand::Value(ptr), Operand::Value(jf1), Operand::Value(key)));
    let i_inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_inc, Rvalue::Bin(BinOp::Add, Operand::Value(i_cur), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(i_inc)));
    b.terminate(Term::Goto(outer));

    b.cur = exit;
    arr
}

/// `a.dot(b)` — the inner product `Σ a[i]*b[i]` of two fixed-length scalar arrays of equal
/// (sema-checked) length, folded in one counted loop. Both sources materialize to a slot
/// (`array_source_slot`); `mul`/`add` lower per element type (int or float).
fn lower_array_dot(b: &mut Builder, a: &hir::Expr, bex: &hir::Expr, elem: Ty) -> Operand {
    let (a_slot, n) = array_source_slot(b, a);
    let (b_slot, _nb) = array_source_slot(b, bex);

    let acc = b.new_slot(elem);
    b.push(Stmt::Store(acc, zero_of(elem)));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));
    let bound = Operand::Const(Const::Int(n, i64_ty()));

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < n
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound)));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: acc += a[i] * b[i].
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);
    let xa = b.fresh_value(elem);
    b.push(Stmt::Let(xa, Rvalue::Index(a_slot, index.clone())));
    let xb = b.fresh_value(elem);
    b.push(Stmt::Let(xb, Rvalue::Index(b_slot, index)));
    let prod = b.fresh_value(elem);
    b.push(Stmt::Let(prod, Rvalue::Bin(BinOp::Mul, Operand::Value(xa), Operand::Value(xb))));
    let a_acc = b.fresh_value(elem);
    b.push(Stmt::Let(a_acc, Rvalue::Load(acc)));
    let next = b.fresh_value(elem);
    b.push(Stmt::Let(next, Rvalue::Bin(BinOp::Add, Operand::Value(a_acc), Operand::Value(prod))));
    b.push(Stmt::Store(acc, Operand::Value(next)));
    b.terminate(Term::Goto(cont));

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    b.cur = exit;
    let r = b.fresh_value(elem);
    b.push(Stmt::Let(r, Rvalue::Load(acc)));
    Operand::Value(r)
}

/// The scalar of a known-scalar element `Ty` (panics on a non-scalar — `to_array` is
/// sema-restricted to scalar elements).
fn scalar_of(ty: Ty) -> align_sema::Scalar {
    align_sema::ty_to_scalar(ty).expect("to_array element must be a scalar (sema-checked)")
}

/// `json.decode(input)` → fill an out struct via the runtime parser (status `i32`), then
/// branch into `Ok(<struct>)` on status 0 or `Err(<code>)` otherwise, yielding the Result.
fn lower_json_decode(b: &mut Builder, struct_id: u32, input: &hir::Expr, result_ty: Ty) -> Operand {
    let sty = Ty::Struct(struct_id);
    let out = b.new_slot(sty);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecode { struct_id, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the filled struct and wrap it.
    b.cur = ok_bb;
    let s = b.fresh_value(sty);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code as the Error.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `json.decode(input)` into an owned `array<elem>` → materialize the array into an out slot via
/// the runtime parser (status `i32`), then branch into `Ok(<array>)` / `Err(<code>)`. Mirrors
/// [`lower_json_decode`]; the array is heap-owned (the unwrapped local `Drop`-frees it).
fn lower_json_decode_array(b: &mut Builder, elem: Ty, input: &hir::Expr, result_ty: Ty) -> Operand {
    let arr_ty = Ty::DynArray(scalar_of(elem));
    let out = b.new_slot(arr_ty);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecodeArray { elem, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized array {ptr,len} and wrap it (it owns its buffer now).
    b.cur = ok_bb;
    let a = b.fresh_value(arr_ty);
    b.push(Stmt::Let(a, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(a))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code (the out slot was zeroed → no buffer allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `fs.read_file(path)` → read the file into an owned `string` materialized in an out slot via
/// the runtime (status `i32`), then branch `Ok(<string>)` / `Err(<code>)`. Mirrors
/// [`lower_json_decode_array`]; the `string` is heap-owned (the unwrapped local `Drop`-frees it).
/// Wrap a runtime builtin's i32 status `code` into `Error.Code(code)` — the `err` payload of
/// `result_ty`'s `Result<_, Error>` (4b-2). The Error enum id comes from `result_ty`'s err scalar.
fn make_error_code(b: &mut Builder, code: ValueId, result_ty: Ty) -> Operand {
    let error_id = match result_ty {
        Ty::Result(_, align_sema::Scalar::Enum(eid)) => eid,
        _ => 0, // sema guarantees `Result<_, Error>` for these builtins
    };
    let ev = b.fresh_value(Ty::Enum(error_id));
    b.push(Stmt::Let(
        ev,
        Rvalue::MakeEnum { enum_id: error_id, variant: align_sema::ERROR_VARIANT_CODE, payload: vec![Operand::Value(code)] },
    ));
    Operand::Value(ev)
}

/// Decode a std runtime **errno-status** into the builtin `Error` value — the one fixed table
/// (`draft.md` §18.2), decoded from the encoding `align_rt_io_*` produces (`io_error_to_status` in
/// `align_runtime`): `1 -> NotFound` (tag 0), `2 -> Invalid` (tag 1), `3 -> Denied` (tag 2),
/// `>= 4 -> Code(status - 4)` (tag 3, the raw errno). Branchless — `tag = min(status-1, 3)`,
/// `code = max(status-4, 0)` (a `select` each) — built into the `{i32 tag, i32 code}` `Error`
/// aggregate by [`Rvalue::MakeError`].
fn make_error_from_status(b: &mut Builder, status: ValueId, result_ty: Ty) -> Operand {
    let error_id = match result_ty {
        Ty::Result(_, align_sema::Scalar::Enum(eid)) => eid,
        _ => 0, // sema guarantees `Result<_, Error>` for these builtins
    };
    let i32t = status_ty();
    let s = Operand::Value(status);
    // tag = min(status - 1, 3)
    let sm1 = b.fresh_value(i32t);
    b.push(Stmt::Let(sm1, Rvalue::Bin(BinOp::Sub, s.clone(), Operand::Const(Const::Int(1, i32t)))));
    let ge3 = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(ge3, Rvalue::Bin(BinOp::Ge, Operand::Value(sm1), Operand::Const(Const::Int(3, i32t)))));
    let tag = b.fresh_value(i32t);
    b.push(Stmt::Let(tag, Rvalue::Select { cond: Operand::Value(ge3), a: Operand::Const(Const::Int(3, i32t)), b: Operand::Value(sm1) }));
    // code = max(status - 4, 0)
    let sm4 = b.fresh_value(i32t);
    b.push(Stmt::Let(sm4, Rvalue::Bin(BinOp::Sub, s, Operand::Const(Const::Int(4, i32t)))));
    let ge0 = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(ge0, Rvalue::Bin(BinOp::Ge, Operand::Value(sm4), Operand::Const(Const::Int(0, i32t)))));
    let code = b.fresh_value(i32t);
    b.push(Stmt::Let(code, Rvalue::Select { cond: Operand::Value(ge0), a: Operand::Value(sm4), b: Operand::Const(Const::Int(0, i32t)) }));
    let ev = b.fresh_value(Ty::Enum(error_id));
    b.push(Stmt::Let(ev, Rvalue::MakeError { enum_id: error_id, tag: Operand::Value(tag), code: Operand::Value(code) }));
    Operand::Value(ev)
}

fn lower_fs_read_file(b: &mut Builder, path: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::String);
    let p = lower_expr(b, path);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::FsReadFile { path: p, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized string {ptr,len} and wrap it (it owns its buffer now).
    b.cur = ok_bb;
    let s = b.fresh_value(Ty::String);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: map the runtime errno-status to the builtin `Error` (the out slot was zeroed → no buffer
    // allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `env.get(name)` → `Option<string>`: the runtime writes the owned value `{ptr,len}` into an out
/// slot and returns an `i32` present flag; branch `Some(<value>)` (flag != 0) / `None` (flag == 0).
/// Mirrors [`lower_fs_read_file`]'s out-slot shape, building an `Option` (not a `Result`).
fn lower_env_get(b: &mut Builder, name: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::String);
    let n = lower_expr(b, name);
    let flag = b.fresh_value(status_ty());
    b.push(Stmt::Let(flag, Rvalue::EnvGet { name: n, out }));

    let present = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(present, Rvalue::Bin(BinOp::Ne, Operand::Value(flag), Operand::Const(Const::Int(0, status_ty())))));
    let some_bb = b.new_block();
    let none_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(present), some_bb, none_bb));

    // Some: load the materialized owned string `{ptr,len}` (it owns its buffer now) and wrap it.
    b.cur = some_bb;
    let s = b.fresh_value(Ty::String);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let somev = b.fresh_value(result_ty);
    b.push(Stmt::Let(somev, Rvalue::OptionSome(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(somev)));
    b.terminate(Term::Goto(join));

    // None: the out slot was zeroed (`{null,0}`) → nothing to free.
    b.cur = none_bb;
    let nonev = b.fresh_value(result_ty);
    b.push(Stmt::Let(nonev, Rvalue::OptionNone));
    b.push(Stmt::Store(rslot, Operand::Value(nonev)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `fs.read_dir(path)` → the runtime writes the owned `array<string>` `{ptr,len}` into an out slot
/// and returns an errno-status; branch `Ok(<array>)` / `Err(<mapped status>)`. Mirrors
/// [`lower_fs_read_file`] with a `DynArray(String)` payload instead of a `string`.
fn lower_fs_read_dir(b: &mut Builder, path: &hir::Expr, result_ty: Ty) -> Operand {
    let arr_ty = Ty::DynArray(align_sema::Scalar::String);
    let out = b.new_slot(arr_ty);
    let p = lower_expr(b, path);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::FsReadDir { path: p, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized `{ptr,len}` owned array and wrap it (it owns its buffers now).
    b.cur = ok_bb;
    let a = b.fresh_value(arr_ty);
    b.push(Stmt::Let(a, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(a))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (`{null,0}`) → nothing to free; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `dns.resolve(host)` → the runtime writes the owned `array<string>` `{ptr,len}` into an out slot
/// and returns a status; branch `Ok(<array>)` / `Err(<mapped status>)`. Identical to
/// [`lower_fs_read_dir`] except for the runtime call — same `DynArray(String)` payload + deep `Drop`.
fn lower_dns_resolve(b: &mut Builder, host: &hir::Expr, result_ty: Ty) -> Operand {
    let arr_ty = Ty::DynArray(align_sema::Scalar::String);
    let out = b.new_slot(arr_ty);
    let h = lower_expr(b, host);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::DnsResolve { host: h, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized `{ptr,len}` owned array and wrap it (it owns its buffers now).
    b.cur = ok_bb;
    let a = b.fresh_value(arr_ty);
    b.push(Stmt::Let(a, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(a))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (`{null,0}`) → nothing to free; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `tcp.connect(host, port)` → the runtime resolves `host`, connects to `port`, and writes the
/// owned `tcp_conn` handle into an out slot, returning a status; branch `Ok(<conn>)` /
/// `Err(<mapped status>)`. Mirrors [`lower_open_handle`] with a second `port` operand and a
/// `tcp_conn` handle payload.
fn lower_tcp_connect(b: &mut Builder, host: &hir::Expr, port: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::TcpConn);
    let h = lower_expr(b, host);
    let p = lower_expr(b, port);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::TcpConnect { host: h, port: p, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the conn handle and wrap it (the unwrapped local owns it — `Drop` closes its fd).
    b.cur = ok_bb;
    let c = b.fresh_value(Ty::TcpConn);
    b.push(Stmt::Let(c, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(c))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to close; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `tcp.listen(host, port)` → the runtime binds a listening socket to `port`, writing the owned
/// `tcp_listener` handle into an out slot and returning a status; branch `Ok(<listener>)` /
/// `Err(<mapped status>)`. Mirrors [`lower_tcp_connect`] with a `tcp_listener` handle payload.
fn lower_tcp_listen(b: &mut Builder, host: &hir::Expr, port: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::TcpListener);
    let h = lower_expr(b, host);
    let p = lower_expr(b, port);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::TcpListen { host: h, port: p, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the listener handle and wrap it (the unwrapped local owns it — `Drop` closes its fd).
    b.cur = ok_bb;
    let l = b.fresh_value(Ty::TcpListener);
    b.push(Stmt::Let(l, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(l))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to close; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `l.accept()` → the runtime blocks for an inbound connection on the listener, writing the new owned
/// `tcp_conn` handle into an out slot and returning a status; branch `Ok(<conn>)` / `Err(<mapped
/// status>)`. Mirrors [`lower_tcp_connect`] but with a listener operand (the receiver) rather than
/// host/port. The accepted `tcp_conn` is freshly owned — its `Drop` closes its fd.
fn lower_tcp_accept(b: &mut Builder, listener: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::TcpConn);
    let l = lower_expr(b, listener);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::TcpAccept { listener: l, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the accepted conn handle and wrap it (the unwrapped local owns it — `Drop` closes fd).
    b.cur = ok_bb;
    let c = b.fresh_value(Ty::TcpConn);
    b.push(Stmt::Let(c, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(c))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to close; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `udp.bind(host, port)` → the runtime opens a bound `SOCK_DGRAM` socket, writing the owned
/// `udp_socket` handle into an out slot and returning a status; branch `Ok(<socket>)` / `Err(<mapped
/// status>)`. Mirrors [`lower_tcp_listen`] with a `udp_socket` handle payload.
fn lower_udp_bind(b: &mut Builder, host: &hir::Expr, port: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::UdpSocket);
    let h = lower_expr(b, host);
    let p = lower_expr(b, port);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::UdpBind { host: h, port: p, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the socket handle and wrap it (the unwrapped local owns it — `Drop` closes its fd).
    b.cur = ok_bb;
    let s = b.fresh_value(Ty::UdpSocket);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to close; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `u.send_to(data, host, port)` → the runtime `sendto`s one datagram from the socket's fd, returning
/// the bytes sent (`>= 0`) or `-(status)`; wrap into `Result<i64, Error>` via the shared count/status
/// helper (the `reader.read` sign convention).
fn lower_udp_send_to(b: &mut Builder, sock: &hir::Expr, data: &hir::Expr, host: &hir::Expr, port: &hir::Expr, result_ty: Ty) -> Operand {
    let s = lower_expr(b, sock);
    let d = lower_expr(b, data);
    let h = lower_expr(b, host);
    let p = lower_expr(b, port);
    let n = b.fresh_value(i64_ty());
    b.push(Stmt::Let(n, Rvalue::UdpSendTo { sock: s, data: d, host: h, port: p }));
    lower_count_or_status_result(b, n, result_ty)
}

/// `u.recv_from(buf)` → the runtime blocks for one datagram, filling `buf` and returning the bytes
/// received (`>= 0`) or `-(status)`; wrap into `Result<i64, Error>` via the shared count/status
/// helper (the `reader.read` sign convention).
fn lower_udp_recv_from(b: &mut Builder, sock: &hir::Expr, buffer: &hir::Expr, result_ty: Ty) -> Operand {
    let s = lower_expr(b, sock);
    let buf = lower_expr(b, buffer);
    let n = b.fresh_value(i64_ty());
    b.push(Stmt::Let(n, Rvalue::UdpRecvFrom { sock: s, buffer: buf }));
    lower_count_or_status_result(b, n, result_ty)
}

/// `process.spawn(cmd, args)` → the runtime `fork`s + `execvp`s, writing the owned `child` handle into
/// an out slot and returning an errno-status; branch `Ok(<child>)` / `Err(<mapped status>)`. Mirrors
/// [`lower_udp_bind`] with a `child` handle payload (the unwrapped local owns it — `Drop` reaps it).
fn lower_process_spawn(b: &mut Builder, cmd: &hir::Expr, args: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::Child);
    let c = lower_expr(b, cmd);
    let a = lower_expr(b, args);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::ProcessSpawn { cmd: c, args: a, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the child handle and wrap it (the unwrapped local owns it — `Drop` reaps its pid).
    b.cur = ok_bb;
    let ch = b.fresh_value(Ty::Child);
    b.push(Stmt::Let(ch, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(ch))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to reap; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `ch.wait()` → the runtime `waitpid`s (marking the child reaped through the borrow), returning the
/// exit code (`>= 0`) or `-(status)`; wrap into `Result<i64, Error>` via the shared count/status
/// helper (the `reader.read` sign convention). `child` is borrowed (never consumed — no move-out).
fn lower_child_wait(b: &mut Builder, child: &hir::Expr, result_ty: Ty) -> Operand {
    let ch = lower_expr(b, child);
    let n = b.fresh_value(i64_ty());
    b.push(Stmt::Let(n, Rvalue::ChildWait { child: ch }));
    lower_count_or_status_result(b, n, result_ty)
}

/// `ch.kill(sig)` → the runtime `kill(pid, sig)`s (guarding a reaped/recycled pid through the borrow),
/// returning an `i32` errno-status; wrap into `Result<(), Error>` via the shared status helper. `child`
/// is borrowed (no move-out); `sig` is a scalar `i64`.
fn lower_child_kill(b: &mut Builder, child: &hir::Expr, sig: &hir::Expr, result_ty: Ty) -> Operand {
    let ch = lower_expr(b, child);
    let s = lower_expr(b, sig);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::ChildKill { child: ch, sig: s }));
    lower_status_result(b, code, result_ty)
}

/// `process.exec(cmd, args)` → the runtime `execvp`s in place. On **success it replaces the image and
/// never returns**, so control falls through to the status check only on failure — the runtime returns
/// the mapped errno, which the shared status helper wraps into the `Err` arm of `Result<(), Error>`
/// (the only observable arm). No cleanup is emitted (unlike `process.exit`): `execvp` discards the
/// address space, so pending `Drop`s / arena ends / buffered writers are inherently lost on success —
/// this is the settled abort-class treatment (`docs/impl/std-design/process.md`).
fn lower_process_exec(b: &mut Builder, cmd: &hir::Expr, args: &hir::Expr, result_ty: Ty) -> Operand {
    let c = lower_expr(b, cmd);
    let a = lower_expr(b, args);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::ProcessExec { cmd: c, args: a }));
    lower_status_result(b, code, result_ty)
}

/// `fs.read_file_view(path)` → the runtime mmaps the file into the enclosing arena, writing the
/// `str` view `{ptr,len}` into an out slot and returning an errno-status; branch `Ok(<view>)` /
/// `Err(<mapped status>)`. The arena handle (guaranteed present — sema requires an enclosing arena)
/// is threaded so the runtime registers the mapping for `munmap` at arena end. Mirrors
/// [`lower_fs_read_file`] with a `str` view payload (no `Drop` — it borrows the arena).
fn lower_fs_read_file_view(b: &mut Builder, path: &hir::Expr, result_ty: Ty) -> Operand {
    let arena = *b.arenas.last().expect("read_file_view outside an arena (sema-checked)");
    let out = b.new_slot(Ty::Str);
    let p = lower_expr(b, path);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::FsReadFileView { path: p, arena: Operand::Value(arena), out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the mapped `str` view and wrap it (arena-owned — no `Drop`).
    b.cur = ok_bb;
    let s = b.fresh_value(Ty::Str);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (`{null,0}`); map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `fs.read_bytes_view(path)` → the binary sibling of [`lower_fs_read_file_view`]: the runtime
/// mmaps the file into the enclosing arena and writes the `bytes` (`slice<u8>`) view `{ptr,len}`
/// into an out slot, returning an errno-status; branch `Ok(<view>)` / `Err(<mapped status>)`. Same
/// shape as the `str` view (identical `{ptr,len}` payload, no `Drop` — it borrows the arena); the
/// only difference is the runtime skips UTF-8 validation, so binary content is accepted.
fn lower_fs_read_bytes_view(b: &mut Builder, path: &hir::Expr, result_ty: Ty) -> Operand {
    let arena = *b.arenas.last().expect("read_bytes_view outside an arena (sema-checked)");
    let elem = align_sema::Scalar::Int(IntTy { bits: 8, signed: false });
    let out = b.new_slot(Ty::Slice(elem));
    let p = lower_expr(b, path);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::FsReadBytesView { path: p, arena: Operand::Value(arena), out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the mapped `slice<u8>` view and wrap it (arena-owned — no `Drop`).
    b.cur = ok_bb;
    let s = b.fresh_value(Ty::Slice(elem));
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (`{null,0}`); map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `fs.open` / `fs.create` → the runtime writes the owned `reader`/`writer` handle into an out slot
/// (of `handle_ty`) and returns an errno-status; branch `Ok(<handle>)` / `Err(<mapped status>)`.
/// Mirrors [`lower_fs_read_file`] with a handle payload instead of a `string`.
fn lower_open_handle(
    b: &mut Builder,
    path: &hir::Expr,
    handle_ty: Ty,
    result_ty: Ty,
    open_rv: impl FnOnce(Operand, Slot) -> Rvalue,
) -> Operand {
    let out = b.new_slot(handle_ty);
    let p = lower_expr(b, path);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, open_rv(p, out)));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the handle pointer and wrap it (the unwrapped local owns it — `Drop` closes it).
    b.cur = ok_bb;
    let h = b.fresh_value(handle_ty);
    b.push(Stmt::Let(h, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(h))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to close; map the status.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `encoding.*_decode(s)` → the runtime writes an owned `buffer` handle into an out slot and
/// returns an `i32` status (0 = ok; `AL_INVALID` -> `Error.Invalid`). Branch `Ok(<buffer>)` /
/// `Err(<mapped status>)`. Mirrors [`lower_open_handle`], but the source is a `str` **view**
/// (not a path) and there is no arena. The wrapped buffer is owned — the unwrapped local `Drop`s it.
fn lower_encoding_decode(b: &mut Builder, kind: hir::EncodingKind, input: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::Buffer);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::EncodingDecode { kind, input: inp, out }));
    emit_status_buffer_result(b, code, out, result_ty)
}

/// Shared tail for a runtime op that writes an owned `buffer` handle into `out` and returns an i32
/// `status` (0 = ok; `AL_INVALID` -> `Error.Invalid`; `>= AL_CODE` -> `Error.Code`): branch the
/// already-emitted `code` into `Ok(<buffer>)` / `Err(<mapped status>)` of `result_ty`. The `out`
/// slot must have been caller-zeroed by codegen so the `Err` path (null handle) frees nothing.
/// Shared by `encoding.*_decode` and the `std.compress` codecs.
fn emit_status_buffer_result(b: &mut Builder, code: ValueId, out: Slot, result_ty: Ty) -> Operand {
    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the buffer handle and wrap it (the unwrapped local owns it — `Drop` frees it).
    b.cur = ok_bb;
    let h = b.fresh_value(Ty::Buffer);
    b.push(Stmt::Let(h, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(h))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to free; map the status (`Error.Invalid`).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `crypto.hmac_sha256(key, data)` → a fresh owned `array<u8>` `{ptr,len}` returned by value (the
/// bound local `Drop`-frees it, same shape as `crypto.sha256`). Out-of-line (`#[inline(never)]`) so
/// its locals stay off the recursive `lower_expr` frame (see the call site).
#[inline(never)]
fn lower_crypto_hmac(b: &mut Builder, key: &hir::Expr, data: &hir::Expr, ty: Ty) -> Operand {
    let kv = lower_expr(b, key);
    let dv = lower_expr(b, data);
    let v = b.fresh_value(ty);
    b.push(Stmt::Let(v, Rvalue::CryptoHmac { key: kv, data: dv }));
    Operand::Value(v)
}

/// `crypto.hkdf_sha256(salt, ikm, info, len)` → the runtime writes an owned `buffer` into `out` +
/// returns an i32 status; branch `Ok(<buffer>)` / `Err(<mapped>)` via the shared `std.compress`
/// machinery. Out-of-line (`#[inline(never)]`) so its locals stay off the recursive `lower_expr`
/// frame (see the call site).
#[inline(never)]
fn lower_crypto_hkdf(b: &mut Builder, salt: &hir::Expr, ikm: &hir::Expr, info: &hir::Expr, len: &hir::Expr, ty: Ty) -> Operand {
    let out = b.new_slot(Ty::Buffer);
    let sv = lower_expr(b, salt);
    let iv = lower_expr(b, ikm);
    let nv = lower_expr(b, info);
    let lv = lower_expr(b, len);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::CryptoHkdf { salt: sv, ikm: iv, info: nv, len: lv, out }));
    emit_status_buffer_result(b, code, out, ty)
}

/// `crypto.{aes_gcm,chacha20_poly1305}_{seal,open}(key, nonce, input, aad)` → the runtime writes an
/// owned `buffer` into `out` + returns an i32 status; branch `Ok(<buffer>)` / `Err(<mapped>)` via the
/// shared `std.compress` machinery. Out-of-line (`#[inline(never)]`) so its locals stay off the
/// recursive `lower_expr` frame (see the call site — the #296 `expr_depth` headroom).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn lower_crypto_aead(
    b: &mut Builder,
    cipher: hir::AeadCipher,
    dir: hir::AeadDir,
    key: &hir::Expr,
    nonce: &hir::Expr,
    input: &hir::Expr,
    aad: &hir::Expr,
    ty: Ty,
) -> Operand {
    let out = b.new_slot(Ty::Buffer);
    let kv = lower_expr(b, key);
    let nv = lower_expr(b, nonce);
    let iv = lower_expr(b, input);
    let av = lower_expr(b, aad);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::CryptoAead { cipher, dir, key: kv, nonce: nv, input: iv, aad: av, out }));
    emit_status_buffer_result(b, code, out, ty)
}

/// `crypto.argon2id(password, salt, params)` → the runtime writes an owned `buffer` into `out` +
/// returns an i32 status; branch `Ok(<buffer>)` / `Err(<mapped>)` via the shared `std.compress`
/// machinery. The `argon2_params` struct is materialized into a temp slot (works for a struct literal
/// *or* a variable) and its four `i64` fields are read out in declaration order (`m_cost`, `t_cost`,
/// `parallelism`, `len` — the `StructDef` order; all `i64`, so layout order == declaration order),
/// then passed to the runtime as flat scalars. Out-of-line (`#[inline(never)]`) so its locals stay
/// off the recursive `lower_expr` frame (see the call site — the #296 `expr_depth` headroom).
#[inline(never)]
fn lower_crypto_argon2(b: &mut Builder, password: &hir::Expr, salt: &hir::Expr, params: &hir::Expr, ty: Ty) -> Operand {
    let out = b.new_slot(Ty::Buffer);
    let pw = lower_expr(b, password);
    let sv = lower_expr(b, salt);
    // Materialize the `argon2_params` struct into a slot, then read its four `i64` fields.
    let pslot = b.new_slot(params.ty);
    store_value_at(b, pslot, &mut Vec::new(), params);
    let read_field = |b: &mut Builder, idx: u32| {
        let v = b.fresh_value(i64_ty());
        b.push(Stmt::Let(v, Rvalue::Field(pslot, vec![idx])));
        Operand::Value(v)
    };
    let m_cost = read_field(b, 0);
    let t_cost = read_field(b, 1);
    let parallelism = read_field(b, 2);
    let len = read_field(b, 3);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(
        code,
        Rvalue::CryptoArgon2(Box::new(Argon2Args { password: pw, salt: sv, m_cost, t_cost, parallelism, len, out })),
    ));
    emit_status_buffer_result(b, code, out, ty)
}

/// `c.parse(args)` → the runtime writes an owned `cli parsed` handle into an out slot and returns an
/// `i32` status (0 = ok; `AL_INVALID` -> `Error.Invalid`). Branch `Ok(<parsed>)` / `Err(<mapped
/// status>)`. Mirrors [`lower_encoding_decode`], but the source is the command handle + the argv
/// `array<str>`. The wrapped `parsed` is owned — the unwrapped local `Drop`s it (`cli_parsed_free`).
fn lower_cli_parse(b: &mut Builder, cmd: &hir::Expr, args: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::CliParsed);
    let cop = lower_expr(b, cmd);
    let av = lower_expr(b, args);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::CliParse { cmd: cop, args: av, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the parsed handle and wrap it (the unwrapped local owns it — `Drop` frees it).
    b.cur = ok_bb;
    let h = b.fresh_value(Ty::CliParsed);
    b.push(Stmt::Let(h, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(h))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to free; map the status (`Error.Invalid`).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// Lower any `std.http` (Slice 1) request/response op. Out-of-line (`#[inline(never)]`) and reached
/// through a single `lower_expr` arm, so all its locals — and the seven ops' distinct shapes — stay
/// off the recursive `lower_expr` frame (the `expr_depth` #296 headroom lesson). Never called for a
/// non-http `e`.
#[inline(never)]
fn lower_http(b: &mut Builder, e: &hir::Expr) -> Operand {
    match &e.kind {
        // `http.request(method, url)` → an owned `http request` handle returned by value (the bound
        // local `Drop`-frees it via `http_request_free`).
        hir::ExprKind::HttpRequest { method, url } => {
            let m = lower_expr(b, method);
            let u = lower_expr(b, url);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::HttpRequest { method: m, url: u }));
            Operand::Value(v)
        }
        // `r.header(name, value)` → append a header to the request handle in place; no value.
        hir::ExprKind::HttpHeader { req, name, value } => {
            let rq = lower_expr(b, req);
            let nm = lower_expr(b, name);
            let vl = lower_expr(b, value);
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::HttpHeader { req: rq, name: nm, value: vl }));
            Operand::Const(Const::Unit)
        }
        // `r.body(data)` → copy the byte view into the request handle's body; no value.
        hir::ExprKind::HttpBody { req, data } => {
            let rq = lower_expr(b, req);
            let d = lower_expr(b, data);
            let v = b.fresh_value(Ty::Unit);
            b.push(Stmt::Let(v, Rvalue::HttpBody { req: rq, data: d }));
            Operand::Const(Const::Unit)
        }
        // `http.parse(data)` → `Result<response, Error>` (out-slot + i32 status; see below).
        hir::ExprKind::HttpParse { data } => lower_http_parse(b, data, e.ty),
        // `resp.status()` → the runtime returns the i64 status directly.
        hir::ExprKind::HttpRespStatus { resp } => {
            let rp = lower_expr(b, resp);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::HttpRespStatus { resp: rp }));
            Operand::Value(v)
        }
        // `resp.header(name)` → `Option<str>` (out-slot + i32 present flag; see below).
        hir::ExprKind::HttpRespHeader { resp, name } => lower_http_resp_header(b, resp, name, e.ty),
        // `resp.body()` → a `slice<u8>` view `{ptr,len}` into the response buffer (region-bound to
        // `resp`; not owned — no `Drop`).
        hir::ExprKind::HttpRespBody { resp } => {
            let rp = lower_expr(b, resp);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::HttpRespBody { resp: rp }));
            Operand::Value(v)
        }
        // `http.client()` → an owned `http client` handle returned by value (the bound local
        // `Drop`-frees it via `http_client_free`).
        hir::ExprKind::HttpClient => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::HttpClient));
            Operand::Value(v)
        }
        // `cl.get(url)` → `Result<response, Error>` (shared out-slot + i32-status lowering). `client`
        // is borrowed (read, not consumed); `url` is a `str` view.
        hir::ExprKind::HttpClientGet { client, url } => {
            let out = b.new_slot(Ty::HttpResponse);
            let c = lower_expr(b, client);
            let u = lower_expr(b, url);
            lower_http_response_result(b, Rvalue::HttpClientGet { client: c, url: u, out }, out, e.ty)
        }
        // `cl.post(url, body)` → `Result<response, Error>`. `url`/`body` are byte views.
        hir::ExprKind::HttpClientPost { client, url, body } => {
            let out = b.new_slot(Ty::HttpResponse);
            let c = lower_expr(b, client);
            let u = lower_expr(b, url);
            let bd = lower_expr(b, body);
            lower_http_response_result(b, Rvalue::HttpClientPost { client: c, url: u, body: bd, out }, out, e.ty)
        }
        // `cl.request(req)` → `Result<response, Error>`. `req` is a Move `http request` **consumed** by
        // the call (the runtime frees it): null its source slot so the exit `Drop` doesn't double-free.
        hir::ExprKind::HttpClientRequest { client, req } => {
            let out = b.new_slot(Ty::HttpResponse);
            let c = lower_expr(b, client);
            let rq = lower_expr(b, req);
            null_moved_source(b, req);
            lower_http_response_result(b, Rvalue::HttpClientRequest { client: c, req: rq, out }, out, e.ty)
        }
        _ => unreachable!("lower_http on a non-http expr"),
    }
}

/// `http.parse(data)` → `Result<response, Error>` via the shared out-slot + i32-status lowering.
fn lower_http_parse(b: &mut Builder, data: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::HttpResponse);
    let d = lower_expr(b, data);
    lower_http_response_result(b, Rvalue::HttpParse { data: d, out }, out, result_ty)
}

/// The shared Ok/Err lowering for the four ops that write an owned `http response` into `out` and
/// return an `i32` status (`code_rv` is the runtime-call Rvalue, its `out` slot already set to `out`):
/// `http.parse` and the Slice-2 client `get`/`post`/`request`. `0` = ok (a 4xx/5xx status is STILL ok
/// — status is data, http.md P2); else `AL_INVALID` -> `Error.Invalid` / an errno -> `Error`. Branches
/// `Ok(<response>)` (the unwrapped local owns it — `Drop`s via `http_resp_free`) / `Err(<mapped
/// status>)`. Out-of-line (`#[inline(never)]`) so its block/slot locals stay off the recursive
/// `lower_expr` frame (the `expr_depth` #296 lesson).
#[inline(never)]
fn lower_http_response_result(b: &mut Builder, code_rv: Rvalue, out: Slot, result_ty: Ty) -> Operand {
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, code_rv));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the response handle and wrap it (the unwrapped local owns it — `Drop` frees it).
    b.cur = ok_bb;
    let h = b.fresh_value(Ty::HttpResponse);
    b.push(Stmt::Let(h, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(h))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the out slot was zeroed (null handle) → nothing to free; map the status (`Error.Invalid`).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `resp.header(name)` → the runtime writes a `str` view (`{ptr,len}`) into an out slot and returns
/// an `i32` present flag; branch `Some(<view>)` / `None`. The view borrows `resp` (region-bound in
/// sema). Mirrors [`lower_env_get`], but the payload is a borrowed `str` view (not an owned string —
/// the None arm's zeroed out slot needs no free either way). Out-of-line for `expr_depth` headroom.
#[inline(never)]
fn lower_http_resp_header(b: &mut Builder, resp: &hir::Expr, name: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::Str);
    let rp = lower_expr(b, resp);
    let nm = lower_expr(b, name);
    let flag = b.fresh_value(status_ty());
    b.push(Stmt::Let(flag, Rvalue::HttpRespHeader { resp: rp, name: nm, out }));

    let present = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(present, Rvalue::Bin(BinOp::Ne, Operand::Value(flag), Operand::Const(Const::Int(0, status_ty())))));
    let some_bb = b.new_block();
    let none_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(present), some_bb, none_bb));

    // Some: load the `str` view and wrap it.
    b.cur = some_bb;
    let s = b.fresh_value(Ty::Str);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let somev = b.fresh_value(result_ty);
    b.push(Stmt::Let(somev, Rvalue::OptionSome(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(somev)));
    b.terminate(Term::Goto(join));

    b.cur = none_bb;
    let nonev = b.fresh_value(result_ty);
    b.push(Stmt::Let(nonev, Rvalue::OptionNone));
    b.push(Stmt::Store(rslot, Operand::Value(nonev)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `r.read(b)` → the runtime returns an `i64`: bytes read (`>= 0`, `0` = EOF) or `-(status)` on
/// error. Branch `Ok(<count>)` / `Err(<mapped status>)` on the sign.
fn lower_reader_read(b: &mut Builder, reader: Operand, buffer: Operand, result_ty: Ty) -> Operand {
    let n = b.fresh_value(i64_ty());
    b.push(Stmt::Let(n, Rvalue::ReaderRead(reader, buffer)));
    lower_count_or_status_result(b, n, result_ty)
}

/// Wrap a runtime `i64` that encodes either a non-negative **count** (`Ok`) or `-(status)` on
/// error (`Err`, the errno mapped through the fixed table) into `Result<i64, Error>`. Shared by
/// `reader.read` and `io.copy` (identical sign convention).
fn lower_count_or_status_result(b: &mut Builder, n: ValueId, result_ty: Ty) -> Operand {
    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Ge, Operand::Value(n), Operand::Const(Const::Int(0, i64_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: the count `n`.
    b.cur = ok_bb;
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(n))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: the status is `-n`, narrowed to i32 for the fixed-table decode.
    b.cur = err_bb;
    let neg = b.fresh_value(i64_ty());
    b.push(Stmt::Let(neg, Rvalue::Bin(BinOp::Sub, Operand::Const(Const::Int(0, i64_ty())), Operand::Value(n))));
    let status = b.fresh_value(status_ty());
    b.push(Stmt::Let(status, Rvalue::Cast { operand: Operand::Value(neg), from: i64_ty(), to: status_ty() }));
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, status, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// Wrap a precomputed i32 errno-status `code` into `Result<(), Error>`: `Ok(())` on `0`, else
/// `Err(<mapped status>)`. The `writer.write` / `writer.flush` tail.
fn lower_status_result(b: &mut Builder, code: ValueId, result_ty: Ty) -> Operand {
    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    b.cur = ok_bb;
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Const(Const::Unit))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_from_status(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `json.decode(input)` into an owned `array<Struct>` (MMv2 slice 8d) → materialize the AoS into
/// an out slot via the runtime parser (status `i32`), then branch `Ok(<array>)` / `Err(<code>)`.
/// Mirrors [`lower_json_decode_array`]; the AoS buffer is heap-owned (the unwrapped local
/// `Drop`-frees it), while its elements' `str` fields remain views into the input.
fn lower_json_decode_struct_array(b: &mut Builder, struct_id: u32, input: &hir::Expr, result_ty: Ty) -> Operand {
    let arr_ty = Ty::DynStructArray(struct_id, Layout::Aos);
    let out = b.new_slot(arr_ty);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecodeStructArray { struct_id, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized array {ptr,len} and wrap it (it owns its buffer now).
    b.cur = ok_bb;
    let a = b.fresh_value(arr_ty);
    b.push(Stmt::Let(a, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(a))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code (the out slot was zeroed → no buffer allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `expr?` → branch on the Result tag. `Err` propagates (early-return an `Err` of the
/// function's own return type — the cold edge); `Ok` continues with the unwrapped value.
fn lower_try(b: &mut Builder, inner: &hir::Expr, ok_ty: Ty) -> Operand {
    let ret_err_ty = match b.ret {
        Ty::Result(_, e) => align_sema::scalar_to_ty(e),
        _ => Ty::Error,
    };
    let r = lower_expr(b, inner);

    let is_ok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_ok, Rvalue::ResultIsOk(r.clone())));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    // NOTE: the Err edge is the designed "cold" path, but this is a plain branch — LLVM
    // branch-weight / cold metadata is not emitted yet (a later codegen optimization).
    b.terminate(Term::Branch(Operand::Value(is_ok), ok_bb, err_bb));

    // Err: extract the error and early-return Err(err) of the function's return type.
    b.cur = err_bb;
    let err = b.fresh_value(ret_err_ty);
    b.push(Stmt::Let(err, Rvalue::ResultUnwrapErr(r.clone())));
    let propagated = b.fresh_value(b.ret);
    b.push(Stmt::Let(propagated, Rvalue::ResultErr(Operand::Value(err))));
    // `?` exits the function: free open arenas and drop owned locals first.
    b.emit_exit_cleanup();
    b.terminate(Term::Return(Some(Operand::Value(propagated))));

    // Ok: continue with the unwrapped value. If the operand was a bound local holding an owned
    // payload (e.g. `r: Result<string,E>`), the payload is now moved into `v`, so null the source
    // slot — its exit `Drop` then frees null, not the moved-out buffer (no double-free). On the
    // Err edge the source's ok payload is already {null,0} (zeroed at construction), so the
    // exit-cleanup drop there is a harmless no-op.
    b.cur = ok_bb;
    let v = b.fresh_value(ok_ty);
    b.push(Stmt::Let(v, Rvalue::ResultUnwrapOk(r)));
    null_moved_source(b, inner);
    Operand::Value(v)
}

/// `opt else fallback` → branch on the Option tag; `Some` unwraps the payload into the
/// result slot, `None` evaluates the fallback (which writes the slot or diverges).
fn lower_else_unwrap(b: &mut Builder, opt: &hir::Expr, fallback: &hir::Expr, ty: Ty) -> Operand {
    // `else` unwraps an `Option` (Some/None) or a `Result` (Ok/Err) — the same two-way shape, just
    // a different discriminant/unwrap rvalue. The `Err` case discards the error; sema restricts the
    // error to a Copy scalar, so there is nothing to drop on the fallback path (a bound-local
    // scrutinee with a Move *Ok* payload is nulled below, exactly like `Option<string>`).
    let is_result = matches!(opt.ty, Ty::Result(..));
    let result_slot = b.new_slot(ty);
    let opt_op = lower_expr(b, opt);

    let is_pos = b.fresh_value(Ty::Bool);
    let test = if is_result { Rvalue::ResultIsOk(opt_op.clone()) } else { Rvalue::OptionIsSome(opt_op.clone()) };
    b.push(Stmt::Let(is_pos, test));
    let some_bb = b.new_block();
    let none_bb = b.new_block();
    let join_bb = b.new_block();
    b.terminate(Term::Branch(Operand::Value(is_pos), some_bb, none_bb));

    // Some/Ok: unwrap the payload into the result slot. If the source was a bound local with an
    // owned payload (`opt: Option<string>` / `Result<string, Error>`), null it — the payload moved
    // into the result slot, so its exit `Drop` must free null (the None/Err edge already has a
    // {null,0} payload).
    b.cur = some_bb;
    let val = b.fresh_value(ty);
    let unwrap = if is_result { Rvalue::ResultUnwrapOk(opt_op) } else { Rvalue::OptionUnwrap(opt_op) };
    b.push(Stmt::Let(val, unwrap));
    b.push(Stmt::Store(result_slot, Operand::Value(val)));
    null_moved_source(b, opt);
    b.terminate(Term::Goto(join_bb));

    // None/Err: the fallback yields the value, or diverges (then the block is already
    // terminated and the store/goto are skipped).
    b.cur = none_bb;
    let fb = lower_expr(b, fallback);
    if !b.is_terminated() {
        b.push(Stmt::Store(result_slot, fb));
        b.terminate(Term::Goto(join_bb));
    }

    b.cur = join_bb;
    let r = b.fresh_value(ty);
    b.push(Stmt::Let(r, Rvalue::Load(result_slot)));
    Operand::Value(r)
}

/// `match scrutinee { … }`: lower per scrutinee kind — a user `enum` (a tag-compare chain over the
/// non-union struct) or builtin `Option`/`Result` (a single 2-way branch on `IsSome`/`IsOk`).
fn lower_match(b: &mut Builder, scrutinee: &hir::Expr, arms: &[hir::MatchArm], ty: Ty) -> Operand {
    // A zero-arm `match` is already a (non-exhaustive) sema error; lower the scrutinee for its
    // effects and yield unit so we never panic on the indexing below.
    if arms.is_empty() {
        lower_expr(b, scrutinee);
        return Operand::Const(Const::Unit);
    }
    let result_slot = (ty != Ty::Unit).then(|| b.new_slot(ty));
    let scrut = lower_expr(b, scrutinee);
    let join_bb = b.new_block();
    match scrutinee.ty {
        Ty::Enum(enum_id) => lower_match_enum(b, enum_id, arms, &scrut, result_slot, join_bb, scrutinee),
        Ty::Option(_) | Ty::Result(..) => lower_match_binary(b, scrutinee.ty, arms, &scrut, result_slot, join_bb, scrutinee),
        // Guarded by sema (`match` requires a sum type); be defensive rather than panic.
        _ => b.terminate(Term::Goto(join_bb)),
    }
    b.cur = join_bb;
    match result_slot {
        Some(slot) => {
            let v = b.fresh_value(ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
        None => Operand::Const(Const::Unit),
    }
}

/// A user `enum`: test the scrutinee's tag against each arm's variant and branch to its body,
/// defaulting to the `_`/last arm.
fn lower_match_enum(b: &mut Builder, enum_id: u32, arms: &[hir::MatchArm], scrut: &Operand, result_slot: Option<Slot>, join_bb: BlockId, scrutinee: &hir::Expr) {
    // The default arm is the `_` wildcard (no variants); absent it, the last arm — exhaustiveness
    // guarantees the scrutinee must be one of its variants by the time control reaches it.
    let default_idx = arms.iter().position(|a| a.variants.is_empty()).unwrap_or(arms.len() - 1);
    // Bind a single-variant arm's payload (an or-pattern / wildcard binds nothing).
    let bind_payload = |b: &mut Builder, arm: &hir::MatchArm| {
        if let [v] = arm.variants[..] {
            for (slot, &local) in arm.bindings.iter().enumerate() {
                bind_local(b, local, Rvalue::EnumPayload { enum_id, variant: v, slot: slot as u32, operand: scrut.clone() });
            }
        }
    };
    for (i, arm) in arms.iter().enumerate() {
        if i == default_idx {
            continue;
        }
        let arm_bb = b.new_block();
        let next_bb = b.new_block();
        // Branch into the arm if the scrutinee's tag equals ANY of the arm's variants (an
        // or-pattern tests them in sequence, each falling through to the next on a miss).
        let n = arm.variants.len();
        for (k, &v) in arm.variants.iter().enumerate() {
            let eq = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(eq, Rvalue::EnumTagEq { enum_id, scrutinee: scrut.clone(), variant: v }));
            if k + 1 == n {
                b.terminate(Term::Branch(Operand::Value(eq), arm_bb, next_bb));
            } else {
                let try_next = b.new_block();
                b.terminate(Term::Branch(Operand::Value(eq), arm_bb, try_next));
                b.cur = try_next;
            }
        }
        b.cur = arm_bb;
        bind_payload(b, arm);
        // Binding an owned payload moves it out of the scrutinee; null the scrutinee so its exit
        // `Drop` doesn't double-free the buffer the binding now owns (mirrors `?`/`lower_try`).
        if !arm.bindings.is_empty() {
            null_moved_source(b, scrutinee);
        }
        finish_arm(b, &arm.body, result_slot, join_bb);
        b.cur = next_bb;
    }
    let d = &arms[default_idx];
    bind_payload(b, d);
    if !d.bindings.is_empty() {
        null_moved_source(b, scrutinee);
    }
    finish_arm(b, &d.body, result_slot, join_bb);
}

/// Builtin `Option`/`Result` (exactly two variants): one boolean branch on `IsSome`/`IsOk`, the
/// `true` edge to the Some/Ok arm and `false` to the None/Err arm. Variant 0 = Some/Ok, 1 = None/Err
/// (matching `match_variants`); either side may be the `_` wildcard.
fn lower_match_binary(b: &mut Builder, ty: Ty, arms: &[hir::MatchArm], scrut: &Operand, result_slot: Option<Slot>, join_bb: BlockId, scrutinee: &hir::Expr) {
    let wild = arms.iter().find(|a| a.variants.is_empty());
    let pos = arms.iter().find(|a| a.variants.contains(&0)).or(wild).expect("exhaustive (sema)");
    let neg = arms.iter().find(|a| a.variants.contains(&1)).or(wild).expect("exhaustive (sema)");
    // A lone `_` covers both variants — no test needed (and binds nothing, so no move to null).
    if std::ptr::eq(pos, neg) {
        finish_arm(b, &pos.body, result_slot, join_bb);
        return;
    }
    let cond = b.fresh_value(Ty::Bool);
    let test = match ty {
        Ty::Option(_) => Rvalue::OptionIsSome(scrut.clone()),
        _ => Rvalue::ResultIsOk(scrut.clone()),
    };
    b.push(Stmt::Let(cond, test));
    let pos_bb = b.new_block();
    let neg_bb = b.new_block();
    b.terminate(Term::Branch(Operand::Value(cond), pos_bb, neg_bb));
    b.cur = pos_bb;
    bind_binary(b, ty, true, pos, scrut);
    // Binding an owned payload (Ok/Some) moves it out of the scrutinee; null the scrutinee so its
    // exit `Drop` doesn't double-free the buffer the binding now owns (mirrors `?`/`lower_try`).
    if !pos.bindings.is_empty() {
        null_moved_source(b, scrutinee);
    }
    finish_arm(b, &pos.body, result_slot, join_bb);
    b.cur = neg_bb;
    bind_binary(b, ty, false, neg, scrut);
    if !neg.bindings.is_empty() {
        null_moved_source(b, scrutinee);
    }
    finish_arm(b, &neg.body, result_slot, join_bb);
}

/// Bind the payload of an `Option`/`Result` arm: Some/Ok → the unwrapped value, Err → the error;
/// None (and any `_` wildcard) binds nothing.
fn bind_binary(b: &mut Builder, ty: Ty, is_pos: bool, arm: &hir::MatchArm, scrut: &Operand) {
    // A wildcard / or-pattern arm binds nothing (no bindings); only a single Some/Ok/Err arm does.
    if arm.bindings.is_empty() {
        return;
    }
    let rv = match (ty, is_pos) {
        (Ty::Option(_), true) => Rvalue::OptionUnwrap(scrut.clone()),
        (Ty::Result(..), true) => Rvalue::ResultUnwrapOk(scrut.clone()),
        (Ty::Result(..), false) => Rvalue::ResultUnwrapErr(scrut.clone()),
        _ => return,
    };
    bind_local(b, arm.bindings[0], rv);
}

/// Compute an rvalue into a fresh value and store it into a binding local's slot.
fn bind_local(b: &mut Builder, local: u32, rv: Rvalue) {
    let pty = b.slots[local as usize];
    let pv = b.fresh_value(pty);
    b.push(Stmt::Let(pv, rv));
    b.push(Stmt::Store(local, Operand::Value(pv)));
}

/// Lower an arm body and, unless it diverged, store the value into the result slot and jump to join.
fn finish_arm(b: &mut Builder, body: &hir::Expr, result_slot: Option<Slot>, join_bb: BlockId) {
    let av = lower_expr(b, body);
    if !b.is_terminated() {
        if let Some(slot) = result_slot {
            b.push(Stmt::Store(slot, av));
            // If the arm yields an owned local (`Ok(xs) => xs`), it moved into the match result; null
            // that source so its exit `Drop` doesn't double-free the buffer the result now owns. (A
            // diverging arm already returned via `lower_fn`'s own null-on-move; a `result_slot`-less
            // (Unit) match has a Unit body, so there is never an owned local to null in that case.)
            null_moved_source(b, body);
        }
        b.terminate(Term::Goto(join_bb));
    }
}

/// `result.map_err(f)` — branch on `Result`: `Ok(v)` passes through; `Err(e)` becomes `Err(f(e))`.
fn lower_map_err(b: &mut Builder, result: &hir::Expr, f: &hir::Expr, out_ty: Ty) -> Operand {
    let (ok_s, e_s) = match result.ty {
        Ty::Result(o, e) => (o, e),
        _ => return lower_expr(b, result), // guarded by sema
    };
    let e2_ty = match out_ty {
        Ty::Result(_, e2) => align_sema::scalar_to_ty(e2),
        _ => out_ty,
    };
    let rv = lower_expr(b, result);
    let fv = lower_expr(b, f);
    // `map_err` unwraps the result on both branches — if it was an owned local, null its slot so
    // the exit cleanup doesn't double-free the moved-out payload.
    null_moved_source(b, result);
    let rslot = b.new_slot(out_ty);
    let is_ok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_ok, Rvalue::ResultIsOk(rv.clone())));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    b.terminate(Term::Branch(Operand::Value(is_ok), ok_bb, err_bb));
    // Ok: pass the payload through unchanged.
    b.cur = ok_bb;
    let okp = b.fresh_value(align_sema::scalar_to_ty(ok_s));
    b.push(Stmt::Let(okp, Rvalue::ResultUnwrapOk(rv.clone())));
    let okr = b.fresh_value(out_ty);
    b.push(Stmt::Let(okr, Rvalue::ResultOk(Operand::Value(okp))));
    b.push(Stmt::Store(rslot, Operand::Value(okr)));
    b.terminate(Term::Goto(join));
    // Err: apply `f` to the error, re-wrap.
    b.cur = err_bb;
    let errp = b.fresh_value(align_sema::scalar_to_ty(e_s));
    b.push(Stmt::Let(errp, Rvalue::ResultUnwrapErr(rv)));
    let conv = b.fresh_value(e2_ty);
    b.push(Stmt::Let(
        conv,
        Rvalue::CallIndirect { callee: fv, args: vec![Operand::Value(errp)], param_tys: vec![align_sema::scalar_to_ty(e_s)], ret_ty: e2_ty },
    ));
    let errr = b.fresh_value(out_ty);
    b.push(Stmt::Let(errr, Rvalue::ResultErr(Operand::Value(conv))));
    b.push(Stmt::Store(rslot, Operand::Value(errr)));
    b.terminate(Term::Goto(join));
    b.cur = join;
    let r = b.fresh_value(out_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// Lower a short-circuiting `&&` / `||`. The left operand is always evaluated; the right operand is
/// evaluated only in the branch where it can still change the result — `a && b` skips `b` when `a`
/// is false (result `false`), `a || b` skips `b` when `a` is true (result `true`). Structurally an
/// `if` over `a` that yields either the constant short-circuit value or `b`.
fn lower_short_circuit(b: &mut Builder, op: BinOp, lhs: &hir::Expr, rhs: &hir::Expr) -> Operand {
    let slot = b.new_slot(Ty::Bool);
    let l = lower_expr(b, lhs);
    let rhs_bb = b.new_block();
    let short_bb = b.new_block();
    let join_bb = b.new_block();
    // `&&`: if `a` is true, evaluate `b`; else short-circuit to `false`.
    // `||`: if `a` is true, short-circuit to `true`; else evaluate `b`.
    let (true_bb, false_bb) = match op {
        BinOp::And => (rhs_bb, short_bb),
        BinOp::Or => (short_bb, rhs_bb),
        _ => unreachable!("lower_short_circuit called with a non-logical operator"),
    };
    b.terminate(Term::Branch(l, true_bb, false_bb));

    // The right-operand branch: the result is `b`. If `b` itself diverges (a `return` inside a
    // block operand), it already terminated `rhs_bb` — don't push a dead store / second terminator.
    b.cur = rhs_bb;
    let r = lower_expr(b, rhs);
    if !b.is_terminated() {
        b.push(Stmt::Store(slot, r));
        b.terminate(Term::Goto(join_bb));
    }

    // The short-circuit branch: the result is the constant that `a` alone determines.
    b.cur = short_bb;
    let short_val = Const::Bool(matches!(op, BinOp::Or));
    b.push(Stmt::Store(slot, Operand::Const(short_val)));
    b.terminate(Term::Goto(join_bb));

    b.cur = join_bb;
    let v = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(v, Rvalue::Load(slot)));
    Operand::Value(v)
}

/// The free-standing owned locals (a subset of `drop_locals`) declared inside a loop body — its
/// per-iteration locals, dropped each pass. Taken from the loop's `body_locals` range (every local
/// declared anywhere in the body, recorded by sema; see `hir::ExprKind::Loop`), intersected with
/// `drop_locals` in declaration order — no fragile per-`ExprKind` walk that could miss a `let`
/// nested in a call argument / tuple / operand.
fn loop_iter_drops(b: &Builder, body_locals: &std::ops::Range<u32>) -> Vec<Slot> {
    b.drop_locals.iter().copied().filter(|s| body_locals.contains(s)).collect()
}

/// `loop { ... }` — a header block, the body, a back-edge to the header, and an exit block that
/// `break` targets. The loop's value is stored into `result_slot` at each `break` and loaded at the
/// exit. Per-iteration owned locals are dropped (and null-reset) at the back-edge and at each `break`
/// so a per-iteration allocation is freed once per pass. A loop with no `break` (`diverges`) never
/// reaches its exit — the header always loops back — so the exit is `Unreachable`.
///
/// `#[inline(never)]` and reached through a bindings-free `lower_expr` arm (taking `e`, destructured
/// here) so neither this large-framed helper nor any arm locals enlarge the recursive `lower_expr`
/// frame (a deep expression chain descends `lower_expr` once per level; see the `expr_depth` test).
#[inline(never)]
fn lower_loop(b: &mut Builder, e: &hir::Expr) -> Operand {
    let hir::ExprKind::Loop { body, diverges, body_locals } = &e.kind else {
        unreachable!("lower_loop on a non-loop expression");
    };
    let (diverges, ty) = (*diverges, e.ty);
    let result_slot = (ty != Ty::Unit && !diverges).then(|| b.new_slot(ty));
    let iter_drops = loop_iter_drops(b, body_locals);
    let header = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));
    b.cur = header;
    b.loops.push(LoopFrame { exit, result_slot, iter_drops: iter_drops.clone() });
    let _ = lower_block(b, body); // the body's trailing value is discarded each iteration
    // Fall-through end of an iteration: drop this pass's per-iteration owned locals (null-resetting
    // so the next pass — or a path that never re-allocated — frees null), then loop back.
    if !b.is_terminated() {
        for s in &iter_drops {
            b.push(Stmt::Drop(*s));
            b.push(Stmt::DropFlagInit(*s));
        }
        b.terminate(Term::Goto(header));
    }
    b.loops.pop();
    b.cur = exit;
    if diverges {
        // No `break`: the exit is unreachable. Terminate it so the CFG is well-formed; the returned
        // operand is never used (code after a diverging loop is dead — `lower_block` stops here).
        b.terminate(Term::Unreachable);
        return Operand::Const(Const::Bool(false));
    }
    match result_slot {
        Some(slot) => {
            let v = b.fresh_value(ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
        // A unit-valued loop: the value is unused by the caller (statement position).
        None => Operand::Const(Const::Bool(false)),
    }
}

fn lower_if(
    b: &mut Builder,
    cond: &hir::Expr,
    then: &hir::Block,
    els: &hir::Block,
    ty: Ty,
) -> Operand {
    let result_slot = (ty != Ty::Unit).then(|| b.new_slot(ty));

    let c = lower_expr(b, cond);
    let then_bb = b.new_block();
    let else_bb = b.new_block();
    let join_bb = b.new_block();
    b.terminate(Term::Branch(c, then_bb, else_bb));

    b.cur = then_bb;
    let tv = lower_block(b, then);
    if let (Some(slot), Some(op)) = (result_slot, tv) {
        b.push(Stmt::Store(slot, op));
    }
    b.terminate(Term::Goto(join_bb));

    b.cur = else_bb;
    let ev = lower_block(b, els);
    if let (Some(slot), Some(op)) = (result_slot, ev) {
        b.push(Stmt::Store(slot, op));
    }
    b.terminate(Term::Goto(join_bb));

    b.cur = join_bb;
    match result_slot {
        Some(slot) => {
            let v = b.fresh_value(ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
        // Unit if: value is unused by the caller (statement position).
        None => Operand::Const(Const::Bool(false)),
    }
}

/// A short type name used in MIR text / diagnostics.
pub fn ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(IntTy { bits, signed }) => format!("{}{}", if signed { 'i' } else { 'u' }, bits),
        Ty::IntVar(_) => "int?".to_string(),
        Ty::Float(FloatTy { bits }) => format!("f{bits}"),
        Ty::FloatVar(_) => "float?".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Option(_) => "Option".to_string(),
        Ty::Result(..) => "Result".to_string(),
        Ty::Box(_) => "box".to_string(),
        Ty::Raw => "raw".to_string(),
        Ty::Array(_, n) | Ty::StructArray(_, n) => format!("array[{n}]"),
        Ty::Slice(_) => "slice".to_string(),
        Ty::Vec(_, n) => format!("vec{n}"),
        Ty::Mask(_, n) => format!("mask{n}"),
        Ty::Soa(id) => format!("soa<struct#{id}>"),
        Ty::DynArray(_) => "array".to_string(),
        Ty::DynStructArray(id, _) => format!("array<struct#{id}>"),
        Ty::DynSliceArray(_) => "array<slice>".to_string(),
        Ty::Str => "str".to_string(),
        Ty::String => "string".to_string(),
        Ty::ArenaHandle => "arena".to_string(),
        Ty::Builder => "builder".to_string(),
        Ty::Writer => "writer".to_string(),
        Ty::Reader => "reader".to_string(),
        Ty::Buffer => "buffer".to_string(),
        Ty::Rng => "rng".to_string(),
        Ty::CliCommand => "cli command".to_string(),
        Ty::CliParsed => "cli parsed".to_string(),
        Ty::TcpConn => "tcp_conn".to_string(),
        Ty::TcpListener => "tcp_listener".to_string(),
        Ty::UdpSocket => "udp_socket".to_string(),
        Ty::Child => "child".to_string(),
        Ty::HttpRequest => "http request".to_string(),
        Ty::HttpResponse => "http response".to_string(),
        Ty::HttpClient => "http client".to_string(),
        Ty::Struct(id) => format!("struct#{id}"),
        Ty::Tuple(id) => format!("tuple#{id}"),
        Ty::Fn(id) => format!("fn#{id}"),
        Ty::Enum(id) => format!("enum#{id}"),
        Ty::Task(_) => "Task".to_string(),
        Ty::DictEncoded(id, _) => format!("dict_encoded<struct#{id}>"),
        // Monomorphization substitutes every `Ty::Param` before MIR; reaching here is a compiler bug.
        Ty::Param(_) => unreachable!("Ty::Param survived monomorphization"),
        Ty::Unit => "()".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_diag::Diagnostics;
    use align_lexer::tokenize;
    use align_parser::parse_file;
    use align_sema::check_file;

    fn lower(src: &str) -> Program {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors());
        lower_program(&hir)
    }

    #[test]
    fn m0_lowers_to_return() {
        let p = lower("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        let f = &p.fns[0];
        // entry stores the literal into x's slot; a later block returns the loaded value.
        assert!(f.blocks.iter().any(|b| matches!(b.term, Term::Return(Some(_)))));
    }

    #[test]
    fn if_creates_branch() {
        let p = lower("fn f(n: i64) -> i64 {\n  if n < 2 { return n }\n  return n\n}\n");
        let f = &p.fns[0];
        assert!(f.blocks.iter().any(|b| matches!(b.term, Term::Branch(..))));
    }

    #[test]
    fn struct_lowers_to_field_stores_and_loads() {
        let src = "Point { x: i32, y: i32 }\nfn main() -> i32 {\n  p := Point { x: 3, y: 4 }\n  return p.x + p.y\n}\n";
        let p = lower(src);
        // `Point` plus the always-registered builtin `argon2_params` struct (the std.crypto Argon2
        // parameters type — present in every program's struct table, like the builtin `Error` enum).
        assert_eq!(p.structs.len(), 2);
        let f = &p.fns[0];
        let stmts: Vec<&Stmt> = f.blocks.iter().flat_map(|b| &b.stmts).collect();
        // Two field stores for the literal, two field loads for the reads.
        assert_eq!(stmts.iter().filter(|s| matches!(s, Stmt::StoreField(..))).count(), 2);
        assert_eq!(
            stmts
                .iter()
                .filter(|s| matches!(s, Stmt::Let(_, Rvalue::Field(..))))
                .count(),
            2
        );
    }

    /// Count, across every function (incl. lifted lambdas), how many statements match `pred`.
    fn count_stmts(p: &Program, pred: impl Fn(&Stmt) -> bool) -> usize {
        p.fns.iter().flat_map(|f| &f.blocks).flat_map(|b| &b.stmts).filter(|s| pred(s)).count()
    }

    const BUILDER_REDUCE_SRC: &str = "pub fn build(s: slice<i64>) -> i64 {\n  b := s.reduce(builder(), fn b, x {\n    b.write(\"item-\")\n    b.write_int(x)\n    b.write(\"-status \")\n    b\n  })\n  res := b.to_string()\n  return res.len()\n}\n";

    #[test]
    fn builder_str_int_str_is_fused() {
        let p = lower(BUILDER_REDUCE_SRC);
        // The `str,int,str` triple collapses to one fused write; the two component writes are gone.
        assert_eq!(count_stmts(&p, |s| matches!(s, Stmt::Let(_, Rvalue::BuilderWriteStrIntStr(..)))), 1);
        assert_eq!(count_stmts(&p, |s| matches!(s, Stmt::Let(_, Rvalue::BuilderWriteStr(..)))), 0);
        assert_eq!(count_stmts(&p, |s| matches!(s, Stmt::Let(_, Rvalue::BuilderWriteInt(..)))), 0);
    }

    #[test]
    fn builder_int_str_str_is_not_fused() {
        // Wrong shape (`int,str,str`): the peephole only fuses `str,int,str`, so nothing collapses.
        let src = "pub fn build(s: slice<i64>) -> i64 {\n  b := s.reduce(builder(), fn b, x {\n    b.write_int(x)\n    b.write(\"-a-\")\n    b.write(\"-b-\")\n    b\n  })\n  res := b.to_string()\n  return res.len()\n}\n";
        let p = lower(src);
        assert_eq!(count_stmts(&p, |s| matches!(s, Stmt::Let(_, Rvalue::BuilderWriteStrIntStr(..)))), 0);
        assert_eq!(count_stmts(&p, |s| matches!(s, Stmt::Let(_, Rvalue::BuilderWriteInt(..)))), 1);
        assert_eq!(count_stmts(&p, |s| matches!(s, Stmt::Let(_, Rvalue::BuilderWriteStr(..)))), 2);
    }
}
