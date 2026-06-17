//! 意味解析: 名前解決 + 型推論/検査 → typed HIR (`docs/impl/03-types.md`)。
//!
//! M0 範囲: 整数型のみ。局所推論 + 双方向型付けの最小版。整数リテラルは未制約の
//! 推論変数として扱い、文脈 (return の戻り型など) で具象幅に確定する。最後まで
//! 未制約なら既定 `i64` (`03-types.md` §2)。move 検査・arena 検査・効果検査は M3 以降。

use align_ast as ast;
use align_diag::Diagnostics;
use align_span::Span;

pub mod hir;
pub use hir::*;

/// 整数の幅と符号。`i32` = `IntTy { bits: 32, signed: true }`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntTy {
    pub bits: u8,
    pub signed: bool,
}

impl IntTy {
    pub fn name(&self) -> String {
        format!("{}{}", if self.signed { 'i' } else { 'u' }, self.bits)
    }
}

/// sema 内部の型表現 (`03-types.md` §1 の M0 部分集合)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int(IntTy),
    /// 未解決の整数 (推論変数)。最終的に具象 [`IntTy`] へ確定する。
    IntVar(u32),
    Unit,
    Error,
}

/// 解析済みプログラムを返す。エラーは `diags` に積む。
pub fn check_file(file: &ast::File, diags: &mut Diagnostics) -> Program {
    let mut cx = Checker {
        diags,
        int_vars: Vec::new(),
    };
    let fns = file
        .items
        .iter()
        .map(|ast::Item::Fn(f)| cx.check_fn(f))
        .collect();
    Program { fns }
}

struct Checker<'a> {
    diags: &'a mut Diagnostics,
    /// 整数推論変数の解決状態。`None` = 未確定。
    int_vars: Vec<Option<IntTy>>,
}

/// 関数本体のスコープ。M0 は単純な線形スコープ (シャドーイングは後続)。
struct Scope {
    locals: Vec<LocalInfo>,
}

struct LocalInfo {
    name: String,
    id: LocalId,
    ty: Ty,
}

impl<'a> Checker<'a> {
    fn fresh_int_var(&mut self) -> Ty {
        let id = self.int_vars.len() as u32;
        self.int_vars.push(None);
        Ty::IntVar(id)
    }

    /// 推論変数を可能な限り具象型へ解決する。
    fn resolve(&self, ty: Ty) -> Ty {
        match ty {
            Ty::IntVar(v) => match self.int_vars[v as usize] {
                Some(it) => Ty::Int(it),
                None => ty,
            },
            other => other,
        }
    }

    /// 最終的に未解決の整数変数は既定 `i64` に落とす。
    fn finalize(&self, ty: Ty) -> Ty {
        match self.resolve(ty) {
            Ty::IntVar(_) => Ty::Int(IntTy {
                bits: 64,
                signed: true,
            }),
            other => other,
        }
    }

    /// `ty` を具象整数 `target` に一致させる。矛盾があれば `Err`。
    fn unify_int(&mut self, ty: Ty, target: IntTy) -> Result<(), Ty> {
        match self.resolve(ty) {
            Ty::IntVar(v) => {
                self.int_vars[v as usize] = Some(target);
                Ok(())
            }
            Ty::Int(it) if it == target => Ok(()),
            Ty::Error => Ok(()),
            other => Err(other),
        }
    }

    fn check_fn(&mut self, f: &ast::FnDecl) -> Fn {
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Unit,
        };
        let mut scope = Scope { locals: Vec::new() };
        let mut next_local: u32 = 0;

        let body = match &f.body {
            ast::FnBody::Block(b) => self.check_block(b, ret, &mut scope, &mut next_local),
            ast::FnBody::Expr(e) => {
                // `= expr` 形: 本体式を戻り型で check し、暗黙 return とする。
                let te = self.check_expr(e, Some(ret), &mut scope, &mut next_local);
                vec![Stmt::Return(Some(te))]
            }
        };

        // 全 HIR の型を確定 (推論変数 → 具象 or 既定)。
        let mut body = body;
        for s in &mut body {
            self.finalize_stmt(s);
        }
        let locals = scope
            .locals
            .iter()
            .map(|l| Local {
                id: l.id,
                name: l.name.clone(),
                ty: self.finalize(l.ty),
            })
            .collect();

        Fn {
            name: f.name.name.clone(),
            ret: self.finalize(ret),
            locals,
            body,
            span: f.span,
        }
    }

    fn check_block(
        &mut self,
        b: &ast::Block,
        ret: Ty,
        scope: &mut Scope,
        next_local: &mut u32,
    ) -> Vec<Stmt> {
        let mut out = Vec::new();
        for s in &b.stmts {
            match s {
                ast::Stmt::Let { name, ty, init, .. } => {
                    let expected = ty.as_ref().map(|t| self.resolve_type(t));
                    let te = self.check_expr(init, expected, scope, next_local);
                    let local_ty = expected.unwrap_or(te.ty);
                    let id = *next_local;
                    *next_local += 1;
                    scope.locals.push(LocalInfo {
                        name: name.name.clone(),
                        id,
                        ty: local_ty,
                    });
                    out.push(Stmt::Let {
                        local: id,
                        ty: local_ty,
                        init: te,
                    });
                }
                ast::Stmt::Return(value) => {
                    let te = value
                        .as_ref()
                        .map(|e| self.check_expr(e, Some(ret), scope, next_local));
                    out.push(Stmt::Return(te));
                }
                ast::Stmt::Expr(e) => {
                    let te = self.check_expr(e, None, scope, next_local);
                    out.push(Stmt::Expr(te));
                }
                ast::Stmt::Assign { place, value } => {
                    // M0 では未使用だが、最低限の検査を通す。
                    let pe = self.check_expr(place, None, scope, next_local);
                    let ve = self.check_expr(value, Some(pe.ty), scope, next_local);
                    out.push(Stmt::Expr(ve));
                }
            }
        }
        if let Some(tail) = &b.tail {
            // ブロック末尾式 = ブロック値。M0 では関数末尾なら戻り値扱い。
            let te = self.check_expr(tail, Some(ret), scope, next_local);
            out.push(Stmt::Return(Some(te)));
        }
        out
    }

    fn check_expr(
        &mut self,
        e: &ast::Expr,
        expected: Option<Ty>,
        scope: &mut Scope,
        next_local: &mut u32,
    ) -> Expr {
        match &e.kind {
            ast::ExprKind::Int(v) => {
                let ty = self.fresh_int_var();
                if let Some(exp) = expected {
                    self.expect_int(ty, exp, e.span);
                }
                Expr {
                    kind: ExprKind::Int(*v),
                    ty,
                    span: e.span,
                }
            }
            ast::ExprKind::Path(p) => {
                let name = p
                    .segments
                    .last()
                    .map(|s| s.name.as_str())
                    .unwrap_or("");
                match scope.locals.iter().rev().find(|l| l.name == name) {
                    Some(l) => {
                        let ty = l.ty;
                        if let Some(exp) = expected {
                            self.expect_int(ty, exp, e.span);
                        }
                        Expr {
                            kind: ExprKind::Local(l.id),
                            ty,
                            span: e.span,
                        }
                    }
                    None => {
                        self.diags
                            .error(format!("未定義の名前: '{name}'"), e.span);
                        Expr {
                            kind: ExprKind::Int(0),
                            ty: Ty::Error,
                            span: e.span,
                        }
                    }
                }
            }
            ast::ExprKind::Binary { op, lhs, rhs } => {
                let l = self.check_expr(lhs, expected, scope, next_local);
                let r = self.check_expr(rhs, Some(l.ty), scope, next_local);
                let ty = l.ty;
                Expr {
                    kind: ExprKind::Binary {
                        op: *op,
                        lhs: Box::new(l),
                        rhs: Box::new(r),
                    },
                    ty,
                    span: e.span,
                }
            }
            ast::ExprKind::Block(b) => {
                // M0 では稀。中身を検査し、Unit とみなす最小実装。
                let _ = self.check_block(b, Ty::Unit, scope, next_local);
                Expr {
                    kind: ExprKind::Int(0),
                    ty: Ty::Unit,
                    span: e.span,
                }
            }
        }
    }

    /// `ty` を期待型 `exp` に一致させる。整数同士のみ unify、他は型不一致。
    fn expect_int(&mut self, ty: Ty, exp: Ty, span: Span) {
        match self.resolve(exp) {
            Ty::Int(it) => {
                if self.unify_int(ty, it).is_err() {
                    let got = self.resolve(ty);
                    self.diags.error(
                        format!("型不一致: {} が必要ですが {} です", it.name(), ty_name(got)),
                        span,
                    );
                }
            }
            Ty::IntVar(_) => {
                // 期待側も未確定。M0 では一方を他方に合わせる必要はない。
            }
            Ty::Unit => {
                self.diags
                    .error("型不一致: () が必要ですが整数です", span);
            }
            Ty::Error => {}
        }
    }

    fn resolve_type(&mut self, t: &ast::Type) -> Ty {
        let name = t
            .path
            .segments
            .last()
            .map(|s| s.name.as_str())
            .unwrap_or("");
        match parse_int_name(name) {
            Some(it) => Ty::Int(it),
            None if name == "()" => Ty::Unit,
            None => {
                self.diags
                    .error(format!("M0 では未対応の型: '{name}'"), t.span);
                Ty::Error
            }
        }
    }

    // --- finalize: HIR 内の型を具象化 ---

    fn finalize_stmt(&self, s: &mut Stmt) {
        match s {
            Stmt::Let { ty, init, .. } => {
                *ty = self.finalize(*ty);
                self.finalize_expr(init);
            }
            Stmt::Return(Some(e)) | Stmt::Expr(e) => self.finalize_expr(e),
            Stmt::Return(None) => {}
        }
    }

    fn finalize_expr(&self, e: &mut Expr) {
        e.ty = self.finalize(e.ty);
        if let ExprKind::Binary { lhs, rhs, .. } = &mut e.kind {
            self.finalize_expr(lhs);
            self.finalize_expr(rhs);
        }
    }
}

fn ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(it) => it.name(),
        Ty::IntVar(_) => "整数(未確定)".to_string(),
        Ty::Unit => "()".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

fn parse_int_name(name: &str) -> Option<IntTy> {
    let (signed, rest) = match name.as_bytes().first()? {
        b'i' => (true, &name[1..]),
        b'u' => (false, &name[1..]),
        _ => return None,
    };
    let bits: u8 = rest.parse().ok()?;
    matches!(bits, 8 | 16 | 32 | 64).then_some(IntTy { bits, signed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_lexer::tokenize;
    use align_parser::parse_file;

    fn check(src: &str) -> (Program, Diagnostics) {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let p = check_file(&f, &mut d);
        (p, d)
    }

    #[test]
    fn int_literal_infers_return_type() {
        // x := 1 は未制約 → return x が i32 を要求 → x: i32 に確定。
        let (p, d) = check("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        assert!(!d.has_errors(), "想定外のエラー");
        let f = &p.fns[0];
        assert_eq!(f.ret, Ty::Int(IntTy { bits: 32, signed: true }));
        assert_eq!(f.locals[0].ty, Ty::Int(IntTy { bits: 32, signed: true }));
    }

    #[test]
    fn unconstrained_int_defaults_to_i64() {
        let (p, d) = check("fn f() -> i64 {\n  y := 5\n  return y\n}\n");
        assert!(!d.has_errors());
        assert_eq!(p.fns[0].locals[0].ty, Ty::Int(IntTy { bits: 64, signed: true }));
    }

    #[test]
    fn undefined_name_errors() {
        let (_p, d) = check("fn f() -> i32 {\n  return z\n}\n");
        assert!(d.has_errors());
    }
}
