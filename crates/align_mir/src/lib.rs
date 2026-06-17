//! MIR: バックエンド非依存の中間表現 (`docs/impl/04-mir.md`)。
//!
//! Align の意味論 (脱糖・fusion・SIMD化・arena) はここで確定し、`MIR → LLVM` は
//! 純粋 lowering に限定する。allocation / error path / 並列単位は明示ノードとして
//! 残す (「隠さない」)。M0 では関数を CFG (基本ブロック列) に落とし、`Let`/`Return`
//! と四則演算 `Bin` のみを扱う。fusion/SIMD/arena は対象機能の導入時に追加する。

use align_ast::BinOp;
use align_sema::{hir, IntTy, Ty};

pub mod print;

/// MIR の局所値 (SSA 風: 一度だけ定義)。HIR の [`hir::LocalId`] とは別の連番。
pub type ValueId = u32;

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Function>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub ret: Ty,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

pub type BlockId = u32;

#[derive(Clone, Debug)]
pub enum Stmt {
    /// `v = rvalue` (純粋計算)。
    Let(ValueId, Rvalue),
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    Use(Operand),
    Bin(BinOp, Operand, Operand),
}

#[derive(Clone, Debug)]
pub enum Operand {
    Const(i128, Ty),
    Value(ValueId),
}

#[derive(Clone, Debug)]
pub enum Term {
    Return(Option<Operand>),
}

/// typed HIR → MIR。M0 の脱糖は最小 (糖衣がまだ無い)。
pub fn lower_program(program: &hir::Program) -> Program {
    Program {
        fns: program.fns.iter().map(lower_fn).collect(),
    }
}

struct Builder {
    next_value: ValueId,
    stmts: Vec<Stmt>,
    /// HIR ローカル → 現在の値 (M0 は再代入が無いので単純な写像)。
    local_values: Vec<(hir::LocalId, ValueId)>,
}

impl Builder {
    fn fresh(&mut self) -> ValueId {
        let v = self.next_value;
        self.next_value += 1;
        v
    }

    fn bind(&mut self, local: hir::LocalId, value: ValueId) {
        self.local_values.push((local, value));
    }

    fn lookup(&self, local: hir::LocalId) -> Option<ValueId> {
        self.local_values
            .iter()
            .rev()
            .find(|(l, _)| *l == local)
            .map(|(_, v)| *v)
    }
}

fn lower_fn(f: &hir::Fn) -> Function {
    let mut b = Builder {
        next_value: 0,
        stmts: Vec::new(),
        local_values: Vec::new(),
    };
    let mut term = Term::Return(None);

    for s in &f.body {
        match s {
            hir::Stmt::Let { local, init, .. } => {
                let v = lower_expr_to_value(&mut b, init);
                b.bind(*local, v);
            }
            hir::Stmt::Return(value) => {
                let op = value.as_ref().map(|e| lower_expr(&mut b, e));
                term = Term::Return(op);
            }
            hir::Stmt::Expr(e) => {
                let _ = lower_expr_to_value(&mut b, e);
            }
        }
    }

    let block = Block {
        id: 0,
        stmts: b.stmts,
        term,
    };
    Function {
        name: f.name.clone(),
        ret: f.ret,
        blocks: vec![block],
    }
}

/// 式を operand に落とす (定数や既存値はそのまま、複合式は新しい値へ)。
fn lower_expr(b: &mut Builder, e: &hir::Expr) -> Operand {
    match &e.kind {
        hir::ExprKind::Int(v) => Operand::Const(*v, e.ty),
        hir::ExprKind::Local(l) => match b.lookup(*l) {
            Some(v) => Operand::Value(v),
            None => Operand::Const(0, e.ty),
        },
        hir::ExprKind::Binary { .. } => Operand::Value(lower_expr_to_value(b, e)),
    }
}

/// 式を必ず1つの値へ束縛して返す (`Let` を発行)。
fn lower_expr_to_value(b: &mut Builder, e: &hir::Expr) -> ValueId {
    match &e.kind {
        hir::ExprKind::Binary { op, lhs, rhs } => {
            let l = lower_expr(b, lhs);
            let r = lower_expr(b, rhs);
            let v = b.fresh();
            b.stmts.push(Stmt::Let(v, Rvalue::Bin(*op, l, r)));
            v
        }
        _ => {
            let op = lower_expr(b, e);
            let v = b.fresh();
            b.stmts.push(Stmt::Let(v, Rvalue::Use(op)));
            v
        }
    }
}

/// 型を MIR テキスト/診断で使う短い名前へ。
pub fn ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(IntTy { bits, signed }) => format!("{}{}", if signed { 'i' } else { 'u' }, bits),
        Ty::IntVar(_) => "int?".to_string(),
        Ty::Unit => "()".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_lexer::tokenize;
    use align_parser::parse_file;
    use align_sema::check_file;
    use align_diag::Diagnostics;

    fn lower(src: &str) -> Program {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors());
        lower_program(&hir)
    }

    #[test]
    fn m0_lowers_to_return() {
        let p = lower("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        let f = &p.fns[0];
        assert_eq!(f.blocks.len(), 1);
        // x := 1 が1つの値に束縛され、return がその値を返す。
        assert!(matches!(f.blocks[0].term, Term::Return(Some(Operand::Value(_)))));
    }
}
