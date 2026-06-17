//! AST 定義 (`docs/impl/02-frontend.md` §9)。
//!
//! 脱糖はしない: 書かれた形を保持し、formatter / lint / sema が使う。全ノードは
//! [`Span`] を持つ。M0 では言語の最小部分集合 (`fn` / `:=` / `return` / 整数 / 四則演算)
//! のみを表現する。後続マイルストーンで列を拡張していく。

use align_span::Span;

#[derive(Clone, Debug)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// `a.b.c` のようなドット区切りパス (module / 参照)。
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

/// 関数本体: ブロック、または単一式 `= expr` 形 (`02-frontend.md` §3)。
#[derive(Clone, Debug)]
pub enum FnBody {
    Block(Block),
    Expr(Box<Expr>),
}

/// 型表記。M0 は `i32` 等の単純パスのみ。
#[derive(Clone, Debug)]
pub struct Type {
    pub path: Path,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// 末尾の式 (`;`/改行 END を伴わない) はブロックの値になる (`02-frontend.md` §5)。
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
    /// 整数リテラル。型は文脈で決まる (`03-types.md` §2)。
    Int(i128),
    Path(Path),
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Block(Block),
}
