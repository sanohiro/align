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

/// A resolved foreign-function declaration (`extern "C" fn name(params) -> ret`). Bodyless: it
/// carries only the C symbol and its FFI-safe signature types, which codegen turns into an external
/// LLVM declaration. A call to it lowers to an ordinary [`ExprKind::Call`] keyed by `name`.
#[derive(Clone, Debug)]
pub struct ExternFn {
    /// The literal C symbol (never mangled).
    pub name: String,
    pub params: Vec<crate::Ty>,
    /// The return type; [`crate::Ty::Unit`] for a `void` return.
    pub ret: crate::Ty,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Fn>,
    /// Foreign (C-ABI) function declarations, surfaced to codegen as external LLVM declarations.
    pub externs: Vec<ExternFn>,
    /// External libraries to link (`-l<name>`), from `extern "C" link("name")` clauses — deduped,
    /// in first-seen order. Consumed by the driver's link step (libc/libm are always linked and are
    /// not listed here).
    pub link_libs: Vec<String>,
    /// Struct definitions, indexed by the id carried in [`crate::Ty::Struct`].
    pub structs: Vec<StructDef>,
    /// Sum-type definitions, indexed by the id carried in [`crate::Ty::Enum`].
    pub enums: Vec<EnumDef>,
    /// Anonymous tuple types, indexed by the id carried in [`crate::Ty::Tuple`]. Interned
    /// (deduplicated by element list) during checking, so `(i64, i64)` is one entry.
    pub tuples: Vec<TupleDef>,
    /// Function-value types, indexed by the id carried in [`crate::Ty::Fn`]. Source annotations are
    /// interned by scalar signature; concrete values/locals receive distinct entries so their
    /// inferred effects do not contaminate unrelated functions with the same ABI.
    pub fn_types: Vec<FnTy>,
    /// M15 S2 (per-unit compilation): imported `pub` functions this unit *calls* but whose bodies
    /// live in another unit's object. Each is a bodyless declaration (its already-mangled
    /// `module$name` symbol + the signature resolved into this unit's type universe); codegen emits
    /// an external LLVM `declare` for each so a cross-unit `Rvalue::Call` resolves. **Empty in the
    /// whole-program build path** (every callee's body is in `fns`), so the default object stays
    /// byte-identical — populated only when checking a unit against interface-only dependencies.
    pub imported_fns: Vec<ImportedFn>,
}

/// A cross-unit `pub` function call target (M15 S2): the callee's mangled symbol plus its
/// signature, with no body. Codegen turns each into an external LLVM `declare` under the same
/// Align ABI a defining unit would emit for the function, so the linker resolves the call against
/// the unit that owns the definition. See [`Program::imported_fns`].
#[derive(Clone, Debug)]
pub struct ImportedFn {
    /// The already-mangled `module$name` symbol (collision-free across units).
    pub name: String,
    /// Parameter types, in declaration order (this unit's `Ty` universe).
    pub params: Vec<Ty>,
    /// The return type; [`crate::Ty::Unit`] for a `()` return.
    pub ret: Ty,
}

#[derive(Clone, Debug)]
pub struct FnTy {
    /// Parameter types (scalar-only for now).
    pub params: Vec<crate::Scalar>,
    /// Return type (a scalar).
    pub ret: crate::Scalar,
    /// Inferred observable effect of invoking a value of this type. This is internal type
    /// information: source annotations remain `fn(T) -> R`, while the checker refines the bit from
    /// each value's origin and conservatively joins mutable assignments. `Unknown` is fail-closed
    /// at Pure/parallel boundaries.
    pub effect: std::cell::Cell<crate::FnEffect>,
}

// Effects are inferred mutable facts, not part of source-level signature identity. Keeping them out
// of equality makes annotation interning stable after a concrete value has been refined. Concrete
// value types are deliberately allocated with `fresh_fn_type`, so equal signatures may still have
// independent effect cells.
impl PartialEq for FnTy {
    fn eq(&self, other: &Self) -> bool {
        self.params == other.params && self.ret == other.ret
    }
}

impl Eq for FnTy {}

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
    /// zero-copy interop), or `None` for the type's natural alignment. Populated from the parsed
    /// `align(N)` attribute; honored at the one storage-alignment seam (`type_align`).
    pub align: Option<u32>,
    /// Set by a `layout(C)` attribute: the struct has a stable, C-compatible flat byte layout
    /// (declaration-order fields, natural alignment, no reordering — which is Align's default
    /// layout, so the marker *locks* it and opts the struct into FFI). Only a `layout(C)` struct may
    /// be read/written through a `raw` pointer, because only it promises a fixed flat representation.
    pub c_repr: bool,
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
    /// Locals whose type owns resources and therefore needs a path-local MIR drop flag. The flag
    /// decides whether the value currently in the slot is individually owned or arena-owned;
    /// moved/uninitialised slots also keep it clear. Populated after move/escape analysis.
    pub drop_locals: Vec<LocalId>,
    /// Subset of [`Self::drop_locals`] whose declaration initializer is individually owned. MIR
    /// sets these locals' runtime flags after their `let`; the remaining initial values are
    /// arena-owned and leave the flag clear.
    pub drop_individual_locals: Vec<LocalId>,
    /// Static allocation provenance for owned expressions, keyed by their source span. Control-flow
    /// lowering uses the per-branch facts to build a runtime ownership join; direct local moves
    /// still forward their path-local flag instead of consulting this conservative table.
    pub drop_individual_exprs: std::collections::HashMap<Span, bool>,
    /// M15 S2: this is a non-entry, non-generic `pub` **user** function — a candidate external
    /// symbol under separate compilation (its `module$name` mangling is globally collision-free).
    /// The *fact* is mode-independent; whether it actually gets `external` linkage is decided at
    /// lowering (whole-program lowering forces every function `internal` for byte-identity; per-unit
    /// lowering honors this bit). `false` for the entry unit's functions (nothing imports the
    /// entry), monomorphs, and lifted lambdas.
    pub exportable: bool,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub id: LocalId,
    pub name: String,
    pub ty: Ty,
    pub is_mut: bool,
    /// Whether this local is a real function **parameter** (declared in the signature), as opposed
    /// to a `let` binding or a lambda capture. Used by `map_into`'s alias gate: a slice parameter's
    /// buffer is distinct from the other arguments by the caller's `out` no-alias contract, whereas
    /// a slice `let`-bound to a value of unknown origin (a fn-returned slice, a soa column, a
    /// struct-field slice) could alias anything and cannot back a `noalias` claim.
    pub is_param: bool,
    /// A declared over-alignment (bytes, a validated power of two) from an `align(N) data := [...]`
    /// binding, or `None` for the value's natural alignment. Set only for a scalar fixed-array
    /// binding; propagated to the MIR slot's alloca alignment (the aligned-vector-load enabler).
    pub align: Option<u32>,
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
    /// `drop_new` (set by escape analysis) records whether the replacement is individually owned;
    /// MIR writes it into the local's runtime drop flag after the store.
    Assign {
        local: LocalId,
        value: Expr,
        drop_old: std::cell::Cell<bool>,
        drop_new: std::cell::Cell<bool>,
    },
    /// `base[index] = value` — element store into a `mut` array local or `out` slice parameter.
    /// Lowering emits a bounds check (abort on out-of-range), like an element read.
    AssignIndex { base: LocalId, index: Expr, value: Expr },
    /// `v[lane] = value` — write one lane of a `mut vecN<T>` local (M6): `v = insertelement(v,
    /// value, lane)`. `lane` is a constant in `0..N`.
    AssignVecLane { local: LocalId, lane: u32, value: Expr },
    /// `root.f0.f1.… = value` — store into a (possibly nested) field of a struct local. `path` is
    /// the chain of field indices (length ≥ 1).
    AssignField { root: LocalId, path: Vec<u32>, value: Expr },
    /// `base[index].f0.f1.… = value` — store the leaf field reached by `path` (length ≥ 1) of
    /// element `index` of a struct-array or soa local (the write counterpart of the
    /// `base[index].f0.f1.…` read). `soa` picks the lowering: a column store (`StoreColumn`) for a
    /// `soa<Struct>` (scalar columns ⇒ path length 1), else a slot element-field store
    /// (`StoreElemField`, fixed `array<Struct>`) or a pointer-based store (`StoreElemFieldPtr`, owned
    /// dynamic `array<Struct>`). Lowering emits a bounds check. Each non-final path segment is a
    /// nested struct; the leaf is a scalar (or, for a fixed array, an owned `string` with drop-of-old).
    AssignElemField { base: LocalId, index: Expr, path: Vec<u32>, struct_id: u32, soa: bool, value: Expr },
    /// `base[index] = value` — store a whole struct value into element `index` (the write
    /// counterpart of the `base[index]` whole-element read / `s[i]` gather). `soa` picks the
    /// lowering: a per-column scatter (`StoreColumn` per field) for a `soa<Struct>`, else a single
    /// aggregate slot store (`StoreIndex`) for a fixed `array<Struct>`. First cut: the struct is
    /// plain-old-data (flat primitive-scalar fields), so the value is Copy — no region/move/drop.
    AssignElem { base: LocalId, index: Expr, struct_id: u32, soa: bool, value: Expr },
    Return(Option<Expr>),
    /// `break expr` — end the innermost enclosing `loop`, yielding `expr` (a bare `break` yields
    /// `()`). Diverges (control leaves to the loop's exit); the only loop exit.
    Break(Option<Expr>),
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
    /// `base[index].field.sub…` — read the field `path` of element `index` of a struct-array local.
    /// Used by `json.encode` over a fixed struct array (unrolled; `index` is a constant). `path` is
    /// the chain of logical field indices (length ≥ 1); a length > 1 descends through nested-struct
    /// element fields (encode recurses into them). Lowers to the GEP `[0, index, *path]`.
    IndexField {
        base: LocalId,
        index: u32,
        path: Vec<u32>,
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
    /// `loop { ... }` — the one sequential-control construct. Repeats `body` until a `break`; the
    /// loop's value is the (unified) break value. `diverges` is true when the loop has no `break`
    /// (it never yields — like a `match` whose arms all diverge), so it satisfies any expected type
    /// and control never reaches the code after it. The body's trailing value is discarded each
    /// iteration; per-iteration owned locals drop at each pass (handled in MIR lowering).
    ///
    /// `body_locals` is the half-open `LocalId` range of every local **declared inside** `body` —
    /// recorded by sema as the `self.locals` growth across checking the body, so it captures locals
    /// nested at any depth / any expression position (a `let` inside a block that is itself a call
    /// argument, tuple element, `if`/`match` arm, …) without a fragile per-`ExprKind` walk, and
    /// excludes lifted-lambda locals (a lambda swaps its own `locals` in/out). MIR intersects it with
    /// `drop_locals` to drop the loop's per-iteration owned locals each pass.
    Loop { body: Block, diverges: bool, body_locals: std::ops::Range<LocalId> },
    /// `arena { ... }` — a region; allocations inside are bulk-freed at block end.
    Arena(Block),
    /// `unsafe { ... }` — a marker block permitting `raw.*` ops. No runtime effect; lowers to its
    /// inner block. (Enforcement + impurity are handled in sema.)
    Unsafe(Block),
    /// `raw.alloc(size)` — allocate `size` bytes on the flat heap, yielding a `raw` byte pointer.
    /// `unsafe`-only; the caller owns the memory and must `raw.free` it (no auto-drop).
    RawAlloc(Box<Expr>),
    /// `raw.free(p)` — free a `raw` pointer previously returned by `raw.alloc`. `unsafe`-only.
    RawFree(Box<Expr>),
    /// `raw.load(p, offset)` — read a primitive `scalar` value at byte `offset` from `p`. `unsafe`-only.
    RawLoad { ptr: Box<Expr>, offset: Box<Expr>, scalar: crate::Scalar },
    /// `raw.store(p, offset, v)` — write the primitive scalar `value` at byte `offset` of `p`. Yields
    /// unit. `unsafe`-only.
    RawStore { ptr: Box<Expr>, offset: Box<Expr>, value: Box<Expr> },
    /// `raw.offset(p, n)` — advance a `raw` pointer by `n` bytes, yielding a new `raw`. `unsafe`-only.
    RawOffset { ptr: Box<Expr>, offset: Box<Expr> },
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
    /// `pooled` (doc-13 §8.4, S3) is set only at a qualifying `let` binding whose every element
    /// folds to a compile-time constant scalar: the local stays a fixed `Ty::Array` (Copy — the
    /// type is unchanged), but MIR lowers the binding to a single memcpy from a per-unit read-only
    /// global (the #514 rodata mechanism) instead of `n` element stores. Read-only bindings then
    /// collapse to a direct rodata read (LLVM eliminates the alloca+memcpy). `false` everywhere
    /// else — a temporary, a `mut`/`align(N)` binding, a runtime-valued element, or a `str`/struct
    /// element keeps the ordinary per-element store path.
    ArrayLit { elems: Vec<Expr>, elem: crate::Ty, pooled: bool },
    /// A substituted top-level **aggregate constant** — the array-literal analogue of
    /// [`ExprKind::Str`]. `NAME := [1, 2, 3]` folds to a `ConstVal::Array` and is substituted at
    /// each use as this node, typed `slice<elem>` with `Region::Static`: the elements live in a
    /// per-unit private read-only global (rodata), and the value is a borrowed `{ptr, len}` view of
    /// it (exactly like a `str` literal is a static view of its bytes — ownership is a property of
    /// the type, so a top-level array constant is a `slice<T>`, never an owned `array<T>`). `elems`
    /// are the already-folded scalar/str literal elements; `elem` is their shared scalar type and
    /// `len` is the fixed length. Carries no borrow provenance and is never moved/dropped.
    ConstArray { elems: Vec<Expr>, elem: crate::Scalar, len: u32 },
    /// `zip(a, b, …)` — a lazy multi-source pipeline head. Each source is an array/slice of a
    /// Copy scalar; `tuple_id` describes the per-index SSA tuple. There is no tuple array and this
    /// node is valid only as the `source` of a pipeline terminal.
    ArrayZip { sources: Vec<Expr>, tuple_id: u32 },
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
    /// `source.….map_into(dst)` — a **materializing terminal that writes into a caller-provided
    /// `out`/`mut` slice `dst`** instead of allocating a fresh buffer (the `to_array` sibling that
    /// reuses caller storage — `draft.md` §7's `out` parameter as a pipeline terminal). One fused
    /// counted loop stores `dst[i] = f(source[i])` for length-preserving stages only (v1 rejects
    /// `where`); the runtime requires `dst.len() == source.len()` (abort otherwise). `elem` is the
    /// element scalar. The expression `ty` is `Unit`. Sema proves `dst` does not alias the source
    /// (the soundness precondition for the LLVM scoped-`noalias` metadata codegen emits on this
    /// loop's load/store — the disjoint-buffer claim that lets the vectorizer drop its runtime
    /// overlap guard).
    ArrayMapInto { source: Box<Expr>, stages: Vec<Stage>, dst: Box<Expr>, elem: crate::Ty },
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
    /// `io.stdin` — a `reader` over fd 0. The `ty` is [`crate::Ty::Reader`] (an owned Move handle;
    /// its fd is borrowed, not closed on `Drop`). Constructing it is allocation only (pure), like
    /// `BuilderNew`; the *reads* are what is Impure.
    ReaderStdin,
    /// `fs.open(path)` — open `path` (a `str`) for reading; the `ty` is `Result<reader, Error>`. The
    /// returned `reader` owns its fd (closed on `Drop`). Impure (touches the filesystem).
    ReaderOpen { path: Box<Expr> },
    /// `io.stdout` / `io.stderr` / `io.stdout.buffered()` — a `writer` over a standard-stream fd
    /// (`fd`: 1 = stdout, 2 = stderr), `buffered` selecting the O(buffer) accumulator ("one type,
    /// many constructors"). The `ty` is [`crate::Ty::Writer`] (an owned Move handle; its fd is
    /// borrowed, not closed). Constructing it is allocation only (pure); the *writes* are Impure.
    WriterStd { fd: i32, buffered: bool },
    /// `fs.create(path)` — create/truncate `path` (a `str`) for writing; the `ty` is
    /// `Result<writer, Error>`. The returned `writer` owns its fd (flushed + closed on `Drop`).
    /// Impure.
    WriterCreate { path: Box<Expr> },
    /// `r.read(b: mut buffer)` — read up to `b`'s capacity into `b` (overwriting its length),
    /// borrowing both `reader` and `buffer` (neither consumed). The `ty` is `Result<i64, Error>`
    /// (bytes read; `0` = EOF). Impure.
    ReaderRead { reader: Box<Expr>, buffer: Box<Expr> },
    /// `r.buffered()` — upgrade a `reader` to carry a lookahead (the read dual of the buffered
    /// *writer*), **consuming** (moving) the source reader and yielding a buffered `reader` over the
    /// same fd. The `ty` is [`crate::Ty::Reader`]. Pure (allocation only — the lookahead buffer, no
    /// I/O), like `BufferNew`. Its region inherits the source reader's (a `c.reader().buffered()`
    /// stays conn-bound). Only a buffered reader may `read_line`.
    ReaderBuffered { reader: Box<Expr> },
    /// `r.read_line(b: mut buffer)` — read the next line into `b` with its `\r?\n` terminator
    /// stripped (`b.len()` = body length), borrowing both `reader` and `buffer` (neither consumed;
    /// the buffer is filled in place and **grows** as needed). The `ty` is `Result<i64, Error>`:
    /// bytes consumed including the terminator, `0` = EOF (`< 0` errors are folded into `Err` in
    /// lowering, the `reader.read` sign convention). Requires a **buffered** receiver (sema-enforced).
    /// Impure. A buffer-mutating op (registered like `BufferAppend` in every analysis pass).
    ReaderReadLine { reader: Box<Expr>, buffer: Box<Expr> },
    /// `bytes.as_str()` — the validating VIEW at the bytes→text boundary (the view sibling of
    /// `bytes.to_string()`): checks `bytes` is valid UTF-8 and yields a zero-copy `str` view of the
    /// same storage. The `ty` is `Result<str, Error>` (`Error.Invalid` on invalid UTF-8). The `str`
    /// view is region-bound through the receiver (`region_of` inherits `bytes`'s region — a view of
    /// `buf.bytes()` stays pinned to the buffer). Pure (a memcpy-class validation, no I/O). `bytes`
    /// is borrowed, not consumed. Works on any `bytes` (`slice<u8>`) value.
    BytesAsStr { bytes: Box<Expr> },
    /// `w.write(x)` — append a `str`/`bytes` (`slice<u8>`) value or a `builder`'s bytes to a
    /// `writer`, borrowing it (not consumed). `builder` marks the builder-source form (its bytes are
    /// written directly). The `ty` is `Result<(), Error>`. Impure.
    WriterWrite { writer: Box<Expr>, arg: Box<Expr>, builder: bool },
    /// `w.flush()` — flush a `writer`'s buffered bytes to the OS, borrowing it. The `ty` is
    /// `Result<(), Error>`. Impure.
    WriterFlush { writer: Box<Expr> },
    /// `io.copy(r: reader, w: writer)` — stream all of `r` into `w` through a fixed-size buffer
    /// (memory is O(buffer), never O(file size)), borrowing **both** (neither consumed — the fd
    /// ownership does not move, so `r`/`w` remain usable after the call, like `print`'s argument).
    /// The `ty` is `Result<i64, Error>` (bytes transferred). Impure. v1 is the portable fixed-buffer
    /// loop; `sendfile`/`splice` fast paths stay post-M9 (`open-questions.md` "Transparent
    /// zero-copy I/O"), added without changing this node.
    IoCopy { reader: Box<Expr>, writer: Box<Expr> },
    /// `fs.create_rw(path)` — open `path` (a `str`) `O_RDWR|O_CREAT|O_TRUNC` for offset-addressed
    /// block I/O; the `ty` is `Result<file, Error>`. The returned `file` owns its fd (closed on
    /// `Drop`). Impure (touches the filesystem). (align-LLM runway A4.)
    FileCreateRw { path: Box<Expr> },
    /// `fs.open_rw(path)` — open an existing `path` (a `str`) `O_RDWR` for in-place update; the `ty`
    /// is `Result<file, Error>`. The returned `file` owns its fd (closed on `Drop`). Impure. (A4.)
    FileOpenRw { path: Box<Expr> },
    /// `f.pread(b: mut buffer, off)` — one positionless read at file offset `off` into `b`
    /// (overwriting its length), borrowing both `file` and `buffer` (neither consumed). The `ty` is
    /// `Result<i64, Error>` (actual bytes read; `0` = EOF). Impure. A negative `off` aborts. (A4.)
    FilePread { file: Box<Expr>, buffer: Box<Expr>, offset: Box<Expr> },
    /// `f.pwrite(data, off)` — write **all** of `data` (a `bytes` view) at file offset `off`,
    /// borrowing `file` (not consumed). The `ty` is `Result<i64, Error>` (the full byte count).
    /// Impure. A negative `off` aborts; a write past EOF extends the file. (A4.)
    FilePwrite { file: Box<Expr>, data: Box<Expr>, offset: Box<Expr> },
    /// `f.len()` — the file's live byte length (a fresh `fstat`), borrowing `file`. The `ty` is
    /// `Result<i64, Error>`. Impure (a syscall — the length is not cached). (A4.)
    FileLen { file: Box<Expr> },
    /// `buffer(cap)` — open an owned growable byte buffer with read window `cap` (a `str`-less byte
    /// sink for `reader.read`). The `ty` is [`crate::Ty::Buffer`] (an owned Move handle, `Drop`-freed).
    /// Pure (allocation only), like `BuilderNew`.
    BufferNew { capacity: Box<Expr> },
    /// `b.bytes()` — a `slice<u8>` view of the buffer's current contents. Borrows the buffer
    /// (region-tracked: the view must not outlive `b`). Pure.
    BufferBytes { buffer: Box<Expr> },
    /// `s.bytes()` — a zero-cost `slice<u8>` view of a `str`'s UTF-8 bytes. An owned `string`
    /// receiver is first wrapped in `StrBorrow`, so this node always borrows a `str` and inherits
    /// its storage region. Pure; the `{ptr,len}` representation is unchanged in MIR.
    StrBytes { inner: Box<Expr> },
    /// `b.len()` — the buffer's current byte count (an `i64`). Pure.
    BufferLen { buffer: Box<Expr> },
    /// `bytes.<scalar>_<le|be>(off)` — a bounds-checked binary scalar **read** from a `bytes`
    /// (`slice<u8>`) view at byte offset `off`. The `ty` is the read scalar (`u8`/`i8`/…/`f64`);
    /// its width (1/2/4/8 bytes) comes from `ty`. `be` selects big-endian byte order (single-byte
    /// reads carry `be:false`). Out-of-range (`off < 0` or `off + width > len`) **aborts** — the
    /// same policy as `slice[i]` (a structural over-read is a bug; a parser checks `.len()` first).
    /// Pure (a memory read, like an index). The `bytes` view and `off` are borrowed, not consumed.
    BytesRead { bytes: Box<Expr>, offset: Box<Expr>, be: bool },
    /// `buf.put_<scalar>_<le|be>(v)` — append `v`'s bytes to a growable `buffer` in the given byte
    /// order, growing it (the encode dual of [`BytesRead`]). `v`'s scalar type sets the width; `be`
    /// selects big-endian. The `ty` is [`crate::Ty::Unit`]. The receiver must be a `mut buffer`
    /// local (mutated in place). Pure (in-memory growth, no I/O — like a `mut` array store).
    BufferPut { buffer: Box<Expr>, value: Box<Expr>, be: bool },
    /// `buf.append(data)` — append a raw `bytes` (`slice<u8>`) blob to a growable `buffer`, copying
    /// the bytes in and growing it. The `ty` is [`crate::Ty::Unit`]. The receiver must be a `mut
    /// buffer` local; `data` is borrowed (copied, not consumed). Pure (in-memory growth).
    BufferAppend { buffer: Box<Expr>, data: Box<Expr> },
    /// `array_builder()` — open an empty growable typed array builder (M12 A6). The `ty` is
    /// [`crate::Ty::ArrayBuilder`] (an owned Move handle, `Drop`-freed); the element type is carried
    /// by that `ty`. Pure (allocation only), like `BuilderNew`/`BufferNew`.
    ArrayBuilderNew { elem: crate::Scalar },
    /// `b.push(v)` — append one element to a growable `array_builder`, growing it (amortized). The
    /// `ty` is [`crate::Ty::Unit`]. The receiver must be a `mut array_builder` local (mutated in
    /// place). `moves_value` is set for a `string` element: `v` is **moved** into the builder (its
    /// source is nulled), so MoveCheck consumes it; a Copy-scalar element borrows `v`. Pure (growth).
    ArrayBuilderPush { builder: Box<Expr>, value: Box<Expr>, moves_value: bool },
    /// `b.append(xs)` — bulk-append a `slice<T>` of Copy-scalar elements to a growable
    /// `array_builder`, copying them in and growing it. The `ty` is [`crate::Ty::Unit`]. The receiver
    /// must be a `mut array_builder` local; `data` is borrowed (copied, not consumed). Pure (growth).
    ArrayBuilderAppend { builder: Box<Expr>, data: Box<Expr> },
    /// `b.build()` — freeze an `array_builder` into an owned `array<T>`, **consuming** (moving) the
    /// builder (a zero-copy ptr+len retype). The `ty` is [`crate::Ty::DynArray`] of the element.
    ArrayBuilderBuild(Box<Expr>),
    /// `fs.write_file(path, data)` — create/truncate `path` (a `str`) and write all of `data`, then
    /// close. `data` is a `str`/`bytes` (`slice<u8>`) view, or — when `builder` is set — a `builder`'s
    /// accumulated bytes (borrowed, not consumed). The `ty` is `Result<(), Error>`. Impure.
    FsWriteFile { path: Box<Expr>, data: Box<Expr>, builder: bool },
    /// `fs.exists(path)` — whether `path` exists. Every error (stat failure) folds to `false`, so the
    /// `ty` is [`crate::Ty::Bool`], never a `Result` (`draft.md` §18.2). Impure (touches the filesystem).
    FsExists { path: Box<Expr> },
    /// `fs.remove(path)` — delete the file at `path`. The `ty` is `Result<(), Error>`. Impure.
    FsRemove { path: Box<Expr> },
    /// `fs.read_dir(path)` — the entry names of directory `path` as a freshly heap-allocated owned
    /// `array<string>` (each element owns its buffer; a **deep** `Drop`). The `ty` is
    /// `Result<array<string>, Error>`. Owned/returnable (borrows nothing). Impure.
    FsReadDir { path: Box<Expr> },
    /// `dns.resolve(host)` (`std.net`) — resolve `host` to its IP-address strings via `getaddrinfo`,
    /// as a freshly heap-allocated owned `array<string>` (each element owns its buffer; a **deep**
    /// `Drop`, identical to [`FsReadDir`]). The `ty` is `Result<array<string>, Error>`.
    /// Owned/returnable (borrows nothing). Impure (a name-resolution syscall).
    DnsResolve { host: Box<Expr> },
    /// `tcp.connect(host, port)` (`std.net`) — resolve `host` (via `getaddrinfo`) and open a TCP
    /// connection to `port`, trying each resolved address in order. The `ty` is
    /// `Result<tcp_conn, Error>` (an owned Move handle owning the connected socket fd; `Drop` closes
    /// it). `host` is a borrowed `str` (never consumed), `port` an `i64` (Copy). SO_KEEPALIVE is set
    /// on success. Impure (a network syscall).
    TcpConnect { host: Box<Expr>, port: Box<Expr> },
    /// `c.reader()` (`std.net`) — borrow an M9 `reader` over the `tcp_conn` `conn`'s socket fd
    /// (`owns_fd: false`; only the conn's `Drop` closes it). The `ty` is [`crate::Ty::Reader`], its
    /// region bound to `conn` (see `region_of`). `conn` is borrowed (never consumed).
    ConnReader { conn: Box<Expr> },
    /// `c.writer()` (`std.net`) — borrow an M9 (unbuffered) `writer` over the `tcp_conn` `conn`'s
    /// socket fd (`owns_fd: false`; only the conn's `Drop` closes it). The `ty` is
    /// [`crate::Ty::Writer`], its region bound to `conn`. `conn` is borrowed (never consumed).
    ConnWriter { conn: Box<Expr> },
    /// `tcp.listen(host, port)` (`std.net`) — resolve `host` (via `getaddrinfo` with `AI_PASSIVE`; a
    /// null/empty host binds the wildcard address) and open a listening TCP socket bound to `port`
    /// (`SO_REUSEADDR` set before `bind`, then `listen` with a fixed backlog). The `ty` is
    /// `Result<tcp_listener, Error>` (an owned Move handle owning the listening socket fd; `Drop`
    /// closes it). `host` is a borrowed `str` (never consumed), `port` an `i64` (Copy). Impure (a
    /// network syscall).
    TcpListen { host: Box<Expr>, port: Box<Expr> },
    /// `l.accept()` (`std.net`) — block until an inbound connection arrives on the `tcp_listener`
    /// `listener`, returning a new **owned** `tcp_conn` (the Slice-2 type — its reader/writer/`Drop`
    /// all just work). The `ty` is `Result<tcp_conn, Error>`. `EINTR` on `accept` is retried (accept
    /// loops are the common case), unlike `connect`. `listener` is borrowed (never consumed).
    TcpAccept { listener: Box<Expr> },
    /// `udp.bind(host, port)` (`std.net`) — resolve `host` (via `getaddrinfo` with `AI_PASSIVE`; a
    /// null/empty host binds the wildcard address) and open a `SOCK_DGRAM` (UDP) socket bound to
    /// `port`. The `ty` is `Result<udp_socket, Error>` (an owned Move handle owning the socket fd;
    /// `Drop` closes it). `host` is a borrowed `str` (never consumed), `port` an `i64` (Copy). Impure
    /// (a network syscall).
    UdpBind { host: Box<Expr>, port: Box<Expr> },
    /// `u.send_to(data, host, port)` (`std.net`) — resolve `host`/`port` (per call, `SOCK_DGRAM`) and
    /// `sendto` the byte view `data` as one datagram from the `udp_socket` `sock`'s fd. The `ty` is
    /// `Result<i64, Error>` (the number of bytes actually sent). `EINTR` is retried (a datagram send
    /// is atomic). `sock` is borrowed (never consumed); `data` is a borrowed byte view; `host` a
    /// borrowed `str`; `port` an `i64`. Impure.
    UdpSendTo { sock: Box<Expr>, data: Box<Expr>, host: Box<Expr>, port: Box<Expr> },
    /// `u.recv_from(buf)` (`std.net`) — block for one inbound datagram on the `udp_socket` `sock`,
    /// filling the caller's `buffer` `buffer` up to its capacity (overwriting its length) and yielding
    /// `Result<i64, Error>` (the number of bytes received). `EINTR` is retried (a blocking wait, the
    /// `accept` rationale). A datagram larger than the buffer is truncated (the excess is discarded by
    /// the kernel — `recvfrom` semantics), with the count being what fit. The peer address is **not**
    /// returned in v1 (deferred — see `check_udp_recv_from`). `sock` and `buffer` are both borrowed.
    UdpRecvFrom { sock: Box<Expr>, buffer: Box<Expr> },
    /// `fs.read_file_view(path)` — mmap the regular file at `path` read-only into the enclosing arena,
    /// yielding a `str` view of its bytes. Requires an enclosing `arena {}` (like `heap.new`); the
    /// region is bound to the arena, and `munmap` runs at arena end. The `ty` is `Result<str, Error>`.
    /// Escapes the region via `.clone()`. Impure.
    FsReadFileView { path: Box<Expr> },
    /// `fs.read_bytes_view(path)` — the **binary sibling** of [`Self::FsReadFileView`]: the same
    /// arena-scoped `mmap` (regular-file fast path, owned-copy fallback for special / zero-length
    /// files, `munmap` at arena end) minus the UTF-8 validation, yielding a `bytes` (`slice<u8>`)
    /// view of the raw file bytes. This is the way to zero-copy `mmap` a binary asset (a GGUF model
    /// file, a packed index) — content a `str` view would reject. Requires an enclosing `arena {}`;
    /// the view is region-bound to the arena and cannot escape it. The `ty` is
    /// `Result<slice<u8>, Error>`. Impure.
    FsReadBytesView { path: Box<Expr> },
    /// `path.join(a, b)` — join two path fragments with a single `/` separator into a freshly
    /// heap-allocated owned `string` (the `ty` is [`crate::Ty::String`]). Pure string manipulation
    /// (no OS access); the separator is collapsed at the boundary (`a`'s trailing `/` and `b`'s
    /// leading `/` fold to one). An empty fragment yields a clone of the other.
    PathJoin { a: Box<Expr>, b: Box<Expr> },
    /// `path.base(p)` / `path.dir(p)` / `path.ext(p)` — a **zero-copy substring `str` view** of `p`
    /// (the `ty` is [`crate::Ty::Str`]); `kind` selects which component. The result aliases `p`'s
    /// bytes, so its region is **inherited from `p`** (`region_of`) — it must not outlive `p`. Pure.
    PathComponent { kind: PathComponentKind, path: Box<Expr> },
    /// `path.normalize(p)` — lexically resolve `.` / `..` / redundant `/` into a freshly
    /// heap-allocated owned `string` (the `ty` is [`crate::Ty::String`]). POSIX vocabulary only, a
    /// pure string operation — no symlink resolution, no filesystem access. Pure.
    PathNormalize { path: Box<Expr> },
    /// `env.get(name)` — the value of environment variable `name` as a freshly heap-allocated owned
    /// `string`, or `None` if unset (the `ty` is `Option<string>`; the string is owned because the
    /// environment is volatile — a view would dangle after a later `env.set`). Impure (reads process
    /// environment).
    EnvGet { name: Box<Expr> },
    /// `env.set(name, value)` — set environment variable `name` to `value` (the `ty` is
    /// `Result<(), Error>`). Impure. v1 is a plain `setenv`; concurrent `env.set` from multiple tasks
    /// is **undefined** (POSIX — `setenv` is not thread-safe), documented, not enforced.
    EnvSet { name: Box<Expr>, value: Box<Expr> },
    /// `time.now()` — wall-clock time as UNIX-epoch nanoseconds (`CLOCK_REALTIME`), an `i64` (the
    /// `ty` is [`crate::Ty::Int`] i64). Impure (observes the clock).
    TimeNow,
    /// `time.instant()` — a monotonic-clock reading in nanoseconds (`CLOCK_MONOTONIC`), an `i64`.
    /// Impure.
    TimeInstant,
    /// `time.sleep(ns)` — suspend the calling thread for `ns` nanoseconds (the `ty` is
    /// [`crate::Ty::Unit`]). A negative `ns` is a no-op; `EINTR` resumes for the remaining time.
    /// Impure.
    TimeSleep { ns: Box<Expr> },
    /// `process.exit(code)` — run the current function's pending cleanup (Drops for live owned
    /// locals + arena ends + buffered-writer flushes, the exact emission a top-level `return` uses),
    /// THEN call libc `exit(code)`. The settled cleanup-then-exit semantics
    /// (`docs/impl/std-design/process.md`): the default hard-exit is the *safe* one — no silently
    /// lost buffered output. Impure; diverges. **v1 gap:** only the current frame's cleanup runs —
    /// full multi-frame stack unwind is the documented ideal, deferred. There is no `Never` type
    /// yet, so the `ty` is [`crate::Ty::Unit`]; `process.exit` therefore cannot sit in the tail
    /// position of a non-unit-returning function (use it as a statement).
    ProcessExit { code: Box<Expr> },
    /// `process.abort()` — the named-dangerous escape hatch: immediate `_exit`, running NO cleanup
    /// (no Drops / flushes / atexit). The asymmetric counterpart to `process.exit`. Impure; diverges.
    /// The `ty` is [`crate::Ty::Unit`] (no `Never` type yet).
    ProcessAbort,
    /// `process.spawn(cmd, args)` (`std.process`) — `fork` + `execvp(cmd, argv)` in the child. `cmd`
    /// is the borrowed `str` lookup path (resolved via `PATH`); `args` is the borrowed `array<str>`
    /// that becomes the child's **full** `argv` — the caller supplies `argv[0]` (P5). The `ty` is
    /// `Result<child, Error>` (an owned Move handle owning the child's pid; `Drop` reaps it via a
    /// blocking `waitpid`). A `fork` failure surfaces as `Err(errno)`; an `execvp` failure cannot be
    /// reported synchronously — the forked child `_exit(127)`s (the shell convention), so an
    /// exec-not-found surfaces later as `wait() == 127`. Impure. Both `cmd` and `args` are borrowed
    /// (never consumed).
    ProcessSpawn { cmd: Box<Expr>, args: Box<Expr> },
    /// `ch.wait()` (`std.process`) — block in `waitpid` for the `child` to exit, returning its
    /// exit code as `Result<i64, Error>`: a normal exit yields `WEXITSTATUS` (`0..=255`); a
    /// signal-killed child yields `128 + signal` (the shell convention). Marks the child **reaped**
    /// (through the borrow — the receiver is read, not consumed, so the later `Drop` becomes a no-op);
    /// a second `wait()` on an already-reaped child is `Err` (a clean status, not an `ECHILD` race).
    /// `child` is borrowed (never consumed — mirrors `l.accept()`). Impure.
    ChildWait { child: Box<Expr> },
    /// `ch.kill(sig)` (`std.process`) — send signal `sig` (an `i64`) to the `child` via libc `kill`,
    /// returning `Result<(), Error>`. Like [`ChildWait`], `child` is **borrowed** (never consumed); the
    /// already-`reaped` flag guards against signalling a recycled pid (killing a reaped child is a clean
    /// `Err`, not a stray signal to an unrelated process). `sig == 0` is the standard liveness probe (no
    /// signal sent, just an existence/permission check); a negative or out-of-range `sig` is
    /// `Error.Invalid`; `EPERM`/`ESRCH` surface via the shared errno table. Impure.
    ChildKill { child: Box<Expr>, sig: Box<Expr> },
    /// `process.exec(cmd, args)` (`std.process`) — `execvp(cmd, argv)` **in the current process**: on
    /// success it REPLACES the process image and never returns, so the `ty` `Result<(), Error>` is only
    /// ever observed as its `Err` arm (an `execvp` failure — the mapped errno). `cmd` is the borrowed
    /// `str` lookup path (resolved via `PATH`); `args` is the borrowed `array<str>` / `slice<str>` that
    /// becomes the new image's **full** `argv` (the caller supplies `argv[0]`, P5 — same convention as
    /// [`ProcessSpawn`]). **No cleanup runs** on the success path: `execvp` discards the entire address
    /// space, so pending `Drop`s / arena ends / buffered-writer flushes DO NOT run (buffered bytes still
    /// in user space are lost — flush before `exec` if they matter). This is inherent to `execvp` and is
    /// abort-class in cleanup terms (unlike `process.exit`, which runs cleanup first). Align-owned fds
    /// are `CLOEXEC` (Slice 2), so the new image does NOT inherit them; only the inherited standard
    /// streams (fds 0/1/2, not `CLOEXEC`) survive. Impure; both `cmd` and `args` are borrowed.
    ProcessExec { cmd: Box<Expr>, args: Box<Expr> },
    /// `encoding.base64_encode`/`base64url_encode`/`hex_encode(data)` — encode a byte view (`str` /
    /// owned `string` (auto-borrowed) / `slice<u8>`) into a freshly heap-allocated owned `string`
    /// (the `ty` is [`crate::Ty::String`]). `kind` selects the alphabet. Pure (a byte transform, no
    /// I/O); `data` is borrowed, never consumed (like `hash64` / `print`).
    EncodingEncode { kind: EncodingKind, data: Box<Expr> },
    /// `encoding.base64_decode`/`base64url_decode`/`hex_decode(s)` — decode a `str` into an owned
    /// `buffer` (`bytes` carries no UTF-8 invariant, so a decoded blob is not a `str`); invalid
    /// input yields `Error.Invalid`. The `ty` is `Result<buffer, Error>`. Pure; `input` is borrowed.
    EncodingDecode { kind: EncodingKind, input: Box<Expr> },
    /// `encoding.utf8_valid(b)` — whether the bytes `b` (`slice<u8>`) are valid UTF-8 (the `ty` is
    /// [`crate::Ty::Bool`]). A thin wrapper over the shared UTF-8 validator, for checking `bytes`
    /// before turning them into a `str`. Pure; `data` is borrowed.
    Utf8Valid { data: Box<Expr> },
    /// `compress.gzip_compress(data, level)` — compress the byte view `data` (`str` / owned `string`
    /// auto-borrowed / `slice<u8>`) at `level` (an `i64` in `0..=9`; out-of-range aborts at runtime,
    /// a programmer error like `rand.range`) into an owned `buffer` (the `ty` is
    /// `Result<buffer, Error>`). Wraps the libz DEFLATE engine (draft §15). **Impure** (a C-engine
    /// call — never `Pure`, so excluded from `par_map`). `data` is borrowed, never consumed.
    Compress { kind: CompressKind, data: Box<Expr>, level: Box<Expr> },
    /// `compress.gzip_decompress(data)` — inflate the gzip byte view `data` into an owned `buffer`
    /// (the `ty` is `Result<buffer, Error>`). Corrupt / truncated input, or a decompress "bomb" that
    /// would exceed the runtime output cap, yields `Error.Invalid`. **Impure**; `data` is borrowed.
    Decompress { kind: CompressKind, data: Box<Expr> },
    /// `rand.seed()` — a fresh [`crate::Ty::Rng`] seeded from the OS CSPRNG (`getrandom`). The `ty`
    /// is [`crate::Ty::Rng`], a **Copy** state-only value (no fd/ownership). Impure (reads OS
    /// entropy — a different sequence each run).
    RandSeed,
    /// `rand.seed_with(s)` — a deterministic [`crate::Ty::Rng`] seeded from the `i64` `s` (same seed
    /// → same sequence). The `ty` is [`crate::Ty::Rng`]. Impure (it produces mutable RNG state; a
    /// closure that seeds/advances an rng is never `Pure`, so it stays out of `par_map`).
    RandSeedWith { seed: Box<Expr> },
    /// `r.next()` — advance the rng state (Xoshiro256++) and return the next `i64`. `rng` is a bound
    /// **mut** local (the receiver, an [`ExprKind::Local`]); the state is updated in place. The `ty`
    /// is `i64`. Impure (mutates the receiver state).
    RandNext { rng: Box<Expr> },
    /// `r.range(lo, hi)` — a uniform `i64` in `[lo, hi)` (bias-free, Lemire nearly-divisionless);
    /// `lo >= hi` aborts at runtime. `rng` is a bound mut local. The `ty` is `i64`. Impure.
    RandRange { rng: Box<Expr>, lo: Box<Expr>, hi: Box<Expr> },
    /// `r.shuffle(out xs)` — Fisher-Yates shuffle of the writable slice `xs` (`slice<T>`) in place.
    /// `rng` is a bound mut local; `xs` is a mut/`out` slice place. The `ty` is [`crate::Ty::Unit`].
    /// Impure (mutates both the rng state and the slice contents).
    RandShuffle { rng: Box<Expr>, xs: Box<Expr>, elem: crate::Ty },
    /// `r.sample(xs, k)` — `k` elements drawn from `xs` (`slice<T>`) without replacement, as a fresh
    /// owned `array<T>` (`ty` = [`crate::Ty::DynArray`]); `k < 0` or `k > xs.len()` aborts at
    /// runtime. `rng` is a bound mut local. Impure.
    RandSample { rng: Box<Expr>, xs: Box<Expr>, k: Box<Expr>, elem: crate::Ty },
    /// `cli.command(name)` — a fresh [`crate::Ty::CliCommand`] builder named `name` (a `str`). A
    /// **Move** handle owning its registered-flag table; `Drop`-freed. Pure (no I/O — argv is
    /// already captured by `main(args)`). `name` is borrowed.
    CliCommand { name: Box<Expr> },
    /// `c.flag_bool(name)` / `c.flag_str(name, default)` / `c.flag_i64(name, default)` — register a
    /// flag into the command `cmd`'s table (`kind` selects which). The `ty` is [`crate::Ty::Unit`].
    /// `cmd` is a bound [`crate::Ty::CliCommand`] local, mutated in place through its handle pointer
    /// (not consumed — like a `buffer` method). `default` is `None` for `flag_bool` (default `false`),
    /// `Some(str)` for `flag_str`, `Some(i64)` for `flag_i64`. Pure.
    CliFlag { cmd: Box<Expr>, kind: CliFlagKind, name: Box<Expr>, default: Option<Box<Expr>> },
    /// `c.parse(args)` — parse the `array<str>` argv `args` against `cmd`'s flag table, yielding
    /// `Result<parsed, Error>` (the `ty`). An unknown flag / missing value / malformed i64 / wrong
    /// kind is `Error.Invalid`. **`cmd` is borrowed, NOT consumed** (`c.usage()` stays callable after
    /// parse, including on the `Err` path). `args` is borrowed. Pure.
    CliParse { cmd: Box<Expr>, args: Box<Expr> },
    /// `p.get_bool(name)` — the parsed `bool` value of flag `name` (the `ty` is [`crate::Ty::Bool`]).
    /// Total after a successful parse: an unregistered `name` or a kind mismatch is a **runtime
    /// abort** (programmer error, like an OOB index — never a silent default / `Result`). `parsed` is
    /// a bound [`crate::Ty::CliParsed`] local. Pure.
    CliGetBool { parsed: Box<Expr>, name: Box<Expr> },
    /// `p.get_i64(name)` — the parsed `i64` value of flag `name` (the `ty` is `i64`). Abort on
    /// unregistered / wrong-kind (see [`Self::CliGetBool`]). `parsed` is a bound local. Pure.
    CliGetI64 { parsed: Box<Expr>, name: Box<Expr> },
    /// `p.get_str(name)` — the parsed `str` value of flag `name`, a **view into `parsed`'s storage**
    /// (the `ty` is [`crate::Ty::Str`]), so it is region-bound to `parsed` (must not outlive it —
    /// `.clone()` copies out). Abort on unregistered / wrong-kind. `parsed` is a bound local. Pure.
    CliGetStr { parsed: Box<Expr>, name: Box<Expr> },
    /// `c.usage()` — render `cmd`'s flag table into a fresh owned `string` (the `ty` is
    /// [`crate::Ty::String`]). `cmd` is borrowed, not consumed. Pure.
    CliUsage { cmd: Box<Expr> },
    /// `http.request(method, url)` — a fresh [`crate::Ty::HttpRequest`] builder (a **Move** handle
    /// owning its method / url / header list / body buffer; `Drop`-freed). `method` / `url` are
    /// borrowed `str` (copied into the handle). Total — the URL is not parsed here (validation is
    /// deferred to `serialize`, so a runtime-supplied URL never aborts the builder). Pure (no I/O —
    /// the network client is Slice 2).
    HttpRequest { method: Box<Expr>, url: Box<Expr> },
    /// `r.header(name, value)` — append a header to the request `req` (mutated in place through its
    /// handle, not consumed — like a `buffer`/cli method). The `ty` is [`crate::Ty::Unit`]. A CR / LF
    /// / NUL in the name or value (or an empty name) **aborts** at runtime (request-smuggling defence,
    /// http.md P6 — header injection is a programmer error). `req` is a bound local; `name`/`value`
    /// are borrowed `str`. Pure.
    HttpHeader { req: Box<Expr>, name: Box<Expr>, value: Box<Expr> },
    /// `r.body(data)` — set the request `req`'s body to a copy of `data` (a byte view — `str` /
    /// `slice<u8>`), mutating in place. The `ty` is [`crate::Ty::Unit`]. `req` is a bound local. Pure.
    HttpBody { req: Box<Expr>, data: Box<Expr> },
    /// `http.parse(data)` — parse a complete HTTP/1.1 response buffer `data` (a byte view) into an
    /// owned [`crate::Ty::HttpResponse`], yielding `Result<response, Error>` (the `ty`). A malformed
    /// status line / non-numeric status / header without `:` / chunked encoding / bad or oversized
    /// framing / header flood → `Error.Invalid`. The response owns ONE copy of the bytes (zero-copy
    /// offset table, http.md R1). `data` is borrowed. Pure.
    HttpParse { data: Box<Expr> },
    /// `resp.status()` — the parsed status code of the response `resp` (the `ty` is `i64`). `resp` is
    /// a bound [`crate::Ty::HttpResponse`] local. Pure.
    HttpRespStatus { resp: Box<Expr> },
    /// `resp.header(name)` — a **case-insensitive** header lookup on `resp`, yielding
    /// `Option<str>` (the `ty`) whose `str` is a **view** into `resp`'s buffer (region-bound to
    /// `resp` — an escape past `resp`'s `Drop` is a compile error). `resp` is a bound local; `name`
    /// is borrowed. Pure.
    HttpRespHeader { resp: Box<Expr>, name: Box<Expr> },
    /// `resp.body()` — the response body as a `slice<u8>` **view** into `resp`'s buffer (the `ty` is
    /// [`crate::Ty::Slice`] of `u8`), region-bound to `resp` (no copy, http.md R1). `resp` is a bound
    /// local. Pure.
    HttpRespBody { resp: Box<Expr> },
    /// `http.client()` — a fresh [`crate::Ty::HttpClient`] handle (a **Move** handle; `Drop`-freed).
    /// Slice 2 carries no pooled state. **Impure**? No — allocating the handle performs no I/O; the
    /// *requests* (`get`/`post`/`request`) are Impure. No operands.
    HttpClient,
    /// `cl.get(url)` — perform a `GET url` over a fresh connection, yielding `Result<response, Error>`
    /// (the `ty`). `cl` is a bound [`crate::Ty::HttpClient`] local (borrowed, not consumed); `url` is a
    /// borrowed `str`. **Impure** (network I/O). A 4xx/5xx status is `Ok(response)` (http.md P2); a
    /// `https://` / malformed URL is `Error.Invalid` (http.md P1).
    HttpClientGet { client: Box<Expr>, url: Box<Expr> },
    /// `cl.post(url, body)` — perform a `POST url` with `body` (auto `Content-Length`) over a fresh
    /// connection, yielding `Result<response, Error>`. `cl` is a bound local (borrowed); `url` is a
    /// borrowed `str`; `body` is a borrowed byte view (`str` / owned `string` / `slice<u8>`). **Impure**.
    HttpClientPost { client: Box<Expr>, url: Box<Expr>, body: Box<Expr> },
    /// `cl.request(req)` — perform the fully-built request `req` over a fresh connection, yielding
    /// `Result<response, Error>`. `cl` is a bound local (borrowed); `req` is a
    /// [`crate::Ty::HttpRequest`] **consumed** (moved into the call — the runtime frees it). **Impure**.
    HttpClientRequest { client: Box<Expr>, req: Box<Expr> },
    /// `cl.get_many(urls, max_concurrency)` — perform a batch of GETs over bounded concurrency,
    /// yielding `Result<array<response>, Error>` (the `ty` — an owned [`crate::Ty::DynResponseArray`]
    /// Ok payload; results in input order, all-or-Err). `cl` is a bound [`crate::Ty::HttpClient`] local
    /// (borrowed, shared across the workers); `urls` is a borrowed `slice<str>`; `max_concurrency` is an
    /// `i64` (`<= 0` aborts at runtime). **Impure** (network I/O). std.http item 6 / R5.
    HttpGetMany { client: Box<Expr>, urls: Box<Expr>, max_concurrency: Box<Expr> },
    /// `http.serve(host, port)` — bind a listening socket and yield `Result<http_server, Error>` (the
    /// `ty`). The `http_server` ([`crate::Ty::HttpServer`]) is an owned **Move** handle owning the
    /// listening fd (`Drop`-closed). `host` is a borrowed `str` (empty → wildcard); `port` is an `i64`.
    /// **Impure** (opens a socket). Wraps the net rail's `tcp.listen` (SO_REUSEADDR, backlog 128).
    HttpServe { host: Box<Expr>, port: Box<Expr> },
    /// `srv.accept()` — block for one inbound connection, read + parse its request, and yield
    /// `Result<http_request_ctx, Error>` (the `ty`). The `http_request_ctx`
    /// ([`crate::Ty::HttpRequestCtx`]) is an owned **Move** handle owning the accepted fd + the parsed
    /// request (zero-copy offset table, http.md R1). A malformed / smuggling request is `Error.Invalid`
    /// (the listener stays alive). `srv` is a bound [`crate::Ty::HttpServer`] local (borrowed, not
    /// consumed — a server accepts many). **Impure** (network I/O).
    HttpAccept { server: Box<Expr> },
    /// `ctx.method()` — the request method as a `str` **view** into `ctx`'s buffer (the `ty` is
    /// [`crate::Ty::Str`]), region-bound to `ctx`. `ctx` is a bound [`crate::Ty::HttpRequestCtx`] local.
    /// Pure.
    HttpCtxMethod { ctx: Box<Expr> },
    /// `ctx.path()` — the request target (origin-form path) as a `str` **view** into `ctx`'s buffer (the
    /// `ty` is [`crate::Ty::Str`]), region-bound to `ctx`. `ctx` is a bound local. Pure.
    HttpCtxPath { ctx: Box<Expr> },
    /// `ctx.header(name)` — a **case-insensitive** request-header lookup on `ctx`, yielding
    /// `Option<str>` (the `ty`) whose `str` is a **view** into `ctx`'s buffer (region-bound to `ctx`).
    /// `ctx` is a bound local; `name` is borrowed. Pure. The read-dual of [`HttpRespHeader`].
    HttpCtxHeader { ctx: Box<Expr>, name: Box<Expr> },
    /// `ctx.body()` — the request body as a `slice<u8>` **view** into `ctx`'s buffer (the `ty` is
    /// [`crate::Ty::Slice`] of `u8`), region-bound to `ctx`. `ctx` is a bound local. Pure. The read-dual
    /// of [`HttpRespBody`].
    HttpCtxBody { ctx: Box<Expr> },
    /// `http.response(status)` — a fresh [`crate::Ty::ResponseBuilder`] (a **Move** handle owning a
    /// status + header list + optional body; `Drop`-freed). The build-dual of [`HttpRequest`], named
    /// apart from the parsed read-view [`crate::Ty::HttpResponse`]. Total (the status is validated at
    /// `respond`). `status` is an `i64`. Pure (no I/O — the write is at `respond`).
    HttpResponseBuilder { status: Box<Expr> },
    /// `rb.header(name, value)` — append a header to the response builder `rb` (mutated in place through
    /// its handle, not consumed). The `ty` is [`crate::Ty::Unit`]. A CR / LF / NUL in the name or value
    /// (or an empty name) **aborts** at runtime (http.md P6). `rb` is a bound local; `name`/`value` are
    /// borrowed `str`. Pure. The build-dual of [`HttpHeader`].
    HttpRbHeader { rb: Box<Expr>, name: Box<Expr>, value: Box<Expr> },
    /// `rb.body(data)` — set the response builder `rb`'s body to a copy of `data` (a byte view),
    /// mutating in place; setting the body is what makes `respond` auto-emit `Content-Length`. The `ty`
    /// is [`crate::Ty::Unit`]. `rb` is a bound local. Pure. The build-dual of [`HttpBody`].
    HttpRbBody { rb: Box<Expr>, data: Box<Expr> },
    /// `ctx.respond(rb)` — serialize `rb` and write the response to `ctx`'s connection in one write,
    /// then close the fd (v1: one request per conn), yielding `Result<(), Error>` (the `ty`). **Consumes
    /// BOTH** `ctx` ([`crate::Ty::HttpRequestCtx`]) and `rb` ([`crate::Ty::ResponseBuilder`]) — the
    /// runtime frees both (like [`HttpClientRequest`]'s `req`). A caller-supplied Content-Length / a bad
    /// status is `Error.Invalid`. **Impure** (network I/O).
    HttpRespond { ctx: Box<Expr>, rb: Box<Expr> },
    /// `ctx.respond_stream(rb)` — begin a chunked/streaming response: serialize `rb`'s head (header-only)
    /// plus the transfer framing, write it, and yield `Result<http_stream, Error>` (the `ty`). **Consumes
    /// BOTH** `ctx` ([`crate::Ty::HttpRequestCtx`]) and `rb` ([`crate::Ty::ResponseBuilder`]) — the
    /// runtime frees both (like [`HttpRespond`]) and lifts the accepted fd into the returned
    /// [`crate::Ty::HttpStream`]. A `rb` with a body set **aborts** (a header-only builder is required —
    /// the streamed body is written with `s.send`); a bad status is `Error.Invalid`. A 1.0 request gets
    /// close-delimited raw framing (no chunked). **Impure** (network I/O).
    HttpRespondStream { ctx: Box<Expr>, rb: Box<Expr> },
    /// `s.send(chunk)` — write one streamed chunk (one chunk frame in framed/1.1 mode, or raw payload
    /// bytes in 1.0 mode) to the stream `s` ([`crate::Ty::HttpStream`]), yielding `Result<(), Error>`
    /// (the `ty`). `send("")` is a no-op returning `Ok` (an empty chunk is the terminator). `s` is a
    /// bound local, **borrowed** (mutated in place — the poison latch), not consumed; `chunk` is a byte
    /// view. **Impure** (network I/O).
    HttpStreamSend { stream: Box<Expr>, chunk: Box<Expr> },
    /// `s.finish()` — the sole clean terminator: write `0\r\n\r\n` (framed mode) + close, yielding
    /// `Result<(), Error>` (the `ty`). **Consumes** `s` ([`crate::Ty::HttpStream`]) — the runtime frees
    /// it. A poisoned stream (a prior failed `send`) skips the terminal write and returns `Err`.
    /// **Impure** (network I/O).
    HttpStreamFinish { stream: Box<Expr> },
    /// `crypto.constant_time_equal(a, b)` — a constant-time byte-equality test over two byte views
    /// `a` / `b` (`str` / owned `string` auto-borrowed / `slice<u8>`); the `ty` is
    /// [`crate::Ty::Bool`]. The input *length* is **public** (crypto.md P1): differing lengths return
    /// `false` immediately; the constant-time guarantee is over the *content* of equal-length inputs
    /// (a byte-diff OR-reduction with no early return, no secret-dependent branch — the runtime
    /// hardens the reduction against the compiler). **Pure** (a self-hosted branchless computation, no
    /// I/O), so it is allowed inside a `par_map` closure. Both operands are borrowed, never consumed.
    CryptoCtEqual { a: Box<Expr>, b: Box<Expr> },
    /// `crypto.random(out)` — fill the whole `buffer` `out` (its full capacity) with OS CSPRNG bytes
    /// (`getrandom` / `getentropy`, key-grade); the `ty` is [`crate::Ty::Unit`]. A CSPRNG failure is
    /// rare and **aborts** (key material is not a recoverable `Result`, the `rand.seed` policy). `out`
    /// is a **mut** [`crate::Ty::Buffer`], borrowed and mutated in place through its handle pointer
    /// (not consumed — like `reader.read`'s buffer). **Impure** (reads OS entropy), so an
    /// rng-filling closure is never `Pure` and is excluded from `par_map`.
    CryptoRandom { out: Box<Expr> },
    /// `crypto.sha256(data)` / `crypto.sha512(data)` — the cryptographic digest of the byte view
    /// `data` (`str` / owned `string` auto-borrowed / `slice<u8>`), as a freshly heap-allocated owned
    /// `array<u8>` of fixed length (32 for SHA-256, 64 for SHA-512). The `ty` is
    /// [`crate::Ty::DynArray`] of `u8` — an *owned, dynamic-length* array carrying its known length
    /// (the ideal fixed-`array<u8; N>` form is not expressible with the current runtime-return ABI,
    /// which hands back a `{ptr,len}` heap array; the length is fixed by `algo` and re-checked in the
    /// runtime — crypto.md "Fixed-size digests as `array<u8>`"). `algo` selects SHA-256 vs SHA-512.
    /// Wraps OpenSSL libcrypto's one-shot `EVP_Q_digest`. **Impure** (a C-engine call — never `Pure`,
    /// so excluded from `par_map`, matching `std.compress`; the determinism of hashing does not make
    /// it pure). `data` is borrowed, never consumed.
    CryptoHash { algo: HashAlgo, data: Box<Expr> },
    /// `crypto.hmac_sha256(key, data)` — the 32-byte HMAC-SHA-256 tag of the byte view `data` under
    /// the byte view `key`, as a freshly heap-allocated owned `array<u8>` of length 32 (the same
    /// owned-`array<u8>` shape as [`ExprKind::CryptoHash`]; the `ty` is [`crate::Ty::DynArray`] of
    /// `u8`). Empty `key` and empty `data` are both valid HMAC inputs. Wraps OpenSSL libcrypto's
    /// one-shot `EVP_Q_mac`. **Impure** (a C-engine call — never `Pure`, so excluded from `par_map`).
    /// Both `key` and `data` are borrowed, never consumed.
    CryptoHmac { key: Box<Expr>, data: Box<Expr> },
    /// `crypto.hkdf_sha256(salt, ikm, info, len)` — derive `len` bytes with HKDF-SHA-256 over the byte
    /// views `salt` / `ikm` (input keying material) / `info`, as a `Result<buffer, Error>` (the same
    /// status→owned-`buffer` shape as [`ExprKind::Decompress`]; the `ty` is
    /// [`crate::Ty::Result`] of `buffer` / `Error`). `len` is an `i64`; a non-positive or over-limit
    /// (`> 255*32` = 8160, the RFC 5869 `L` limit for SHA-256) length → `Error.Invalid` (validated as
    /// a **public** value before the engine); `salt` and `info` may be empty. Wraps OpenSSL
    /// libcrypto's `EVP_KDF` HKDF path. **Impure** (a C-engine call). All four operands are borrowed,
    /// never consumed.
    CryptoHkdf { salt: Box<Expr>, ikm: Box<Expr>, info: Box<Expr>, len: Box<Expr> },
    /// `crypto.aes_gcm_seal` / `aes_gcm_open` / `chacha20_poly1305_seal` / `chacha20_poly1305_open`
    /// (M11 Slice 4) — authenticated encryption (AEAD) over OpenSSL libcrypto's `EVP_CIPHER`. One node
    /// serves all four surfaces: `cipher` param-swaps the fetched cipher (`"AES-256-GCM"` /
    /// `"ChaCha20-Poly1305"`) and `dir` picks seal (encrypt) vs open (decrypt) — mirroring the
    /// [`CompressKind`]-carrying compress nodes. Both directions yield `Result<buffer, Error>` (the
    /// status→owned-`buffer` shape as [`ExprKind::Decompress`]; the `ty` is [`crate::Ty::Result`] of
    /// `buffer` / `Error`).
    ///
    /// Combined format (the libsodium convention): seal's output `buffer` is `ciphertext || tag`
    /// (the 16-byte authentication tag appended), and open's `input` is that same `ciphertext || tag`.
    /// `key` must be exactly 32 bytes (both ciphers are 256-bit) and `nonce` exactly 12 bytes — public
    /// values validated before any engine call, a violation → `Error.Invalid`. `plaintext`/`ciphertext`
    /// (`input`) and `aad` may be empty (empty plaintext → a tag-only 16-byte output that round-trips).
    ///
    /// **All-or-nothing on open** (crypto.md P2): open never yields partial plaintext — the whole
    /// ciphertext is decrypted into an internal buffer and released to the caller ONLY after the tag
    /// verifies, and any failure (tag mismatch, truncation, bad length, engine error) is the single
    /// opaque `Error.Invalid` (indistinguishable). Seal engine failures (after param validation) →
    /// `Error.Code`. **Impure** (a C-engine call — never `Pure`, so excluded from `par_map`). All four
    /// operands are borrowed byte views, never consumed.
    ///
    /// **Nonce discipline** (crypto.md P3): nonce reuse under the same key is catastrophic (GCM
    /// especially — it destroys confidentiality and forgery resistance). v1 does not auto-generate
    /// nonces; the caller supplies one (e.g. via `crypto.random`). A nonce-generating convenience is a
    /// recorded candidate.
    CryptoAead { cipher: AeadCipher, dir: AeadDir, key: Box<Expr>, nonce: Box<Expr>, input: Box<Expr>, aad: Box<Expr> },
    /// `crypto.argon2id(password, salt, params)` — Argon2id password hashing / KDF via OpenSSL
    /// libcrypto's `EVP_KDF_fetch("ARGON2ID")` (OpenSSL >= 3.2). `password` / `salt` are byte views;
    /// `params` is the builtin **Copy** struct `argon2_params { m_cost, t_cost, parallelism, len }`
    /// (all `i64`: `m_cost` KiB, `t_cost` iterations, `parallelism` lanes, `len` output bytes) — an
    /// ordinary struct literal at the call site (`argon2_params{m_cost: 65536, t_cost: 3,
    /// parallelism: 1, len: 32}`), so the four security-tuning knobs are named, never positional.
    /// Yields a `Result<buffer, Error>` (the [`ExprKind::CryptoAead`] status→owned-`buffer` shape).
    /// Public param bounds (`t_cost`/`parallelism`/`m_cost`/`len` ranges) are validated **before** the
    /// engine → `Error.Invalid`; a genuine engine failure → `Error.Code`. **Impure** (a C-engine call
    /// — never `Pure`, so excluded from `par_map`). All three operands are borrowed, never consumed
    /// (`params` is a Copy struct).
    CryptoArgon2 { password: Box<Expr>, salt: Box<Expr>, params: Box<Expr> },
}

/// Which AEAD cipher an [`ExprKind::CryptoAead`] uses (M11 Slice 4). Both are 256-bit (32-byte key)
/// with a 96-bit (12-byte) nonce and a 128-bit (16-byte) tag; the axis param-swaps the fetched
/// `EVP_CIPHER` name only. The HIR-walking passes treat it opaquely (they match `..`); only sema
/// dispatch and codegen distinguish the two (the runtime picks the cipher by its entry point).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AeadCipher {
    /// AES-256-GCM (NIST SP 800-38D) — constant-time on AES-NI/PCLMULQDQ hardware. EVP name
    /// `"AES-256-GCM"`.
    Aes256Gcm,
    /// ChaCha20-Poly1305 (RFC 8439) — a constant-time software AEAD. EVP name `"ChaCha20-Poly1305"`.
    ChaCha20Poly1305,
}

/// Which direction an [`ExprKind::CryptoAead`] runs (M11 Slice 4): seal (authenticated encryption) or
/// open (authenticated decryption). The direction is this `dir`, the cipher is [`AeadCipher`] — one
/// node kind serves all four `{aes_gcm,chacha20_poly1305}_{seal,open}` surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AeadDir {
    /// `_seal(key, nonce, plaintext, aad)` — encrypt + authenticate → `ciphertext || tag`.
    Seal,
    /// `_open(key, nonce, ciphertext, aad)` — verify + decrypt `ciphertext || tag`, all-or-nothing.
    Open,
}

/// Which cryptographic hash an [`ExprKind::CryptoHash`] computes (M11 Slice 2). One node kind serves
/// both — the algorithm is this `algo`, param-swapping the EVP digest name and the fixed output
/// length (mirroring [`EncodingKind`] / [`CompressKind`]). The HIR-walking passes treat it opaquely
/// (they match `..`); only sema dispatch, codegen, and the runtime distinguish the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgo {
    /// SHA-256 (FIPS 180-4) — a 32-byte digest. EVP name `"SHA256"`.
    Sha256,
    /// SHA-512 (FIPS 180-4) — a 64-byte digest. EVP name `"SHA512"`.
    Sha512,
}

/// Which `std.cli` flag an [`ExprKind::CliFlag`] registers — the kind decides the value type and
/// whether a default is carried (`Bool` defaults to `false` with no operand; `Str`/`I64` carry one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliFlagKind {
    /// `c.flag_bool(name)` — a boolean flag, default `false`, set by a bare `--name`.
    Bool,
    /// `c.flag_str(name, default)` — a `str` flag with a default, set by `--name value` / `--name=value`.
    Str,
    /// `c.flag_i64(name, default)` — an `i64` flag with a default, set by `--name value` / `--name=value`.
    I64,
}

/// Which `std.encoding` transform an [`ExprKind::EncodingEncode`] / [`ExprKind::EncodingDecode`]
/// performs — the alphabet is the only axis of variation, so one node kind serves encode and
/// decode alike (the direction is the node, the alphabet is this `kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingKind {
    /// Standard Base64 (RFC 4648 §4): `A-Za-z0-9+/`, `=` padding on encode.
    Base64,
    /// URL/filename-safe Base64 (RFC 4648 §5): `-`/`_`, no padding on encode.
    Base64Url,
    /// Lower-case hex (`hex_encode`); `hex_decode` accepts both cases.
    Hex,
}

/// Which `std.compress` codec an [`ExprKind::Compress`] / [`ExprKind::Decompress`] uses. gzip (libz)
/// is the M11 Slice 1 codec; zstd (libzstd) is Slice 2 — the direction is the node kind, the codec
/// is this `kind` (mirroring [`EncodingKind`]). The HIR-walking passes treat the codec opaquely
/// (they match `..`); only sema dispatch, codegen, and the runtime distinguish the two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressKind {
    /// gzip framing (RFC 1952) over DEFLATE, via `libz` — windowBits 15+16 (the gzip wrapper).
    Gzip,
    /// zstd framing (RFC 8878) via `libzstd` — one-shot `ZSTD_compress` / streaming decompress.
    Zstd,
}

/// Which component `path.base` / `path.dir` / `path.ext` extracts — each a zero-copy `str` view
/// (a substring) of the input path (`std.path`, view-safe POSIX lexical semantics).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathComponentKind {
    /// `path.base(p)` — the final path component (trailing `/` stripped; all-`/` → `/`).
    Base,
    /// `path.dir(p)` — everything before the final component (an **empty** view when `p` has no
    /// separator, since the result must be a substring of `p` — not `.`).
    Dir,
    /// `path.ext(p)` — the extension of the final component including the leading `.` (empty when
    /// there is none, or when the only `.` starts the component — a dotfile).
    Ext,
}

/// The source/key path of a column-oriented `group_by` ([`ExprKind::ArrayGroupAgg`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSource {
    /// `soa<Struct>`, contiguous columns, an **i64** key — the dense hash-aggregate path.
    SoaI64,
    /// `soa<Struct>`, contiguous columns, a **str** key column — interned to dense ids by the runtime
    /// reading the two separate contiguous columns (key + value), then aggregated and labeled
    /// (`align_rt_group_*_str_cols`). The columnar counterpart of [`Self::AosStr`].
    SoaStr,
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
    /// `.where(fn(s) = s.contains(NEEDLE))` where `NEEDLE` is a **loop-invariant** `str` expression
    /// free of the lambda parameter (doc-13 §6.6 / §11 P3). Recognised in `collect_pipeline` and
    /// lowered specially — bypassing lambda lifting — so MIR builds one hoisted `str_finder` plan
    /// from `needle` before the loop and reuses it per element (`finder_find(plan, s) >= 0`) instead
    /// of reconstructing a `memchr` searcher on every `str.contains`. The `needle` is type-checked as
    /// a `str` in the enclosing scope; because it is free of the parameter, evaluating it once before
    /// the loop is sound. An element-derived needle (e.g. `s.contains(s[0..3])`) is **not** invariant,
    /// so it is never recognised here and keeps the per-call `Where` path.
    WhereStrContains { needle: Expr },
    /// `.field` — project a struct field out of each element (struct array → scalar).
    Project { field: u32 },
}

#[derive(Clone, Debug)]
pub struct Stage {
    pub kind: StageKind,
    /// The element type after this stage (for `Where`, unchanged from its input).
    pub out_ty: crate::Ty,
}
