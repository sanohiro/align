//! AST definitions (`docs/impl/02-frontend.md` §9).
//!
//! No desugaring: keeps the written form, used by formatter / lint / sema. Every
//! node carries a [`Span`]. M0 represents only the minimal language subset
//! (`fn` / `:=` / `return` / integers / arithmetic). Later milestones extend the
//! variants.

use align_span::Span;

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Dot-separated path like `a.b.c` (module / reference).
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

/// Function body: a block, or single-expression `= expr` form (`02-frontend.md` §3).
#[derive(Clone, Debug)]
pub enum FnBody {
    Block(Block),
    Expr(Box<Expr>),
}

/// Type annotation. M0 supports only simple paths like `i32`.
#[derive(Clone, Debug)]
pub struct Type {
    pub path: Path,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// A trailing expression (without `;`/newline END) becomes the block's value (`02-frontend.md` §5).
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
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Integer literal. Its type is decided by context (`03-types.md` §2).
    Int(i128),
    Path(Path),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Block(Block),
}
