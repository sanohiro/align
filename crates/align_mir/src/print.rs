//! Textual output of MIR (`alignc emit-mir`, `docs/impl/04-mir.md` §8).
//! Used to inspect the CFG and confirm lowering / optimizations (predictability).

use crate::{ty_name, Block, Const, Function, Operand, Program, Rvalue, Stmt, Term};
use align_ast::{BinOp, UnOp};
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
    let params: Vec<String> = f
        .params
        .iter()
        .map(|s| format!("_{s}: {}", ty_name(f.slots[*s as usize])))
        .collect();
    let _ = writeln!(out, "fn {}({}) -> {} {{", f.name, params.join(", "), ty_name(f.ret));
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
            Stmt::Store(slot, op) => {
                let _ = writeln!(out, "    _{slot} <- {}", operand_str(op));
            }
            Stmt::StoreField(slot, idx, op) => {
                let _ = writeln!(out, "    _{slot}.{idx} <- {}", operand_str(op));
            }
            Stmt::StoreIndex(slot, idx, val) => {
                let _ = writeln!(out, "    _{slot}[{}] <- {}", operand_str(idx), operand_str(val));
            }
            Stmt::StoreElemField(slot, idx, field, val) => {
                let _ = writeln!(out, "    _{slot}[{}].{field} <- {}", operand_str(idx), operand_str(val));
            }
            Stmt::PtrStore(ptr, idx, val) => {
                let _ = writeln!(out, "    {}[{}] <- {}", operand_str(ptr), operand_str(idx), operand_str(val));
            }
            Stmt::DropFlagInit(slot) => {
                let _ = writeln!(out, "    drop_init _{slot}");
            }
            Stmt::Drop(slot) => {
                let _ = writeln!(out, "    drop _{slot}");
            }
            Stmt::DropValue(op) => {
                let _ = writeln!(out, "    drop_value {}", operand_str(op));
            }
            Stmt::ArenaEnd(op) => {
                let _ = writeln!(out, "    arena_end {}", operand_str(op));
            }
        }
    }
    match &b.term {
        Term::Goto(t) => {
            let _ = writeln!(out, "    goto bb{t}");
        }
        Term::Branch(c, t, e) => {
            let _ = writeln!(out, "    branch {} ? bb{t} : bb{e}", operand_str(c));
        }
        Term::Return(Some(op)) => {
            let _ = writeln!(out, "    return {}", operand_str(op));
        }
        Term::Return(None) => {
            let _ = writeln!(out, "    return");
        }
        Term::Unreachable => {
            let _ = writeln!(out, "    unreachable");
        }
    }
}

fn rvalue_str(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(op) => operand_str(op),
        Rvalue::Load(slot) => format!("load _{slot}"),
        Rvalue::Un(op, a) => format!("{}{}", unop_str(*op), operand_str(a)),
        Rvalue::Bin(op, a, b) => {
            format!("{} {} {}", operand_str(a), binop_str(*op), operand_str(b))
        }
        Rvalue::Call(name, args) => {
            let a: Vec<String> = args.iter().map(operand_str).collect();
            format!("call {name}({})", a.join(", "))
        }
        Rvalue::Field(slot, idx) => format!("_{slot}.{idx}"),
        Rvalue::OptionSome(op) => format!("Some({})", operand_str(op)),
        Rvalue::OptionNone => "None".to_string(),
        Rvalue::OptionIsSome(op) => format!("is_some({})", operand_str(op)),
        Rvalue::OptionUnwrap(op) => format!("unwrap({})", operand_str(op)),
        Rvalue::ResultOk(op) => format!("Ok({})", operand_str(op)),
        Rvalue::ResultErr(op) => format!("Err({})", operand_str(op)),
        Rvalue::ResultIsOk(op) => format!("is_ok({})", operand_str(op)),
        Rvalue::ResultUnwrapOk(op) => format!("unwrap_ok({})", operand_str(op)),
        Rvalue::ResultUnwrapErr(op) => format!("unwrap_err({})", operand_str(op)),
        Rvalue::ArenaBegin => "arena_begin".to_string(),
        Rvalue::HeapAlloc(h, init) => format!("heap_alloc({}, {})", operand_str(h), operand_str(init)),
        Rvalue::BoxGet(op) => format!("box_get({})", operand_str(op)),
        Rvalue::BoxClone(h, src) => format!("box_clone({}, {})", operand_str(h), operand_str(src)),
        Rvalue::Index(slot, idx) => format!("_{slot}[{}]", operand_str(idx)),
        Rvalue::IndexField(slot, idx, field) => format!("_{slot}[{}].{field}", operand_str(idx)),
        Rvalue::IndexFieldPtr { base, index, field, struct_id } => {
            format!("{}[{}].{field} (struct#{struct_id})", operand_str(base), operand_str(index))
        }
        Rvalue::MakeSlice(slot, n) => format!("slice(_{slot}, {n})"),
        Rvalue::ArenaAlloc { handle, count, elem } => {
            format!("arena_alloc({}, {} x {})", operand_str(handle), operand_str(count), crate::ty_name(*elem))
        }
        Rvalue::HeapAllocBuf { count, elem } => {
            format!("heap_alloc({} x {})", operand_str(count), crate::ty_name(*elem))
        }
        Rvalue::MakeDynArray { ptr, len } => {
            format!("array({}, {})", operand_str(ptr), operand_str(len))
        }
        Rvalue::SliceLen(op) => format!("slice_len({})", operand_str(op)),
        Rvalue::SlicePtr(op) => format!("slice_ptr({})", operand_str(op)),
        Rvalue::SliceIndex(s, idx) => format!("{}[{}]", operand_str(s), operand_str(idx)),
        Rvalue::StrLit(s) => format!("{s:?}"),
        Rvalue::StrClone(op) => format!("str_clone({})", operand_str(op)),
        Rvalue::BuilderNew => "builder_new".to_string(),
        Rvalue::BuilderWriteStr(b, s) => format!("builder_write({}, {})", operand_str(b), operand_str(s)),
        Rvalue::BuilderWriteInt(b, n) => format!("builder_write_int({}, {})", operand_str(b), operand_str(n)),
        Rvalue::BuilderWriteBool(b, v) => format!("builder_write_bool({}, {})", operand_str(b), operand_str(v)),
        Rvalue::BuilderWriteChar(b, c) => format!("builder_write_char({}, {})", operand_str(b), operand_str(c)),
        Rvalue::BuilderWriteFloat(b, x) => format!("builder_write_float({}, {})", operand_str(b), operand_str(x)),
        Rvalue::BuilderToString(op) => format!("builder_to_string({})", operand_str(op)),
        Rvalue::Template(pieces, _arena) => {
            let ps: Vec<String> = pieces
                .iter()
                .map(|p| match p {
                    crate::TemplatePiece::Static(s) => format!("{s:?}"),
                    crate::TemplatePiece::IntHole(o) => format!("int({})", operand_str(o)),
                    crate::TemplatePiece::StrHole(o) => format!("str({})", operand_str(o)),
                    crate::TemplatePiece::BoolHole(o) => format!("bool({})", operand_str(o)),
                    crate::TemplatePiece::CharHole(o) => format!("char({})", operand_str(o)),
                    crate::TemplatePiece::FloatHole(o) => format!("float({})", operand_str(o)),
                    crate::TemplatePiece::JsonStrHole(o) => format!("json_str({})", operand_str(o)),
                })
                .collect();
            format!("template[{}]", ps.join(", "))
        }
        Rvalue::JsonDecode { struct_id, input, out } => {
            format!("json_decode(struct#{struct_id}, {}, -> _{out})", operand_str(input))
        }
        Rvalue::JsonDecodeArray { elem, input, out } => {
            format!("json_decode_array({} x {}, -> _{out})", operand_str(input), crate::ty_name(*elem))
        }
        Rvalue::JsonDecodeStructArray { struct_id, input, out } => {
            format!("json_decode_struct_array(struct#{struct_id}, {}, -> _{out})", operand_str(input))
        }
        Rvalue::FsReadFile { path, out } => {
            format!("fs_read_file({}, -> _{out})", operand_str(path))
        }
    }
}

fn operand_str(op: &Operand) -> String {
    match op {
        Operand::Const(Const::Int(v, ty)) => format!("{v}_{}", ty_name(*ty)),
        Operand::Const(Const::Float(v, ty)) => format!("{v}_{}", ty_name(*ty)),
        Operand::Const(Const::Char(v)) => format!("'\\u{{{v:x}}}'"),
        Operand::Const(Const::Bool(v)) => v.to_string(),
        Operand::Const(Const::Unit) => "()".to_string(),
        Operand::Value(v) => format!("%{v}"),
        Operand::Arg(i) => format!("arg{i}"),
    }
}

fn unop_str(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "-",
        UnOp::Not => "!",
    }
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}
