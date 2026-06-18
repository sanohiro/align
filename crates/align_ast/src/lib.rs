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

/// Function body: a block, or a single-expression `= expr` form (`02-frontend.md` §3).
#[derive(Clone, Debug)]
pub enum FnBody {
    Block(Block),
    Expr(Box<Expr>),
}

/// Type annotation. A path optionally followed by generic arguments
/// (`Option<i32>`, `Result<i32, Error>`).
#[derive(Clone, Debug)]
pub struct Type {
    pub path: Path,
    pub args: Vec<Type>,
    pub span: Span,
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
}

#[derive(Clone, Debug)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}
