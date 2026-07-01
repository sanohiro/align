//! typed HIR: the tree after type checking (`docs/impl/03-types.md` §10).
//!
//! Nearly isomorphic to the AST, but every expression carries a resolved [`Ty`] and
//! references resolve to a [`LocalId`]. An anti-rewrite output so later stages
//! (MIR/codegen) don't recompute types (`00-overview.md`). M1 has functions + calls,
//! `if` (always represented as an expression; statement-position `if` just has a
//! `Unit` value), comparison/logical operators, `bool`, and `mut` reassignment.

use crate::Ty;
use align_ast::{BinOp, UnOp};
use align_span::Span;

/// Identifier of a local variable (and its memory slot) within a function body.
pub type LocalId = u32;

/// The overflow handling of an explicit-overflow integer op ([`ExprKind::IntArith`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArithMode {
    /// `saturating_*`: clamp to the type's MIN/MAX; result is the same int type.
    Saturating,
    /// `checked_*`: `Option<T>` — `None` on overflow, else `Some(result)`.
    Checked,
}

/// A scalar math builtin ([`ExprKind::MathOp`]) — a method on a numeric value (`core.math`).
/// `Abs`/`Min`/`Max` accept any numeric type; the rest are **float-only**.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathFn {
    /// `x.abs()` — absolute value (signed int / float; identity on unsigned).
    Abs,
    /// `a.min(b)` — the smaller of two numbers (pairwise; distinct from the array reduction).
    Min,
    /// `a.max(b)` — the larger of two numbers (pairwise).
    Max,
    /// `x.sqrt()` — square root (float).
    Sqrt,
    /// `x.floor()` — round toward -∞ (float).
    Floor,
    /// `x.ceil()` — round toward +∞ (float).
    Ceil,
    /// `x.round()` — round to nearest, ties away from zero (float).
    Round,
    /// `x.trunc()` — round toward zero (float).
    Trunc,
    /// `b.pow(e)` — `b` raised to `e` (float).
    Pow,
    /// `fma(a, b, c)` — fused multiply-add `a*b + c` with a single rounding (float scalar or
    /// vector). A free builtin (like `dot`/`select`), not a method; one `vfmadd`/`fmla` instruction.
    Fma,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Fn>,
    /// Struct definitions, indexed by the id carried in [`crate::Ty::Struct`].
    pub structs: Vec<StructDef>,
    /// Sum-type definitions, indexed by the id carried in [`crate::Ty::Enum`].
    pub enums: Vec<EnumDef>,
    /// Anonymous tuple types, indexed by the id carried in [`crate::Ty::Tuple`]. Interned
    /// (deduplicated by element list) during checking, so `(i64, i64)` is one entry.
    pub tuples: Vec<TupleDef>,
    /// Function-value types, indexed by the id carried in [`crate::Ty::Fn`]. Interned during
    /// checking. A `Ty::Fn` value is a function pointer (Copy / `Static`, no environment yet —
    /// non-capturing first-class functions, slice ①).
    pub fn_types: Vec<FnTy>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnTy {
    /// Parameter types (scalar-only for now).
    pub params: Vec<crate::Scalar>,
    /// Return type (a scalar).
    pub ret: crate::Scalar,
}

/// One checked `match` arm. `variants` = the covered variant tags: empty = the `_` wildcard, one
/// = a simple arm, many = an or-pattern (`A | B`). `bindings` are the locals bound to the variant's
/// payload (one per payload slot, in order); an or-pattern / wildcard binds nothing.
#[derive(Clone, Debug)]
pub struct MatchArm {
    pub variants: Vec<u32>,
    pub bindings: Vec<crate::LocalId>,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct TupleDef {
    /// Element types in positional order (`t.0`, `t.1`, …). PR1 cut: primitive scalars only
    /// (int/float/bool/char) — all Copy / `Static`, so a tuple needs no drop or region tracking
    /// yet; owned (`string`/`array<T>`) and `str` elements are a later, additive slice.
    pub elems: Vec<crate::Scalar>,
}

#[derive(Clone, Debug)]
pub struct EnumDef {
    pub name: String,
    /// Variants in declaration order; the index is the tag.
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub name: String,
    /// Positional scalar payload (S1b); empty for a tag-only variant.
    pub payload: Vec<crate::Scalar>,
    /// The first struct field index holding this variant's payload. The enum lowers to a
    /// non-union struct `{ i32 tag, <every variant's payload flattened> }`, so field 0 is the tag
    /// and this variant's payload occupies fields `field_base .. field_base + payload.len()`.
    pub field_base: u32,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    /// Fields in declaration order; the position is the field index used by MIR/codegen.
    pub fields: Vec<FieldDef>,
    /// A declared over-alignment in bytes (`align(N) Node { … }`, for GPU/DMA/page-aligned
    /// zero-copy interop), or `None` for the type's natural alignment. **Reserved for M6** — there
    /// is no surface syntax yet, so this is always `None` today (`open-questions.md` Open
    /// "`align(N)`"). Carrying it on the type now means the M6 work is "parse `align(N)` → set this
    /// field" + "honor it in the one alignment seam" rather than a cross-cutting retrofit.
    pub align: Option<u32>,
}

impl StructDef {
    /// Index of a field by name, if present.
    pub fn field_index(&self, name: &str) -> Option<u32> {
        self.fields.iter().position(|f| f.name == name).map(|i| i as u32)
    }
}

#[derive(Clone, Debug)]
pub struct FieldDef {
    pub name: String,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub struct Fn {
    pub name: String,
    /// Parameter locals, in declaration order. Each is also present in `locals`.
    pub params: Vec<LocalId>,
    pub ret: Ty,
    /// All locals (params + `let` bindings), indexed by [`LocalId`]. Each is a slot.
    pub locals: Vec<Local>,
    pub body: Block,
    pub span: Span,
    /// Free-standing **owned** locals (heap `array<T>`, region `Static`) that are *not*
    /// moved out — MIR must drop (free) each at every function exit. Arena-allocated owned
    /// values are excluded (the arena bulk-frees them). Populated after move/escape analysis
    /// (MMv2 slice 4).
    pub drop_locals: Vec<LocalId>,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub id: LocalId,
    pub name: String,
    pub ty: Ty,
    pub is_mut: bool,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// Trailing expression = block value (`None` if the block has no value).
    /// Boxed to break the `Expr` -> `If` -> `Block` -> `Expr` type cycle.
    pub value: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let { local: LocalId, init: Expr },
    /// `(a, b, ...) := expr` — bind each tuple element to a local. A `None` entry is an
    /// ignored (`_`) element. `tuple_id` indexes [`Program::tuples`] (the `init`'s type).
    LetTuple { locals: Vec<Option<LocalId>>, tuple_id: u32, init: Expr },
    /// `local = value` — reassign a `mut` local. `drop_old` (set by `MoveCheck`) is true when the
    /// local owns a heap buffer that the RHS does *not* move out, so the value being overwritten
    /// must be dropped (freed) before the store — else its buffer leaks. It is a [`Cell`] so the
    /// move analysis, which holds only `&Stmt`, can record the decision without a mutable walk.
    Assign { local: LocalId, value: Expr, drop_old: std::cell::Cell<bool> },
    /// `base[index] = value` — element store into a `mut` array local or `out` slice parameter.
    /// Lowering emits a bounds check (abort on out-of-range), like an element read.
    AssignIndex { base: LocalId, index: Expr, value: Expr },
    /// `v[lane] = value` — write one lane of a `mut vecN<T>` local (M6): `v = insertelement(v,
    /// value, lane)`. `lane` is a constant in `0..N`.
    AssignVecLane { local: LocalId, lane: u32, value: Expr },
    /// `root.f0.f1.… = value` — store into a (possibly nested) field of a struct local. `path` is
    /// the chain of field indices (length ≥ 1).
    AssignField { root: LocalId, path: Vec<u32>, value: Expr },
    /// `base[index].field = value` — store one scalar field of element `index` of a struct-array or
    /// soa local (the write counterpart of the `base[index].field` read). `soa` picks the lowering:
    /// a column store (`StoreColumn`) for a `soa<Struct>`, else a slot element-field store
    /// (`StoreElemField`) for a fixed `array<Struct>`. Lowering emits a bounds check. `field` is the
    /// (scalar) field index; soa/struct fields are primitive, so no whole-element copy or drop.
    AssignElemField { base: LocalId, index: Expr, field: u32, struct_id: u32, soa: bool, value: Expr },
    /// `base[index] = value` — store a whole struct value into element `index` (the write
    /// counterpart of the `base[index]` whole-element read / `s[i]` gather). `soa` picks the
    /// lowering: a per-column scatter (`StoreColumn` per field) for a `soa<Struct>`, else a single
    /// aggregate slot store (`StoreIndex`) for a fixed `array<Struct>`. First cut: the struct is
    /// plain-old-data (flat primitive-scalar fields), so the value is Copy — no region/move/drop.
    AssignElem { base: LocalId, index: Expr, struct_id: u32, soa: bool, value: Expr },
    Return(Option<Expr>),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Ty,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Unit,
    Int(i128),
    Float(f64),
    Char(u32),
    Str(String),
    Bool(bool),
    Local(LocalId),
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    /// `expr as T` — an explicit numeric/char conversion. The target type is this expression's
    /// `ty`; the source type is `inner.ty`. Both are concrete primitive scalars (int / float /
    /// char). Lowers to one MIR `Cast` (truncate / extend / int↔float / float-saturating-to-int).
    Cast(Box<Expr>),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Explicit-overflow integer arithmetic (`core.math`): `x.saturating_add(y)` /
    /// `x.checked_mul(y)` etc. `op` is `Add`/`Sub`/`Mul`. `Saturating` clamps to the type's
    /// MIN/MAX and yields the same int type; `Checked` yields `Option<T>` (`None` on overflow).
    /// (`wrapping_*` is just the default wrapping `Binary`, so it is not represented here.)
    IntArith {
        op: BinOp,
        mode: ArithMode,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// A scalar math builtin (`core.math`): `x.abs()` (one operand) / `a.min(b)` / `a.max(b)`
    /// (two operands). All operands and the result share the numeric type (the `Expr`'s `ty`).
    MathOp {
        fn_: MathFn,
        operands: Vec<Expr>,
    },
    /// A first-class function value (`f := fn x: i32 { … }`): a pointer to the lifted top-level
    /// function `name`. Non-capturing only (slice ①) — no environment. Type is `Ty::Fn`.
    FnValue(String),
    /// A *capturing* closure value (`f := fn x: i32 { x + k }`): the lifted function `lifted`
    /// (which takes the captures as trailing parameters) plus the captured values, which are
    /// copied into a heap/stack environment. `captures` are the enclosing locals, in the order the
    /// lifted function expects them. Type is `Ty::Fn`. Slice ②b-2: scalar (Copy) captures, env on
    /// the stack (the closure cannot escape its frame yet).
    Closure {
        lifted: String,
        captures: Vec<Expr>,
    },
    /// An indirect call through a function value: `f(args)` where `f` is a `Ty::Fn` local.
    CallFnValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `task_group { … }` — a structured concurrency scope (slice ④). ④a lowers it as its block.
    TaskGroup(Block),
    /// `Type.Variant` / `Type.Variant(args)` — a sum-type value. `enum_id` indexes `Program.enums`,
    /// `variant` is the tag; `payload` are the constructor arguments (empty for a tag-only variant).
    EnumValue { enum_id: u32, variant: u32, payload: Vec<Expr> },
    /// `match scrutinee { … }` — exhaustive match over a sum type. `arms` are in source order; a
    /// `variant` of `None` is the `_` wildcard. The expression's value is the matched arm's value.
    Match { scrutinee: Box<Expr>, arms: Vec<MatchArm> },
    /// `result.map_err(f)` — map a `Result`'s error with `f: fn(E) -> E'` (`Ok` passes through).
    ResultMapErr { result: Box<Expr>, f: Box<Expr> },
    /// `spawn(fn { … })` — defer a task; `closure` is the spawned closure value. `fallible` = the
    /// closure returns `Result<R, Error>` (so its `Err` is surfaced by `wait()?`); the task's
    /// result type is `Task<R>` (the `Ok` payload) either way.
    Spawn { closure: Box<Expr>, fallible: bool },
    /// `t.get()` — read a spawned task's result. ④a: identity (the `Task<R>` already holds `R`).
    TaskGet(Box<Expr>),
    /// `wait()` — join all spawned tasks (the single error boundary, ④c). ④a: a no-op marker
    /// (eager execution already completed each task at its `spawn`).
    Wait,
    Call {
        func: String,
        args: Vec<Expr>,
        /// Concrete type arguments inferred for a call to a generic function (one per declared
        /// type parameter); empty for a non-generic call. Monomorphization uses these to pick /
        /// generate the specialized instance and rewrites `func` to its mangled name.
        type_args: Vec<Ty>,
    },
    /// `if` as a value. An absent `else` becomes an empty block with no value, and the
    /// whole expression's `ty` is then `Unit`.
    If {
        cond: Box<Expr>,
        then: Block,
        els: Block,
    },
    /// `Name { ... }`. `fields` are in declaration order and fully populated; `struct_id`
    /// indexes [`Program::structs`]. M1: only valid as a `let` initializer.
    StructLit {
        struct_id: u32,
        fields: Vec<Expr>,
    },
    /// `root.f0.f1.…` read — a (possibly nested) field projection rooted at a struct local. `path`
    /// is the chain of field indices (length ≥ 1); the expression `ty` is the innermost field type.
    /// Lowers to a single GEP `[0, *path]`.
    Field {
        root: LocalId,
        path: Vec<u32>,
    },
    /// `soa_value.field` — project one column of a `soa<Struct>` view as a `slice<FieldTy>`. `base`
    /// is the soa local; `struct_id`/`field` identify the column. Lowers to the column's
    /// `{ ptr + len*prefix_bytes, len }` slice (prefix_bytes = the sizes of the preceding fields).
    SoaColumn {
        base: LocalId,
        struct_id: u32,
        field: u32,
    },
    /// `(e0, e1, ...)` — a tuple value. `tuple_id` indexes [`Program::tuples`]; the
    /// expression `ty` is `Ty::Tuple(tuple_id)`.
    Tuple {
        tuple_id: u32,
        elems: Vec<Expr>,
    },
    /// `recv.N` — positional read of a tuple element. The expression `ty` is the element type.
    TupleIndex {
        recv: Box<Expr>,
        index: u32,
    },
    /// `base[index].field` — read `field` of element `index` of a struct-array local. Used
    /// by `json.encode` over a fixed struct array (unrolled; `index` is a constant).
    IndexField {
        base: LocalId,
        index: u32,
        field: u32,
    },
    /// A block used in expression position; its value is the trailing expression (or
    /// `Unit`). Preserves statements (e.g. a diverging `{ return … }`).
    Block(Block),
    /// `Some(x)` — the expression `ty` is the resulting `Option<T>`.
    OptionSome(Box<Expr>),
    /// `None` — the expression `ty` is the `Option<T>` fixed by context.
    OptionNone,
    /// `opt else fallback` — Option unwrap. `ty` is the unwrapped payload type.
    ElseUnwrap {
        opt: Box<Expr>,
        fallback: Box<Expr>,
    },
    /// `Ok(x)` — the expression `ty` is the resulting `Result<T, E>`.
    ResultOk(Box<Expr>),
    /// `Err(e)` — the expression `ty` is the resulting `Result<T, E>`.
    ResultErr(Box<Expr>),
    /// `expr?` — Result propagation; `ty` is the unwrapped ok payload type. Lowered
    /// against the enclosing function's return type (carried by MIR).
    Try(Box<Expr>),
    /// `arena { ... }` — a region; allocations inside are bulk-freed at block end.
    Arena(Block),
    /// `heap.new(x)` — allocate a `box<T>` in the enclosing arena.
    HeapNew(Box<Expr>),
    /// `b.get()` — read (copy) the value out of a `box<T>`.
    BoxGet(Box<Expr>),
    /// `b.clone()` — deep-copy a `box<T>` into a fresh allocation in the enclosing arena.
    BoxClone(Box<Expr>),
    /// `s.clone()` on a `str` — deep-copy the bytes into a fresh heap-owned `string` (MMv2
    /// slice 7). The result owns its buffer (`Drop`-freed), so it can escape its source's
    /// region — the explicit escape hatch out of a zero-copy view.
    StrClone(Box<Expr>),
    /// `s.contains(n)` / `s.starts_with(p)` / `s.ends_with(s)` — a byte-oriented `str` predicate
    /// (`core.string`), `ty` = `bool`. Both operands are `str` views (an owned `string` operand is
    /// auto-borrowed via [`ExprKind::StrBorrow`]); the comparison reads bytes only, so neither is
    /// moved. Backed by the runtime's `memchr`-class scans.
    StrPredicate { kind: StrPredKind, haystack: Box<Expr>, needle: Box<Expr> },
    /// `s.trim()` / `s.trim_start()` / `s.trim_end()` — strip ASCII whitespace, yielding a
    /// **borrowed sub-`str`** of `recv` (`ty` = `str`, no allocation). `recv` is a `str` view (an
    /// owned `string` is auto-borrowed via [`ExprKind::StrBorrow`]); the result views the same
    /// bytes, so it inherits `recv`'s region and must not outlive it. Backed by a runtime bounds scan.
    StrTrim { kind: StrTrimKind, recv: Box<Expr> },
    /// Borrow an owned `string` as a `str` view (MMv2 slice 7b). The two share the `{ptr,len}`
    /// layout, so this is a zero-cost, allocation-free read-only view — an implicit coercion at
    /// a `str`-parameter call site. The `string` is **not** moved (it stays owned by its slot
    /// and is `Drop`-freed by its owner); the view borrows it, so it is `Frame`-regioned and
    /// must not outlive the frame holding the `string`.
    StrBorrow(Box<Expr>),
    /// `builder()` / `builder(capacity)` — open an append-oriented string builder (MMv2 slice 7c).
    /// The `ty` is [`crate::Ty::Builder`] (an owned, Move handle). `capacity` (an `i64` expr, if
    /// given) pre-sizes the backing buffer so appends don't reallocate as it grows.
    BuilderNew { capacity: Option<Box<Expr>> },
    /// `b.write(s)` / `b.write_int(n)` — append to a builder, mutating it through its handle.
    /// The builder is borrowed (not consumed); the `ty` is `Unit`.
    BuilderWrite { builder: Box<Expr>, arg: Box<Expr>, kind: BuilderWriteKind },
    /// `b.to_string()` — finish a builder into an **owned** `string`, consuming (moving) the
    /// builder. The `ty` is [`crate::Ty::String`].
    BuilderToString(Box<Expr>),
    /// `[e1, e2, ...]` — a fixed-length array literal. `elem` is the element type
    /// (a scalar, or a struct for an array-of-structs whose elements are `StructLit`s).
    ArrayLit { elems: Vec<Expr>, elem: crate::Ty },
    /// `select(mask, a, b)` — lane-wise blend of two `vecN<T>` by a `mask` (M6 slice 2): lane `i`
    /// is `a[i]` where `mask[i]`, else `b[i]`. Lowers to `Rvalue::Select` (an LLVM vector `select`).
    Select { mask: Box<Expr>, a: Box<Expr>, b: Box<Expr> },
    /// `vec.sum_where(mask)` — masked horizontal sum (M6): sum of the lanes where the mask is set,
    /// yielding the element scalar. Lowers to `select(mask, vec, 0)` then a lane reduction.
    VecSumWhere { vec: Box<Expr>, mask: Box<Expr> },
    /// `dot(a, b)` — the dot product of two `vecN<T>` (M6): the element scalar `sum(a[i] * b[i])`.
    /// Lowers to a vector multiply then a lane reduction (the multiply dual of [`VecSumWhere`]).
    VecDot { a: Box<Expr>, b: Box<Expr> },
    /// `v.min()` / `v.max()` — the horizontal min/max of a `vecN<T>` (M6): the smallest/largest lane,
    /// as the element scalar. `max` selects max vs min. Folded with the scalar min/max intrinsic.
    VecMinMax { vec: Box<Expr>, max: bool },
    /// `v.sum()` — the horizontal sum of a `vecN<T>` (M6): the sum of all lanes, as the element
    /// scalar (the unmasked sibling of [`VecSumWhere`]). Lowers via the shared lane reduction.
    VecSum { vec: Box<Expr> },
    /// `s.load(i)` — load `N` consecutive elements of a `slice<T>` starting at index `i` into a
    /// `vecN<T>` (M6): a bounds-checked vector load. `N`/`elem` come from the target annotation.
    VecLoad { src: Box<Expr>, index: Box<Expr>, elem: crate::Scalar, n: u32 },
    /// `s.store(i, v)` — store the `N` lanes of `v` into a writable `slice<T>` at `i..i+N` (M6): a
    /// bounds-checked vector store. Yields `()`. `dst` is a `mut`/`out` slice place.
    VecStore { dst: Box<Expr>, index: Box<Expr>, value: Box<Expr>, elem: crate::Scalar, n: u32 },
    /// `[e0, e1, …]` under a `vecN<T>` annotation — a fixed-width SIMD vector value (M6 slice 1).
    /// Unlike [`ArrayLit`] (slot/memory), a vector is a **register value**: it lowers to a single
    /// `Rvalue::MakeVec` (an insertelement chain), so it flows through value positions like a scalar.
    /// `elem` is the numeric element scalar; the width is `elems.len()` (validated == N in sema).
    VecLit { elems: Vec<Expr>, elem: crate::Scalar },
    /// A fused array pipeline ending in `sum`: `source.map(f).where(p)….sum()`. The
    /// stages and the reduction lower to a single loop (no intermediate arrays).
    ArraySum { source: Box<Expr>, stages: Vec<Stage> },
    /// `source.….count()` — count the elements that survive the stages. Always `i64`;
    /// the element value is irrelevant, so no scalar projection is required.
    ArrayCount { source: Box<Expr>, stages: Vec<Stage> },
    /// `source.….any(p)` / `.all(p)` — whether the predicate `func` holds for any / all
    /// surviving (scalar) elements. Always `bool`; `all` selects an `&&`-fold over `||`.
    ArrayAnyAll { source: Box<Expr>, stages: Vec<Stage>, func: String, captures: Vec<Expr>, all: bool },
    /// `source.….min()` / `.max()` — the smallest / largest surviving (scalar, numeric)
    /// element. `is_max` selects max over min. Seeded with the element type's extreme, so an
    /// empty pipeline yields that extreme (the fold identity, as `sum` yields 0).
    ArrayMinMax { source: Box<Expr>, stages: Vec<Stage>, is_max: bool },
    /// `source.…​.reduce(init, f)` — fold the (post-stage) elements with the binary
    /// function `func` starting from `init`. `ty` is the accumulator type.
    ArrayReduce { source: Box<Expr>, stages: Vec<Stage>, func: String, captures: Vec<Expr>, init: Box<Expr> },
    /// `source.….scan(init, f)` — a *materializing* prefix fold: emit the running accumulator
    /// after each surviving element (`out[k] = acc` after `acc = f(acc, elem)`), starting from
    /// `init`. Yields an owned `array<A>` of survivor length. `elem` is the accumulator scalar
    /// (the output element type, `A`); `func` has type `(A, E) -> A`.
    ArrayScan { source: Box<Expr>, stages: Vec<Stage>, func: String, captures: Vec<Expr>, init: Box<Expr>, elem: crate::Ty },
    /// `a.dot(b)` — the inner product `Σ a[i]*b[i]` of two fixed-length arrays of the same
    /// numeric scalar element and the same (statically known) length. `elem` is that scalar;
    /// the result has type `elem`.
    ArrayDot { a: Box<Expr>, b: Box<Expr>, elem: crate::Ty },
    /// `source.….sort()` — materialize the surviving (numeric scalar) elements into an owned
    /// `array<T>` and sort them ascending in place. `elem` is the element scalar; the result
    /// type is `DynArray(elem)`.
    ArraySort { source: Box<Expr>, stages: Vec<Stage>, elem: crate::Ty },
    /// `source.….sort_by_key(f)` — materialize the surviving (primitive scalar) elements into an
    /// owned `array<T>` and sort them ascending by the key `f(element)` (`key_func`, type
    /// `(elem) -> key_ty`, an orderable scalar). `captures` are a lifted lambda's captured values.
    /// `elem` is the element scalar; the result type is `DynArray(elem)`.
    ArraySortBy { source: Box<Expr>, stages: Vec<Stage>, key_func: String, captures: Vec<Expr>, key_ty: crate::Ty, elem: crate::Ty },
    /// `source.….to_array()` — materialize the surviving (post-stage) elements into an
    /// *owned* `array<T>` (MMv2 slice 3: arena-bump-allocated). `elem` is the element
    /// scalar type; the expression `ty` is `DynArray(elem)`.
    ArrayToArray { source: Box<Expr>, stages: Vec<Stage>, elem: crate::Ty },
    /// `arr.to_soa()` — transpose an AoS struct array (`array<Struct>`) into a column-major
    /// `soa<Struct>` view, arena-bump-allocated (so it needs an arena; the view is region-tied to
    /// it and bulk-freed). `struct_id` indexes `Program::structs`; the expression `ty` is
    /// `Soa(struct_id)`. One fused loop reads each element and scatters its fields into their
    /// columns. The construction primitive that makes `soa<T>` usable in pure Align (it was
    /// parameter-only before): build once, then a multi-column scan touches only the fields it reads.
    ArrayToSoa { source: Box<Expr>, struct_id: u32 },
    /// `source.….partition(p)` — split the surviving (scalar) elements into two owned arrays by
    /// the predicate `func`: those satisfying it, then the rest. The expression `ty` is a tuple
    /// `(array<T>, array<T>)` (`Ty::Tuple`); `elem` is the element scalar. One fused loop fills
    /// both buffers (no intermediate array).
    ArrayPartition { source: Box<Expr>, stages: Vec<Stage>, func: String, captures: Vec<Expr>, elem: crate::Ty },
    /// `source.….par_map(f)` — apply the **Pure** function `func` to each (post-stage) element
    /// and materialize the results into an owned `array<R>` (`elem` = `R`). Semantically a
    /// data-parallel map; the first cut lowers to the sequential collect loop (`map(f)` +
    /// `to_array`), with real thread-parallel execution a runtime follow-up. `func` is required to
    /// be Pure (checked in the parallelism pass over the full call graph).
    ArrayParMap { source: Box<Expr>, stages: Vec<Stage>, func: String, captures: Vec<Expr>, elem: crate::Ty },
    /// `arr.chunks(n)` — split `source` (an array/slice of primitive `elem`) into sub-slices of
    /// length `n` (the last may be shorter), yielding an owned `array<slice<elem>>` whose elements
    /// borrow `source`. The unit of chunk parallelism (`draft.md` §11). `n` is an `i64`.
    ArrayChunks { source: Box<Expr>, n: Box<Expr>, elem: crate::Ty },
    /// Borrow an array (a local stack array) as a `slice<T>` view — `{ &arr[0], len }`.
    /// Allocation-free, so it is an implicit coercion at call sites.
    ArrayToSlice(Box<Expr>),
    /// `.len()` of a `str` or `slice<T>` (the `len` field of the `{ ptr, len }` view); the
    /// result is `i64`. A fixed array's length is a constant and lowers without this node.
    Len(Box<Expr>),
    /// `recv[index]` — element access into a scalar `array`/`slice`/owned `array<T>` (the result
    /// is the scalar element). Lowering emits a bounds check (`0 <= index < len`) that aborts on
    /// an out-of-range index (the settled panic model). `index` is an `i64`.
    Index { recv: Box<Expr>, index: Box<Expr> },
    /// `recv[start..end]` — a half-open range slice of a `str` / `array<T>` / `slice<T>`. The result
    /// is a borrowed view (`ty` = `str` for a `str` receiver, else `slice<T>`) into the receiver's
    /// storage — no allocation, region inherited from `recv` (it cannot outlive it). `start` defaults
    /// to `0` and `end` to the receiver's length when omitted (both `i64`). Lowering emits a bounds
    /// check (`0 <= start <= end <= len`) that aborts on an out-of-range slice (the panic model).
    SliceRange { recv: Box<Expr>, start: Option<Box<Expr>>, end: Option<Box<Expr>> },
    /// `recv[index].f0.f1…` — field access on an element of a struct array (`recv` is a fixed
    /// `array<Struct>` or an owned dynamic `array<Struct>`) with a *runtime* index, MMv2 slice 8f.
    /// `path` is the chain of field indices into the element struct (length ≥ 1); `struct_id`
    /// identifies the element struct (for the pointer-based dynamic-array load). A depth-1 `path`
    /// lowers to one bounds-checked element-field load; a nested `path` (`arr[i].a.x`) loads the
    /// first field's sub-struct, then projects the remainder. Distinct from [`IndexField`], which has
    /// a constant index and a slot-local base.
    ElemField { recv: Box<Expr>, index: Box<Expr>, path: Vec<u32>, struct_id: u32 },
    /// `template "..."` — build a `str` from static parts and interpolated holes. Each
    /// hole is a local (int or str); lowering picks the right builder write by its type.
    Template(Vec<TemplatePart>),
    /// `json.decode(input)` for struct `struct_id` — parse the `str` `input` into that
    /// struct at runtime. The expression `ty` is `Result<Struct, Error>`.
    JsonDecode { struct_id: u32, input: Box<Expr> },
    /// `json.decode(input)` targeting an owned `array<T>` (MMv2 slice 8c) — parse a JSON array of
    /// scalars into a freshly heap-allocated owned `array<T>` (the elements are *copied*, so the
    /// result is `Static`/returnable, not region-tied to the input). `elem` is the (primitive)
    /// element type; the expression `ty` is `Result<array<T>, Error>`.
    JsonDecodeArray { elem: crate::Ty, input: Box<Expr> },
    /// `json.decode(input)` targeting an owned `array<Struct>` (MMv2 slice 8d, draft.md §19) —
    /// parse a JSON array of objects into an owned, dynamic AoS of struct `struct_id`. `str`
    /// fields are zero-copy views into the input, so the array is region-tied to that input; the
    /// expression `ty` is `Result<array<Struct>, Error>`.
    JsonDecodeStructArray { struct_id: u32, input: Box<Expr> },
    /// `json.decode(input)` targeting a `soa<Struct>` (the cache-optimal decode) — parse a JSON
    /// array of objects **directly** into a column-major `soa<Struct>`, arena-allocated (the runtime
    /// `align_rt_json_decode_soa`: a structural count pass discovers N, then values are written
    /// straight into their columns — no AoS intermediate, no transpose; see #228).
    /// Fields must be primitive scalars (the `soa<T>` rule, so no `str` columns / input region tie),
    /// and it needs an enclosing `arena {}`. The expression `ty` is `Result<soa<Struct>, Error>`.
    JsonDecodeSoa { struct_id: u32, input: Box<Expr> },
    /// `s.group_by(.key).{sum,min,max}(.value)` / `.count()` over a `soa<Struct>` local `base` —
    /// column-oriented grouped aggregate. Reads the `key_field` column (and `value_field` for
    /// sum/min/max — `None` for `count`) as `slice<i64>` via [`SoaColumn`] and folds per distinct
    /// `key` into two parallel owned arrays. The expression `ty` is a tuple `(array<i64>, array<i64>)`
    /// (distinct keys, per-key aggregate) — the data-oriented result (no `HashMap`), reusing
    /// `partition`'s tuple-of-two-owned-arrays shape. First slice: `i64` key + `i64` value.
    ///
    /// The `source` selects the key/column path (see [`GroupSource`]): a `soa<Struct>` i64 key, an
    /// **AoS** `array<Struct>` `str` key (dictionary-encoded inline, `ty` = `(array<str>,
    /// array<i64>)`, key views **borrow `base`**), or a precomputed [`crate::Ty::DictEncoded`] value
    /// (reuse its dense-id column — the A2 rail). Value is `i64`; `sum`/`min`/`max`/`count`.
    ArrayGroupAgg { base: LocalId, struct_id: u32, key_field: u32, value_field: Option<u32>, op: GroupOp, source: GroupSource },
    /// `s.group_by(.key).agg(sum(.a), max(.b), count(), …)` — **fused multi-aggregate**: one pass over
    /// the key column computes every aggregate in `aggs` (in result order) into its own result column,
    /// instead of one `group_by` pass per aggregate. The `key`-once / K-accumulator shape that matches
    /// idiomatic fast Rust (`HashMap<K,[Acc;K]>`). The expression `ty` is a tuple
    /// `(array<K>, array<i64>, …)` — distinct keys followed by one `array<i64>` per aggregate. First
    /// cut: an AoS `str` key (`GroupSource::AosStr`), i64 values, `sum`/`min`/`max`/`count`.
    ArrayGroupAggMulti { base: LocalId, struct_id: u32, key_field: u32, aggs: Vec<GroupAgg1>, source: GroupSource },
    /// `s.dict_encode(.key)` — intern the `str` `key_field` column of the AoS `array<Struct>` local
    /// `base` to a dense-id column + a dictionary, yielding a [`crate::Ty::DictEncoded`] value. The
    /// one-time transform (visible cost) of the A2 reuse rail; a later `e.group_by(.key).<agg>(.value)`
    /// reuses the encoded ids (integer-column work) instead of re-interning per group-by. Borrows
    /// `base` (the `dict`/`source` slices view it), so the result is region-tied to it.
    ArrayDictEncode { base: LocalId, struct_id: u32, key_field: u32 },
    /// `fs.read_file(path)` — read the file at `path` (a `str`) into a freshly heap-allocated owned
    /// `string`; the expression `ty` is `Result<string, Error>`. The first `std.fs` surface.
    FsReadFile { path: Box<Expr> },
    /// `io.stdout.write(arg)` — write the bytes of the `str` `arg` to stdout with no trailing
    /// newline; the expression `ty` is `Result<(), Error>`. The first `std.io` surface.
    IoStdoutWrite { arg: Box<Expr> },
    /// `io.stdout.write(b)` where `b` is a `builder` — write the builder's accumulated bytes to
    /// stdout directly (no `to_string()` materialization), borrowing it (not consumed). `ty` is
    /// `Result<(), Error>`.
    IoStdoutWriteBuilder { builder: Box<Expr> },
    /// `io.stdout.buffered()` / `io.stderr.buffered()` — open a buffered writer over `fd` (1 =
    /// stdout, 2 = stderr), the sink-first fast path. The `ty` is [`crate::Ty::BufWriter`] (an owned,
    /// Move handle). Bytes accumulate in a buffer and reach the OS only when it fills or on `flush` /
    /// drop — so writes do no syscall and memory stays O(buffer). I/O-effecting (Impure), unlike the
    /// pure string `BuilderNew`.
    BufWriterNew { fd: i32 },
    /// `w.write(s)` — append a `str` to a buffered stdout writer, borrowing it (not consumed). The
    /// `ty` is `Unit`; an internal flush failure is latched and surfaced by the next `flush`.
    /// Impure (it may flush to stdout).
    BufWriterWrite { writer: Box<Expr>, arg: Box<Expr> },
    /// `w.flush()` — flush a buffered stdout writer's bytes to the OS, borrowing it. The `ty` is
    /// `Result<(), Error>`. Impure.
    BufWriterFlush { writer: Box<Expr> },
}

/// The source/key path of a column-oriented `group_by` ([`ExprKind::ArrayGroupAgg`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSource {
    /// `soa<Struct>`, contiguous columns, an **i64** key — the dense hash-aggregate path.
    SoaI64,
    /// AoS `array<Struct>`, a **str** key — interned to dense ids inline by the runtime, then
    /// aggregated and labeled (A1, `align_rt_group_*_str`).
    AosStr,
    /// A precomputed [`crate::Ty::DictEncoded`] value — reuse its dense-id column via the i64
    /// group path, then label results back through its dictionary (A2 reuse rail).
    Encoded,
}

/// One aggregate of a fused multi-aggregate `group_by` ([`ExprKind::ArrayGroupAggMulti`]): an op and
/// the i64 value field it folds (`None` for `count`, which reads no value column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupAgg1 {
    pub op: GroupOp,
    pub value_field: Option<u32>,
}

/// The aggregate of a column-oriented `group_by` ([`ExprKind::ArrayGroupAgg`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupOp {
    /// `sum(.value)` — per-group sum.
    Sum,
    /// `min(.value)` — per-group minimum.
    Min,
    /// `max(.value)` — per-group maximum.
    Max,
    /// `count()` — per-group row count (no value field).
    Count,
}

/// Which builder append a `BuilderWrite` performs (MMv2 slice 7c/7d).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuilderWriteKind {
    /// `b.write(s)` — append a `str`/`string` value's bytes.
    Str,
    /// `b.write_int(n)` — append a decimal integer.
    Int,
    /// `b.write_bool(v)` — append `true`/`false`.
    Bool,
    /// `b.write_char(c)` — append a `char`'s UTF-8 encoding.
    Char,
    /// `b.write_float(x)` — append an `f32`/`f64`'s shortest round-trip decimal.
    Float,
}

/// Which byte-oriented `str` predicate a `StrPredicate` tests (`core.string`). All three are
/// pure byte comparisons (UTF-8 is the representation, but the scan is byte-level) returning
/// `bool`; the standard runtime backs them with `memchr::memmem` / slice prefix-suffix checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrPredKind {
    /// `s.contains(needle)` — `needle`'s bytes occur somewhere in `s`.
    Contains,
    /// `s.starts_with(prefix)` — `s` begins with `prefix`'s bytes.
    StartsWith,
    /// `s.ends_with(suffix)` — `s` ends with `suffix`'s bytes.
    EndsWith,
    /// `s.find(needle)` — the byte index of `needle`'s first occurrence in `s`, as `Option<i64>`
    /// (`None` if absent). Unlike the others this yields `Option<i64>`, not `bool`; it is the index
    /// sibling of `contains` (`contains == find(..).is_some()`), now useful with range slicing.
    Find,
    /// `s.rfind(needle)` — the byte index of `needle`'s **last** occurrence in `s`, as `Option<i64>`
    /// (`None` if absent). The from-the-end sibling of `find` (e.g. `path.rfind(".")` for a suffix).
    Rfind,
    /// `s.eq_ignore_ascii_case(other)` — byte equality with ASCII letters compared case-insensitively
    /// (`bool`). For protocol/header parsing where case is insignificant; non-ASCII bytes compare
    /// exactly, so it is not Unicode case-folding (that stays package-level).
    EqIgnoreCase,
}

/// Which end(s) a `StrTrim` strips ASCII whitespace from (`core.string`). The result is a
/// borrowed sub-`str` of the receiver (no allocation) — UTF-8 stays the representation, but the
/// trim is byte-level over the WHATWG ASCII whitespace set (space, `\t`, `\n`, `\x0c`, `\r` — *not*
/// vertical tab `\x0b`, matching Rust's `[u8]::trim_ascii`); Unicode whitespace trimming is
/// deliberately package-level, not core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrTrimKind {
    /// `s.trim()` — strip leading and trailing ASCII whitespace.
    Both,
    /// `s.trim_start()` — strip leading ASCII whitespace.
    Start,
    /// `s.trim_end()` — strip trailing ASCII whitespace.
    End,
}

#[derive(Clone, Debug)]
pub enum TemplatePart {
    Text(String),
    /// `{expr}` — interpolate the value of an expression (a printable scalar).
    Hole(Expr),
    /// A `str` expression to be emitted as a JSON string literal (quoted + escaped).
    /// Produced by `json.encode` desugaring, not by surface `template` syntax.
    JsonStr(Expr),
}

#[derive(Clone, Debug)]
pub enum StageKind {
    /// `.map(f)` — transform each element with `func`. `captures` are extra arguments passed after
    /// the element (a lifted lambda's captured enclosing values; empty for a named function).
    Map { func: String, captures: Vec<Expr> },
    /// `.where(p)` — keep only elements for which the predicate `func` is true. `captures` as `Map`.
    Where { func: String, captures: Vec<Expr> },
    /// `.where(.field)` — keep only elements whose (bool) `field` is true.
    WhereField { field: u32 },
    /// `.field` — project a struct field out of each element (struct array → scalar).
    Project { field: u32 },
}

#[derive(Clone, Debug)]
pub struct Stage {
    pub kind: StageKind,
    /// The element type after this stage (for `Where`, unchanged from its input).
    pub out_ty: crate::Ty,
}
