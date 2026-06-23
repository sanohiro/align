//! AST definitions (`docs/impl/02-frontend.md` §9).
//!
//! No desugaring: the written form is preserved for the formatter / lint / sema.
//! Every node carries a [`Span`]. M1 covers `fn` (multi-arg) + calls, `if`/`else`
//! as expression and statement, comparison/logical operators, `bool`, `mut` +
//! reassignment, and integer arithmetic. Structs, floats, chars come in later steps.

use align_span::Span;

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// A dotted path like `a.b.c` (module / reference).
#[derive(Clone, Debug)]
pub struct Path {
    pub segments: Vec<Ident>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct File {
    pub module: Option<Path>,
    pub imports: Vec<Path>,
    pub items: Vec<Item>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vis {
    Private,
    Pub,
}

#[derive(Clone, Debug)]
pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
}

/// A keyword-less type declaration whose body is all `name: Type` fields → a struct
/// (`Name { Variant, ... }` sum types are disambiguated by content, deferred past M1).
#[derive(Clone, Debug)]
pub struct StructDecl {
    pub vis: Vis,
    pub name: Ident,
    pub fields: Vec<FieldDef>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldDef {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub vis: Vis,
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: FnBody,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub is_out: bool,
    pub name: Ident,
    pub ty: Type,
}

/// A lambda parameter: a name with an optional type annotation (`x` or `x: T`). The type is
/// inferred from the use site when omitted; it is required when the lambda is used as a value.
#[derive(Clone, Debug)]
pub struct LambdaParam {
    pub name: Ident,
    pub ty: Option<Type>,
}

/// Function body: a block, or a single-expression `= expr` form (`02-frontend.md` §3).
#[derive(Clone, Debug)]
pub enum FnBody {
    Block(Block),
    Expr(Box<Expr>),
}

/// Type annotation. Either a named type — a path optionally followed by generic
/// arguments (`Option<i32>`, `Result<i32, Error>`); the unit type `()` is a `Named`
/// with the sentinel path `"()"` — or an anonymous tuple type `(T, U, ...)`.
#[derive(Clone, Debug)]
pub enum Type {
    Named { path: Path, args: Vec<Type>, span: Span },
    /// `(T, U, ...)` — an anonymous product type (arity ≥ 2; `()` is unit, `(T)` is grouping).
    Tuple { elems: Vec<Type>, span: Span },
    /// `fn(T, U) -> R` — a function-value type (a higher-order-function parameter, e.g.
    /// `fn apply(f: fn(i64) -> i64, x: i64) -> i64`). `ret` is the return type.
    Fn { params: Vec<Type>, ret: Box<Type>, span: Span },
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named { span, .. } | Type::Tuple { span, .. } | Type::Fn { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// A trailing expression (with no `;`/newline END) becomes the block value (`02-frontend.md` §5).
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        is_mut: bool,
        name: Ident,
        ty: Option<Type>,
        init: Expr,
    },
    /// `(a, b, ...) := expr` — tuple destructuring. Each binder is a name or `_` (ignore).
    /// The element types are inferred from the tuple on the right (no annotation in this cut).
    LetTuple {
        names: Vec<Option<Ident>>,
        init: Expr,
        span: Span,
    },
    Assign {
        place: Expr,
        value: Expr,
    },
    Return(Option<Expr>),
    Expr(Expr),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// The unit value `()`.
    Unit,
    /// Integer literal. Its type is fixed by context (`03-types.md` §2).
    Int(i128),
    /// Floating-point literal; its width (`f32`/`f64`) is fixed by context.
    Float(f64),
    /// Character literal (a Unicode scalar value).
    Char(u32),
    /// String literal (decoded contents).
    Str(String),
    Bool(bool),
    Path(Path),
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
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `recv.field` — field access (struct field) or, when followed by `(...)`, the
    /// callee of a method call. Method chains (`a.map(f).where(p)`) parse as nested
    /// `FieldAccess` + `Call`.
    FieldAccess {
        recv: Box<Expr>,
        field: Ident,
    },
    /// `if cond { .. } else { .. }`. `els` is the optional else branch (a block, or
    /// another `if` for `else if`).
    If {
        cond: Box<Expr>,
        then: Block,
        els: Option<Box<Expr>>,
    },
    Block(Block),
    /// `Name { field: value, ... }` — a struct value literal. Field access (`base.field`)
    /// is parsed as a [`Path`] and resolved in sema.
    StructLit {
        name: Ident,
        fields: Vec<FieldInit>,
    },
    /// `opt else fallback` — Option unwrap. `Some(x)` yields `x`; `None` evaluates
    /// `fallback` (which produces the value, or diverges via `return`).
    ElseUnwrap {
        opt: Box<Expr>,
        fallback: Box<Expr>,
    },
    /// `expr?` — Result propagation. `Ok(v)` yields `v`; `Err(e)` early-returns
    /// `Err(e)` from the enclosing function.
    Try(Box<Expr>),
    /// `arena { ... }` — a region whose allocations are freed in bulk at block end.
    Arena(Block),
    /// `task_group { ... }` — a structured concurrency scope: `spawn(fn { … })` defers work and
    /// returns a `Task<R>`; `wait()` joins; `t.get()` reads a result. (`spawn`/`wait`/`get` are
    /// name-based builtins, like `print`/`heap.new`, valid only inside this scope.)
    TaskGroup(Block),
    /// `[e1, e2, ...]` — a fixed-length array literal.
    ArrayLit(Vec<Expr>),
    /// `recv[index]` — element access into an array / slice / owned array. Out-of-bounds is a
    /// hard runtime error (abort), per the settled panic model.
    Index {
        recv: Box<Expr>,
        index: Box<Expr>,
    },
    /// `.field` — element-field shorthand, valid only as a pipeline stage argument
    /// (e.g. `where(.active)`); refers to a field of the current pipeline element.
    FieldShorthand(Ident),
    /// `fn p0, p1: T { ... }` — an anonymous function (lambda). A parameter's type is inferred
    /// from the use site (a pipeline stage's element type) when unannotated, or written explicitly
    /// (`p: T`) — explicit types are required when the lambda is used as a value (`f := fn x: i32
    /// { … }`), since there is no use site to infer from. The body is a block; sema lifts the
    /// lambda to a synthetic top-level function.
    Lambda {
        params: Vec<LambdaParam>,
        body: Block,
    },
    /// `(e0, e1, ...)` — a tuple value (arity ≥ 2; `()` is `Unit`, `(e)` is just `e`).
    Tuple(Vec<Expr>),
    /// `recv.0` / `recv.1` — positional access into a tuple value.
    TupleIndex {
        recv: Box<Expr>,
        index: u32,
    },
    /// `template "text {expr} ..."` — a string built from static parts and `{expr}`
    /// holes (interpolation). Produces a `str`.
    Template(Vec<TemplatePart>),
}

#[derive(Clone, Debug)]
pub enum TemplatePart {
    /// Literal text between holes.
    Text(String),
    /// `{expr}` — interpolate the value of an expression (int or str).
    Hole(Expr),
}

#[derive(Clone, Debug)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}
