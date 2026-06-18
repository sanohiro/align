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
    Int(i128),
    Float(f64),
    Char(u32),
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
}
