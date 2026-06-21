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

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Fn>,
    /// Struct definitions, indexed by the id carried in [`crate::Ty::Struct`].
    pub structs: Vec<StructDef>,
}

#[derive(Clone, Debug)]
pub struct StructDef {
    pub name: String,
    /// Fields in declaration order; the position is the field index used by MIR/codegen.
    pub fields: Vec<FieldDef>,
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
    Assign { local: LocalId, value: Expr },
    /// `base.field = value` where `base` is a struct local.
    AssignField { base: LocalId, index: u32, value: Expr },
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
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        func: String,
        args: Vec<Expr>,
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
    /// `base.field` read, where `base` is a struct local. The expression `ty` is the
    /// field type.
    Field {
        base: LocalId,
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
    /// Borrow an owned `string` as a `str` view (MMv2 slice 7b). The two share the `{ptr,len}`
    /// layout, so this is a zero-cost, allocation-free read-only view — an implicit coercion at
    /// a `str`-parameter call site. The `string` is **not** moved (it stays owned by its slot
    /// and is `Drop`-freed by its owner); the view borrows it, so it is `Frame`-regioned and
    /// must not outlive the frame holding the `string`.
    StrBorrow(Box<Expr>),
    /// `builder()` — open an append-oriented string builder (MMv2 slice 7c). The `ty` is
    /// [`crate::Ty::Builder`] (an owned, Move handle).
    BuilderNew,
    /// `b.write(s)` / `b.write_int(n)` — append to a builder, mutating it through its handle.
    /// The builder is borrowed (not consumed); the `ty` is `Unit`.
    BuilderWrite { builder: Box<Expr>, arg: Box<Expr>, kind: BuilderWriteKind },
    /// `b.to_string()` — finish a builder into an **owned** `string`, consuming (moving) the
    /// builder. The `ty` is [`crate::Ty::String`].
    BuilderToString(Box<Expr>),
    /// `[e1, e2, ...]` — a fixed-length array literal. `elem` is the element type
    /// (a scalar, or a struct for an array-of-structs whose elements are `StructLit`s).
    ArrayLit { elems: Vec<Expr>, elem: crate::Ty },
    /// A fused array pipeline ending in `sum`: `source.map(f).where(p)….sum()`. The
    /// stages and the reduction lower to a single loop (no intermediate arrays).
    ArraySum { source: Box<Expr>, stages: Vec<Stage> },
    /// `source.….count()` — count the elements that survive the stages. Always `i64`;
    /// the element value is irrelevant, so no scalar projection is required.
    ArrayCount { source: Box<Expr>, stages: Vec<Stage> },
    /// `source.….any(p)` / `.all(p)` — whether the predicate `func` holds for any / all
    /// surviving (scalar) elements. Always `bool`; `all` selects an `&&`-fold over `||`.
    ArrayAnyAll { source: Box<Expr>, stages: Vec<Stage>, func: String, all: bool },
    /// `source.….min()` / `.max()` — the smallest / largest surviving (scalar, numeric)
    /// element. `is_max` selects max over min. Seeded with the element type's extreme, so an
    /// empty pipeline yields that extreme (the fold identity, as `sum` yields 0).
    ArrayMinMax { source: Box<Expr>, stages: Vec<Stage>, is_max: bool },
    /// `source.…​.reduce(f, init)` — fold the (post-stage) elements with the binary
    /// function `func` starting from `init`. `ty` is the accumulator type.
    ArrayReduce { source: Box<Expr>, stages: Vec<Stage>, func: String, init: Box<Expr> },
    /// `source.….scan(f, init)` — a *materializing* prefix fold: emit the running accumulator
    /// after each surviving element (`out[k] = acc` after `acc = f(acc, elem)`), starting from
    /// `init`. Yields an owned `array<A>` of survivor length. `elem` is the accumulator scalar
    /// (the output element type, `A`); `func` has type `(A, E) -> A`.
    ArrayScan { source: Box<Expr>, stages: Vec<Stage>, func: String, init: Box<Expr>, elem: crate::Ty },
    /// `a.dot(b)` — the inner product `Σ a[i]*b[i]` of two fixed-length arrays of the same
    /// numeric scalar element and the same (statically known) length. `elem` is that scalar;
    /// the result has type `elem`.
    ArrayDot { a: Box<Expr>, b: Box<Expr>, elem: crate::Ty },
    /// `source.….sort()` — materialize the surviving (numeric scalar) elements into an owned
    /// `array<T>` and sort them ascending in place. `elem` is the element scalar; the result
    /// type is `DynArray(elem)`.
    ArraySort { source: Box<Expr>, stages: Vec<Stage>, elem: crate::Ty },
    /// `source.….to_array()` — materialize the surviving (post-stage) elements into an
    /// *owned* `array<T>` (MMv2 slice 3: arena-bump-allocated). `elem` is the element
    /// scalar type; the expression `ty` is `DynArray(elem)`.
    ArrayToArray { source: Box<Expr>, stages: Vec<Stage>, elem: crate::Ty },
    /// Borrow an array (a local stack array) as a `slice<T>` view — `{ &arr[0], len }`.
    /// Allocation-free, so it is an implicit coercion at call sites.
    ArrayToSlice(Box<Expr>),
    /// `.len()` of a `str` or `slice<T>` (the `len` field of the `{ ptr, len }` view); the
    /// result is `i64`. A fixed array's length is a constant and lowers without this node.
    Len(Box<Expr>),
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
    /// `.map(f)` — transform each element with the named function `func`.
    Map { func: String },
    /// `.where(p)` — keep only elements for which the named predicate `func` is true.
    Where { func: String },
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
