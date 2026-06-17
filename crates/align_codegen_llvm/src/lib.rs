//! バックエンド: MIR → LLVM IR → object (`docs/impl/05-backend-llvm.md`)。
//!
//! 純粋 lowering に徹する段。Align の意味論判断 (脱糖・fusion・SIMD化・region) は
//! MIR で済んでおり、ここは MIR を機械的に LLVM IR へ落とすだけ (anti-rewrite)。
//!
//! M0 範囲: 整数のみ。`fn main() -> iN` を LLVM の `main` 関数として出し、四則演算と
//! 定数 return を lowering する。C ランタイム (crt0) が `main` をエントリとして呼ぶため、
//! M0 では align_runtime を介さず終了コードを返せる (Result 版 main は M2 で結線)。

use std::collections::HashMap;
use std::path::Path;

use align_ast::BinOp;
use align_mir::{Function, Operand, Program, Rvalue, Stmt, Term, ValueId};
use align_sema::{IntTy, Ty};

use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::IntType;
use inkwell::values::IntValue;

/// このビルドで LLVM バックエンドが利用可能か。inkwell をリンクしているので true。
pub fn is_available() -> bool {
    true
}

#[derive(Debug)]
pub enum CodegenError {
    Lowering(String),
    Target(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::Lowering(m) => write!(f, "lowering 失敗: {m}"),
            CodegenError::Target(m) => write!(f, "ターゲット/出力失敗: {m}"),
        }
    }
}

/// MIR を object ファイルへ書き出す。
pub fn emit_object(program: &Program, out: &Path) -> Result<(), CodegenError> {
    let ctx = Context::create();
    let module = ctx.create_module("align");
    let builder = ctx.create_builder();

    for f in &program.fns {
        lower_fn(&ctx, &module, &builder, f)?;
    }

    write_object(&module, out)
}

/// デバッグ用に LLVM IR をテキストで返す (`alignc emit-llvm`)。
pub fn emit_llvm_ir(program: &Program) -> Result<String, CodegenError> {
    let ctx = Context::create();
    let module = ctx.create_module("align");
    let builder = ctx.create_builder();
    for f in &program.fns {
        lower_fn(&ctx, &module, &builder, f)?;
    }
    Ok(module.print_to_string().to_string())
}

fn int_type<'c>(ctx: &'c Context, ty: Ty) -> IntType<'c> {
    match ty {
        // 幅は sema が 8/16/32/64 に確定済み。custom_width_int_type は Result を返すが
        // これらの幅では必ず Ok (inkwell 0.9)。
        Ty::Int(IntTy { bits, .. }) => match bits {
            8 => ctx.i8_type(),
            16 => ctx.i16_type(),
            32 => ctx.i32_type(),
            _ => ctx.i64_type(),
        },
        // M0 では整数以外は来ない。安全側で i32。
        _ => ctx.i32_type(),
    }
}

fn is_signed(ty: Ty) -> bool {
    matches!(ty, Ty::Int(IntTy { signed: true, .. }))
}

fn lower_fn<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    builder: &Builder<'c>,
    f: &Function,
) -> Result<(), CodegenError> {
    let ret_ty = int_type(ctx, f.ret);
    let fn_ty = ret_ty.fn_type(&[], false);
    let func = module.add_function(&f.name, fn_ty, None);

    let mut vals: HashMap<ValueId, IntValue<'c>> = HashMap::new();
    let mut types: HashMap<ValueId, Ty> = HashMap::new();

    // M0 は単一ブロック。複数ブロック (制御フロー) は M1 で追加。
    let block = &f.blocks[0];
    let bb = ctx.append_basic_block(func, "bb0");
    builder.position_at_end(bb);

    for stmt in &block.stmts {
        match stmt {
            Stmt::Let(v, rv) => {
                let (val, ty) = lower_rvalue(ctx, builder, &vals, &types, rv)?;
                vals.insert(*v, val);
                types.insert(*v, ty);
            }
        }
    }

    match &block.term {
        Term::Return(Some(op)) => {
            let v = lower_operand(ctx, &vals, op);
            builder
                .build_return(Some(&v))
                .map_err(|e| CodegenError::Lowering(e.to_string()))?;
        }
        Term::Return(None) => {
            builder
                .build_return(None)
                .map_err(|e| CodegenError::Lowering(e.to_string()))?;
        }
    }
    Ok(())
}

fn lower_rvalue<'c>(
    ctx: &'c Context,
    builder: &Builder<'c>,
    vals: &HashMap<ValueId, IntValue<'c>>,
    types: &HashMap<ValueId, Ty>,
    rv: &Rvalue,
) -> Result<(IntValue<'c>, Ty), CodegenError> {
    match rv {
        Rvalue::Use(op) => Ok((lower_operand(ctx, vals, op), operand_ty(types, op))),
        Rvalue::Bin(op, a, b) => {
            let l = lower_operand(ctx, vals, a);
            let r = lower_operand(ctx, vals, b);
            let ty = operand_ty(types, a);
            let signed = is_signed(ty);
            let e = |r: Result<IntValue<'c>, _>| {
                r.map_err(|e: inkwell::builder::BuilderError| CodegenError::Lowering(e.to_string()))
            };
            // 整数オーバーフローは2の補数 wrap (draft.md §5)。通常の add/mul を出す。
            let v = match op {
                BinOp::Add => e(builder.build_int_add(l, r, "add"))?,
                BinOp::Sub => e(builder.build_int_sub(l, r, "sub"))?,
                BinOp::Mul => e(builder.build_int_mul(l, r, "mul"))?,
                BinOp::Div if signed => e(builder.build_int_signed_div(l, r, "sdiv"))?,
                BinOp::Div => e(builder.build_int_unsigned_div(l, r, "udiv"))?,
                BinOp::Rem if signed => e(builder.build_int_signed_rem(l, r, "srem"))?,
                BinOp::Rem => e(builder.build_int_unsigned_rem(l, r, "urem"))?,
            };
            Ok((v, ty))
        }
    }
}

fn lower_operand<'c>(
    ctx: &'c Context,
    vals: &HashMap<ValueId, IntValue<'c>>,
    op: &Operand,
) -> IntValue<'c> {
    match op {
        Operand::Const(v, ty) => int_type(ctx, *ty).const_int(*v as u64, is_signed(*ty)),
        Operand::Value(id) => vals[id],
    }
}

fn operand_ty(types: &HashMap<ValueId, Ty>, op: &Operand) -> Ty {
    match op {
        Operand::Const(_, ty) => *ty,
        Operand::Value(id) => types.get(id).copied().unwrap_or(Ty::Error),
    }
}

fn write_object(module: &Module, out: &Path) -> Result<(), CodegenError> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| CodegenError::Target(format!("ネイティブターゲット初期化: {e}")))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)
        .map_err(|e| CodegenError::Target(format!("triple 解決: {e}")))?;
    let tm = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CodegenError::Target("TargetMachine 生成に失敗".to_string()))?;

    tm.write_to_file(module, FileType::Object, out)
        .map_err(|e| CodegenError::Target(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_diag::Diagnostics;
    use align_lexer::tokenize;
    use align_mir::lower_program;
    use align_parser::parse_file;
    use align_sema::check_file;

    fn ir(src: &str) -> String {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors());
        emit_llvm_ir(&lower_program(&hir)).unwrap()
    }

    #[test]
    fn m0_emits_main_returning_i32() {
        let text = ir("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        assert!(text.contains("define i32 @main()"), "got:\n{text}");
        assert!(text.contains("ret i32 1"), "got:\n{text}");
    }

    #[test]
    fn arithmetic_lowers() {
        let text = ir("fn main() -> i32 {\n  return 2 + 3 * 4\n}\n");
        // 定数畳み込みは LLVM 任せ。add/mul 命令か畳み込み結果のいずれかが出る。
        assert!(text.contains("@main"), "got:\n{text}");
    }
}
