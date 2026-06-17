//! typed HIR: the tree after type checking (`docs/impl/03-types.md` §10).
//!
//! Nearly isomorphic to the AST, but every expression carries a resolved [`Ty`] and
//! references are resolved to [`LocalId`]. An anti-rewrite output so later stages
//! (MIR/codegen) don't recompute types (`00-overview.md`). M0 has no sugar yet such
//! as `?` / `else` / `template` / arena.

use crate::Ty;
use align_ast::BinOp;
use align_span::Span;

/// Identifier of a local variable within a function body.
pub type LocalId = u32;

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Fn>,
}

#[derive(Clone, Debug)]
pub struct Fn {
    pub name: String,
    pub ret: Ty,
    pub locals: Vec<Local>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub id: LocalId,
    pub name: String,
    pub ty: Ty,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        local: LocalId,
        ty: Ty,
        init: Expr,
    },
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
    Int(i128),
    Local(LocalId),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}
