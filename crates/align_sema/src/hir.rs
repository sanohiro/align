//! typed HIR: 型検査を通った木 (`docs/impl/03-types.md` §10)。
//!
//! AST とほぼ同形だが、全式に解決済みの [`Ty`] が付き、参照は [`LocalId`] に解決済み。
//! 後段 (MIR/codegen) が型を再計算しないための anti-rewrite 出力 (`00-overview.md`)。
//! M0 では `?` / `else` / `template` / arena といった糖衣はまだ無い。

use crate::Ty;
use align_ast::BinOp;
use align_span::Span;

/// 関数本体内のローカル変数の識別子。
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
