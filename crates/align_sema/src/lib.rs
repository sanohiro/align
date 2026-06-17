//! Semantic analysis: name resolution + type inference/checking -> typed HIR
//! (`docs/impl/03-types.md`).
//!
//! M0 scope: integer types only. Minimal local inference + bidirectional typing.
//! Integer literals are treated as unconstrained inference variables and fixed to a
//! concrete width by context (e.g. a return type). If still unconstrained at the
//! end, default to `i64` (`03-types.md` §2). Move/arena/effect checking is M3+.

use align_ast as ast;
use align_diag::Diagnostics;
use align_span::Span;

pub mod hir;
pub use hir::*;

/// Integer width and sign. `i32` = `IntTy { bits: 32, signed: true }`.
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

/// sema-internal type representation (the M0 subset of `03-types.md` §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int(IntTy),
    /// Unresolved integer (inference variable). Eventually fixed to a concrete [`IntTy`].
    IntVar(u32),
    Unit,
    Error,
}

/// Return the analyzed program. Errors are pushed to `diags`.
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
    /// Resolution state of integer inference variables. `None` = undetermined.
    int_vars: Vec<Option<IntTy>>,
}

/// Scope of a function body. M0 uses a simple linear scope (shadowing comes later).
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

    /// Resolve an inference variable to a concrete type as far as possible.
    fn resolve(&self, ty: Ty) -> Ty {
        match ty {
            Ty::IntVar(v) => match self.int_vars[v as usize] {
                Some(it) => Ty::Int(it),
                None => ty,
            },
            other => other,
        }
    }

    /// Fall back unresolved integer variables to the default `i64`.
    fn finalize(&self, ty: Ty) -> Ty {
        match self.resolve(ty) {
            Ty::IntVar(_) => Ty::Int(IntTy {
                bits: 64,
                signed: true,
            }),
            other => other,
        }
    }

    /// Unify `ty` with the concrete integer `target`. `Err` on conflict.
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
                // `= expr` form: check the body expression against the return type and make it an implicit return.
                let te = self.check_expr(e, Some(ret), &mut scope, &mut next_local);
                vec![Stmt::Return(Some(te))]
            }
        };

        // Finalize all HIR types (inference variables -> concrete or default).
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
                    // Unused in M0, but run a minimal check anyway.
                    let pe = self.check_expr(place, None, scope, next_local);
                    let ve = self.check_expr(value, Some(pe.ty), scope, next_local);
                    out.push(Stmt::Expr(ve));
                }
            }
        }
        if let Some(tail) = &b.tail {
            // Block trailing expression = block value. In M0, treat it as the return value at function end.
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
                            .error(format!("undefined name: '{name}'"), e.span);
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
                // Rare in M0. Minimal impl: check the contents and treat as Unit.
                let _ = self.check_block(b, Ty::Unit, scope, next_local);
                Expr {
                    kind: ExprKind::Int(0),
                    ty: Ty::Unit,
                    span: e.span,
                }
            }
        }
    }

    /// Match `ty` against the expected type `exp`. Unify only integer-vs-integer; otherwise type mismatch.
    fn expect_int(&mut self, ty: Ty, exp: Ty, span: Span) {
        match self.resolve(exp) {
            Ty::Int(it) => {
                if self.unify_int(ty, it).is_err() {
                    let got = self.resolve(ty);
                    self.diags.error(
                        format!("type mismatch: expected {} but found {}", it.name(), ty_name(got)),
                        span,
                    );
                }
            }
            Ty::IntVar(_) => {
                // Expected side is also undetermined. In M0 there's no need to unify one to the other.
            }
            Ty::Unit => {
                self.diags
                    .error("type mismatch: expected () but found an integer", span);
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
                    .error(format!("type not supported in M0: '{name}'"), t.span);
                Ty::Error
            }
        }
    }

    // --- finalize: make HIR types concrete ---

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
        Ty::IntVar(_) => "integer (undetermined)".to_string(),
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
        // x := 1 is unconstrained -> return x requires i32 -> x: i32 is fixed.
        let (p, d) = check("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        assert!(!d.has_errors(), "unexpected error");
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
