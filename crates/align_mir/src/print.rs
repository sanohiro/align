//! MIR のテキスト出力 (`alignc emit-mir`, `docs/impl/04-mir.md` §8)。
//! fusion 前後の比較や「最適化が効いているか」の確認に使う (予測可能性の担保)。

use crate::{ty_name, Block, Function, Operand, Program, Rvalue, Stmt, Term};
use align_ast::BinOp;
use std::fmt::Write;

pub fn program_to_string(p: &Program) -> String {
    let mut out = String::new();
    for f in &p.fns {
        fn_to_string(&mut out, f);
        out.push('\n');
    }
    out
}

fn fn_to_string(out: &mut String, f: &Function) {
    let _ = writeln!(out, "fn {}() -> {} {{", f.name, ty_name(f.ret));
    for b in &f.blocks {
        block_to_string(out, b);
    }
    let _ = writeln!(out, "}}");
}

fn block_to_string(out: &mut String, b: &Block) {
    let _ = writeln!(out, "  bb{}:", b.id);
    for s in &b.stmts {
        match s {
            Stmt::Let(v, rv) => {
                let _ = writeln!(out, "    %{v} = {}", rvalue_str(rv));
            }
        }
    }
    match &b.term {
        Term::Return(Some(op)) => {
            let _ = writeln!(out, "    return {}", operand_str(op));
        }
        Term::Return(None) => {
            let _ = writeln!(out, "    return");
        }
    }
}

fn rvalue_str(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(op) => operand_str(op),
        Rvalue::Bin(op, a, b) => {
            format!("{} {} {}", operand_str(a), binop_str(*op), operand_str(b))
        }
    }
}

fn operand_str(op: &Operand) -> String {
    match op {
        Operand::Const(v, ty) => format!("{v}_{}", ty_name(*ty)),
        Operand::Value(v) => format!("%{v}"),
    }
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
    }
}
