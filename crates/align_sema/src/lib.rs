//! Semantic analysis: name resolution + type inference/checking -> typed HIR
//! (`docs/impl/03-types.md`).
//!
//! M1 scope: integer types, `bool`, functions with parameters + calls, `if`,
//! comparison/logical operators, and `mut` reassignment. Local inference +
//! bidirectional typing. Integer literals are unconstrained inference variables fixed
//! to a concrete width by context; if still unconstrained at the end, default to `i64`
//! (`03-types.md` §2). Move/arena/effect checking is M3+.

use std::collections::HashMap;

use align_ast::{self as ast, BinOp, UnOp};
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

/// Floating-point width. `f64` = `FloatTy { bits: 64 }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FloatTy {
    pub bits: u8,
}

impl FloatTy {
    pub fn name(&self) -> String {
        format!("f{}", self.bits)
    }
}

/// A variable-free scalar type — the only payloads M2 allows inside `Option`/`Result`.
/// Keeping it `Copy` and non-recursive lets [`Ty`] stay `Copy` (no boxing/interning).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
    Unit,
    /// The M2 `Error` type — an opaque i32 error code (placeholder for the eventual
    /// Error sum type; see `open-questions.md`).
    ErrCode,
}

/// sema-internal type representation (`03-types.md` §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    Int(IntTy),
    /// Unresolved integer (inference variable). Eventually fixed to a concrete [`IntTy`].
    IntVar(u32),
    Float(FloatTy),
    /// Unresolved float (inference variable). Eventually fixed to a concrete [`FloatTy`].
    FloatVar(u32),
    Bool,
    /// A Unicode scalar value (32-bit).
    Char,
    /// `Option<T>`; the payload is a concrete scalar (M2 restriction).
    Option(Scalar),
    /// `Result<T, E>`; both payloads are concrete scalars (M2 restriction).
    Result(Scalar, Scalar),
    /// The `Error` type (M2: an i32 code).
    ErrCode,
    /// A struct type; the id indexes `Program::structs`.
    Struct(u32),
    Unit,
    /// Type-checking error sentinel (bottom). Distinct from the `Error` *type*
    /// ([`Ty::ErrCode`]).
    Error,
}

/// Convert a concrete scalar [`Ty`] to a [`Scalar`]; `None` for vars/composites/structs.
fn ty_to_scalar(ty: Ty) -> Option<Scalar> {
    match ty {
        Ty::Int(it) => Some(Scalar::Int(it)),
        Ty::Float(ft) => Some(Scalar::Float(ft)),
        Ty::Bool => Some(Scalar::Bool),
        Ty::Char => Some(Scalar::Char),
        Ty::Unit => Some(Scalar::Unit),
        Ty::ErrCode => Some(Scalar::ErrCode),
        _ => None,
    }
}

pub fn scalar_to_ty(s: Scalar) -> Ty {
    match s {
        Scalar::Int(it) => Ty::Int(it),
        Scalar::Float(ft) => Ty::Float(ft),
        Scalar::Bool => Ty::Bool,
        Scalar::Char => Ty::Char,
        Scalar::Unit => Ty::Unit,
        Scalar::ErrCode => Ty::ErrCode,
    }
}

fn scalar_name(s: Scalar) -> String {
    ty_name(scalar_to_ty(s))
}

impl Ty {
    fn is_int_like(self) -> bool {
        matches!(self, Ty::Int(_) | Ty::IntVar(_))
    }

    fn is_float_like(self) -> bool {
        matches!(self, Ty::Float(_) | Ty::FloatVar(_))
    }

    fn is_numeric(self) -> bool {
        self.is_int_like() || self.is_float_like()
    }
}

struct FnSig {
    params: Vec<Ty>,
    ret: Ty,
}

/// An assignable location resolved by [`Checker::check_place`].
enum Place {
    Local { id: LocalId, ty: Ty },
    Field { base: LocalId, index: u32, ty: Ty },
    Err,
}

/// Analyze a file into a typed program. Errors are pushed to `diags`.
pub fn check_file(file: &ast::File, diags: &mut Diagnostics) -> Program {
    // Pass 0a: assign an id to every struct name (so field/sig types can refer to them
    // regardless of order).
    let mut struct_ids: HashMap<String, u32> = HashMap::new();
    let mut struct_decls: Vec<&ast::StructDecl> = Vec::new();
    for item in &file.items {
        if let ast::Item::Struct(s) = item {
            if struct_ids.insert(s.name.name.clone(), struct_decls.len() as u32).is_some() {
                diags.error(format!("duplicate type declaration: '{}'", s.name.name), s.span);
            }
            struct_decls.push(s);
        }
    }

    // Pass 0b: resolve field types. M1 restricts struct fields to primitives.
    let structs: Vec<StructDef> = struct_decls
        .iter()
        .map(|s| {
            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let ty = resolve_type(&f.ty, &struct_ids, diags);
                    if matches!(ty, Ty::Struct(_)) {
                        diags.error(
                            "struct fields must be primitive types in M1 (nested structs come later)"
                                .to_string(),
                            f.span,
                        );
                    }
                    FieldDef { name: f.name.name.clone(), ty }
                })
                .collect();
            StructDef { name: s.name.name.clone(), fields }
        })
        .collect();

    // Pass 1: collect function signatures so calls can resolve regardless of order.
    let mut sigs: HashMap<String, FnSig> = HashMap::new();
    for item in &file.items {
        let ast::Item::Fn(f) = item else { continue };
        let params = f
            .params
            .iter()
            .map(|p| resolve_type(&p.ty, &struct_ids, diags))
            .collect();
        let ret = match &f.ret {
            Some(t) => resolve_type(t, &struct_ids, diags),
            None => Ty::Unit,
        };
        sigs.insert(f.name.name.clone(), FnSig { params, ret });
    }

    // Pass 2: check each function body.
    let fns = file
        .items
        .iter()
        .filter_map(|item| {
            let ast::Item::Fn(f) = item else { return None };
            let mut cx = Checker {
                diags,
                sigs: &sigs,
                struct_ids: &struct_ids,
                structs: &structs,
                int_vars: Vec::new(),
                float_vars: Vec::new(),
                locals: Vec::new(),
                scope: Vec::new(),
                ret_hint: Ty::Unit,
            };
            Some(cx.check_fn(f))
        })
        .collect();
    Program { fns, structs }
}

struct Checker<'a> {
    diags: &'a mut Diagnostics,
    sigs: &'a HashMap<String, FnSig>,
    struct_ids: &'a HashMap<String, u32>,
    structs: &'a [StructDef],
    int_vars: Vec<Option<IntTy>>,
    float_vars: Vec<Option<FloatTy>>,
    /// All locals of the current function (slots), never shrinks.
    locals: Vec<Local>,
    /// Visibility stack: (name, id). Truncated on block exit.
    scope: Vec<(String, LocalId)>,
    /// Enclosing function's return type, so `return` checks against it.
    ret_hint: Ty,
}

impl<'a> Checker<'a> {
    fn fresh_int_var(&mut self) -> Ty {
        let id = self.int_vars.len() as u32;
        self.int_vars.push(None);
        Ty::IntVar(id)
    }

    fn fresh_float_var(&mut self) -> Ty {
        let id = self.float_vars.len() as u32;
        self.float_vars.push(None);
        Ty::FloatVar(id)
    }

    fn resolve(&self, ty: Ty) -> Ty {
        match ty {
            Ty::IntVar(v) => match self.int_vars[v as usize] {
                Some(it) => Ty::Int(it),
                None => ty,
            },
            Ty::FloatVar(v) => match self.float_vars[v as usize] {
                Some(ft) => Ty::Float(ft),
                None => ty,
            },
            other => other,
        }
    }

    fn finalize(&self, ty: Ty) -> Ty {
        match self.resolve(ty) {
            Ty::IntVar(_) => Ty::Int(IntTy {
                bits: 64,
                signed: true,
            }),
            Ty::FloatVar(_) => Ty::Float(FloatTy { bits: 64 }),
            other => other,
        }
    }

    /// Unify two types, returning the resolved type. Pushes a diagnostic on mismatch.
    fn unify(&mut self, a: Ty, b: Ty, span: Span) -> Ty {
        let (a, b) = (self.resolve(a), self.resolve(b));
        match (a, b) {
            (Ty::Error, _) | (_, Ty::Error) => Ty::Error,
            (Ty::IntVar(v), Ty::Int(it)) | (Ty::Int(it), Ty::IntVar(v)) => {
                self.int_vars[v as usize] = Some(it);
                Ty::Int(it)
            }
            (Ty::IntVar(_), Ty::IntVar(_)) => a, // both unconstrained; resolve later
            (Ty::FloatVar(v), Ty::Float(ft)) | (Ty::Float(ft), Ty::FloatVar(v)) => {
                self.float_vars[v as usize] = Some(ft);
                Ty::Float(ft)
            }
            (Ty::FloatVar(_), Ty::FloatVar(_)) => a,
            _ if a == b => a,
            _ => {
                self.diags.error(
                    format!("type mismatch: {} vs {}", ty_name(a), ty_name(b)),
                    span,
                );
                Ty::Error
            }
        }
    }

    /// Constrain `ty` to an expected type if one is given.
    fn constrain(&mut self, ty: Ty, expected: Option<Ty>, span: Span) {
        if let Some(exp) = expected {
            self.unify(ty, exp, span);
        }
    }

    // --- locals / scopes ---

    fn declare(&mut self, name: &str, ty: Ty, is_mut: bool) -> LocalId {
        let id = self.locals.len() as LocalId;
        self.locals.push(Local {
            id,
            name: name.to_string(),
            ty,
            is_mut,
        });
        self.scope.push((name.to_string(), id));
        id
    }

    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.scope
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    fn check_fn(&mut self, f: &ast::FnDecl) -> Fn {
        let sig = &self.sigs[&f.name.name];
        let ret = sig.ret;
        let param_tys = sig.params.clone();
        self.ret_hint = ret;

        let mut params = Vec::new();
        for (p, ty) in f.params.iter().zip(param_tys) {
            let id = self.declare(&p.name.name, ty, false);
            params.push(id);
        }

        let body = match &f.body {
            ast::FnBody::Block(b) => self.check_block(b, Some(ret)),
            ast::FnBody::Expr(e) => {
                let value = self.check_expr(e, Some(ret));
                Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(value)),
                }
            }
        };

        // Finalize all inferred types to concrete (or default i64).
        let mut body = body;
        self.finalize_block(&mut body);
        let mut locals = std::mem::take(&mut self.locals);
        for l in &mut locals {
            l.ty = self.finalize(l.ty);
        }

        Fn {
            name: f.name.name.clone(),
            params,
            ret: self.finalize(ret),
            locals,
            body,
            span: f.span,
        }
    }

    /// Check a block. `expected` is the expected type of its trailing value (if any).
    fn check_block(&mut self, b: &ast::Block, expected: Option<Ty>) -> Block {
        let scope_mark = self.scope.len();
        let mut stmts = Vec::new();

        for s in &b.stmts {
            match s {
                ast::Stmt::Let { is_mut, name, ty, init } => {
                    let ann = ty.as_ref().map(|t| self.resolve_type(t));
                    // A struct literal is only legal here, as a `let` initializer.
                    let init = match &init.kind {
                        ast::ExprKind::StructLit { name: sname, fields } => {
                            self.check_struct_lit(sname, fields, init.span)
                        }
                        _ => self.check_expr(init, ann),
                    };
                    let local_ty = ann.unwrap_or(init.ty);
                    let local = self.declare(&name.name, local_ty, *is_mut);
                    stmts.push(Stmt::Let { local, init });
                }
                ast::Stmt::Return(value) => {
                    // The enclosing function's return type is the expected one. We
                    // thread it via `expected` of the body block (M1: one level).
                    let v = value.as_ref().map(|e| self.check_expr(e, Some(self.ret_hint)));
                    stmts.push(Stmt::Return(v));
                }
                ast::Stmt::Expr(e) => {
                    let te = self.check_expr(e, None);
                    stmts.push(Stmt::Expr(te));
                }
                ast::Stmt::Assign { place, value } => match self.check_place(place) {
                    Place::Local { id, ty } => {
                        let v = self.check_expr(value, Some(ty));
                        stmts.push(Stmt::Assign { local: id, value: v });
                    }
                    Place::Field { base, index, ty } => {
                        let v = self.check_expr(value, Some(ty));
                        stmts.push(Stmt::AssignField { base, index, value: v });
                    }
                    Place::Err => {
                        let v = self.check_expr(value, None);
                        stmts.push(Stmt::Expr(v));
                    }
                },
            }
        }

        let value = b
            .tail
            .as_ref()
            .map(|e| Box::new(self.check_expr(e, expected)));
        self.scope.truncate(scope_mark);
        Block { stmts, value }
    }

    fn resolve_type(&mut self, t: &ast::Type) -> Ty {
        resolve_type(t, self.struct_ids, self.diags)
    }

    /// Resolve an assignable place: a `mut` local, or `mut_local.field`.
    fn check_place(&mut self, place: &ast::Expr) -> Place {
        let ast::ExprKind::Path(p) = &place.kind else {
            self.diags.error("invalid assignment target", place.span);
            return Place::Err;
        };
        let base_name = p.segments.first().map(|s| s.name.as_str()).unwrap_or("");
        let Some(id) = self.lookup(base_name) else {
            self.diags
                .error(format!("undefined name: '{base_name}'"), place.span);
            return Place::Err;
        };
        let local = &self.locals[id as usize];
        let (is_mut, local_ty) = (local.is_mut, local.ty);
        if !is_mut {
            self.diags.error(
                format!("cannot assign to immutable '{base_name}' (declare with `mut`)"),
                place.span,
            );
        }
        match p.segments.len() {
            1 => {
                if matches!(local_ty, Ty::Struct(_)) {
                    self.diags.error(
                        "cannot assign a whole struct in M1; assign individual fields".to_string(),
                        place.span,
                    );
                    return Place::Err;
                }
                Place::Local { id, ty: local_ty }
            }
            2 => {
                let field = &p.segments[1];
                match self.field_of(local_ty, &field.name, place.span) {
                    Some((index, ty)) => Place::Field { base: id, index, ty },
                    None => Place::Err,
                }
            }
            _ => {
                self.diags
                    .error("nested field access is not supported in M1".to_string(), place.span);
                Place::Err
            }
        }
    }

    /// Resolve `(field_index, field_type)` for `ty.name`, reporting errors against `span`.
    fn field_of(&mut self, ty: Ty, name: &str, span: Span) -> Option<(u32, Ty)> {
        let Ty::Struct(id) = ty else {
            if ty != Ty::Error {
                self.diags
                    .error(format!("type {} has no fields", ty_name(ty)), span);
            }
            return None;
        };
        let def = &self.structs[id as usize];
        match def.field_index(name) {
            Some(idx) => Some((idx, def.fields[idx as usize].ty)),
            None => {
                self.diags
                    .error(format!("no field '{name}' on '{}'", def.name), span);
                None
            }
        }
    }

    fn check_expr(&mut self, e: &ast::Expr, expected: Option<Ty>) -> Expr {
        match &e.kind {
            ast::ExprKind::Unit => {
                self.constrain(Ty::Unit, expected, e.span);
                Expr { kind: ExprKind::Unit, ty: Ty::Unit, span: e.span }
            }
            ast::ExprKind::Int(v) => {
                let ty = self.fresh_int_var();
                self.constrain(ty, expected, e.span);
                Expr { kind: ExprKind::Int(*v), ty, span: e.span }
            }
            ast::ExprKind::Float(v) => {
                let ty = self.fresh_float_var();
                self.constrain(ty, expected, e.span);
                Expr { kind: ExprKind::Float(*v), ty, span: e.span }
            }
            ast::ExprKind::Char(v) => {
                self.constrain(Ty::Char, expected, e.span);
                Expr { kind: ExprKind::Char(*v), ty: Ty::Char, span: e.span }
            }
            ast::ExprKind::Bool(b) => {
                self.constrain(Ty::Bool, expected, e.span);
                Expr { kind: ExprKind::Bool(*b), ty: Ty::Bool, span: e.span }
            }
            ast::ExprKind::Path(p) => self.check_path(p, expected, e.span),
            ast::ExprKind::Unary { op, expr } => {
                let inner = self.check_expr(expr, expected);
                let ty = match op {
                    UnOp::Neg => {
                        if !inner.ty.is_numeric() && inner.ty != Ty::Error {
                            self.diags.error("unary '-' expects a number", e.span);
                        }
                        inner.ty
                    }
                    UnOp::Not => {
                        self.unify(inner.ty, Ty::Bool, e.span);
                        Ty::Bool
                    }
                };
                Expr { kind: ExprKind::Unary { op: *op, expr: Box::new(inner) }, ty, span: e.span }
            }
            ast::ExprKind::Binary { op, lhs, rhs } => self.check_binary(*op, lhs, rhs, expected, e.span),
            ast::ExprKind::Call { callee, args } => self.check_call(callee, args, expected, e.span),
            ast::ExprKind::ElseUnwrap { opt, fallback } => {
                self.check_else_unwrap(opt, fallback, expected, e.span)
            }
            ast::ExprKind::Try(inner) => self.check_try(inner, e.span),
            ast::ExprKind::StructLit { .. } => {
                self.diags.error(
                    "struct literals are only allowed as `name := Type { ... }` in M1".to_string(),
                    e.span,
                );
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span }
            }
            ast::ExprKind::If { cond, then, els } => self.check_if(cond, then, els.as_deref(), expected, e.span),
            ast::ExprKind::Block(b) => {
                let block = self.check_block(b, expected);
                let ty = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                Expr { kind: ExprKind::Block(block), ty, span: e.span }
            }
        }
    }

    fn check_path(&mut self, p: &ast::Path, expected: Option<Ty>, span: Span) -> Expr {
        let err = |s: Span| Expr { kind: ExprKind::Local(u32::MAX), ty: Ty::Error, span: s };
        // `None` builtin: its Option type comes from context.
        if single_name(p) == Some("None") {
            return match expected {
                Some(Ty::Option(s)) => Expr { kind: ExprKind::OptionNone, ty: Ty::Option(s), span },
                _ => {
                    self.diags
                        .error("cannot infer the Option type of `None` here (add an annotation)".to_string(), span);
                    Expr { kind: ExprKind::OptionNone, ty: Ty::Error, span }
                }
            };
        }
        let base = p.segments.first().map(|s| s.name.as_str()).unwrap_or("");
        let Some(id) = self.lookup(base) else {
            self.diags.error(format!("undefined name: '{base}'"), span);
            return err(span);
        };
        let local_ty = self.locals[id as usize].ty;
        match p.segments.len() {
            1 => {
                if matches!(local_ty, Ty::Struct(_)) {
                    self.diags.error(
                        "cannot use a struct value directly in M1 (access its fields)".to_string(),
                        span,
                    );
                    return err(span);
                }
                self.constrain(local_ty, expected, span);
                Expr { kind: ExprKind::Local(id), ty: local_ty, span }
            }
            2 => match self.field_of(local_ty, &p.segments[1].name, span) {
                Some((index, ty)) => {
                    self.constrain(ty, expected, span);
                    Expr { kind: ExprKind::Field { base: id, index }, ty, span }
                }
                None => err(span),
            },
            _ => {
                self.diags
                    .error("nested field access is not supported in M1".to_string(), span);
                err(span)
            }
        }
    }

    /// `Name { field: value, ... }`. Reorders inits into declaration order and requires
    /// every field exactly once. Only reached from a `let` initializer (M1).
    fn check_struct_lit(&mut self, name: &ast::Ident, fields: &[ast::FieldInit], span: Span) -> Expr {
        let Some(&id) = self.struct_ids.get(&name.name) else {
            self.diags
                .error(format!("undefined type: '{}'", name.name), name.span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        let layout: Vec<(String, Ty)> = self.structs[id as usize]
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.ty))
            .collect();
        let sname = self.structs[id as usize].name.clone();

        let mut values: Vec<Option<Expr>> = (0..layout.len()).map(|_| None).collect();
        for fi in fields {
            match layout.iter().position(|(n, _)| *n == fi.name.name) {
                Some(idx) => {
                    if values[idx].is_some() {
                        self.diags
                            .error(format!("duplicate field '{}'", fi.name.name), fi.span);
                    }
                    values[idx] = Some(self.check_expr(&fi.value, Some(layout[idx].1)));
                }
                None => {
                    self.diags
                        .error(format!("no field '{}' on '{sname}'", fi.name.name), fi.span);
                    let _ = self.check_expr(&fi.value, None);
                }
            }
        }

        let mut out = Vec::with_capacity(layout.len());
        for (idx, v) in values.into_iter().enumerate() {
            match v {
                Some(e) => out.push(e),
                None => {
                    self.diags
                        .error(format!("missing field '{}' in '{sname}'", layout[idx].0), span);
                    out.push(Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span });
                }
            }
        }
        Expr { kind: ExprKind::StructLit { struct_id: id, fields: out }, ty: Ty::Struct(id), span }
    }

    fn check_binary(&mut self, op: BinOp, lhs: &ast::Expr, rhs: &ast::Expr, expected: Option<Ty>, span: Span) -> Expr {
        let ty;
        let (l, r);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                l = self.check_expr(lhs, expected);
                r = self.check_expr(rhs, Some(l.ty));
                let t = self.unify(l.ty, r.ty, span);
                if !t.is_numeric() && t != Ty::Error {
                    self.diags.error("arithmetic expects numbers (int or float)", span);
                }
                ty = t;
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                l = self.check_expr(lhs, None);
                r = self.check_expr(rhs, Some(l.ty));
                self.unify(l.ty, r.ty, span);
                ty = Ty::Bool;
            }
            BinOp::And | BinOp::Or => {
                l = self.check_expr(lhs, Some(Ty::Bool));
                r = self.check_expr(rhs, Some(Ty::Bool));
                ty = Ty::Bool;
            }
        }
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::Binary { op, lhs: Box::new(l), rhs: Box::new(r) }, ty, span }
    }

    fn check_call(&mut self, callee: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let name = match &callee.kind {
            ast::ExprKind::Path(p) => single_name(p).map(|s| s.to_string()),
            _ => None,
        };
        let Some(name) = name else {
            self.diags.error("only direct function calls are supported", span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        if name == "print" {
            return self.check_print(args, span);
        }
        if name == "Some" {
            return self.check_some(args, expected, span);
        }
        if name == "Ok" || name == "Err" {
            return self.check_result_ctor(&name, args, expected, span);
        }
        if name == "error" {
            return self.check_error_ctor(args, span);
        }
        let Some(sig) = self.sigs.get(&name) else {
            self.diags.error(format!("undefined function: '{name}'"), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        let (param_tys, ret) = (sig.params.clone(), sig.ret);
        if args.len() != param_tys.len() {
            self.diags.error(
                format!("'{name}' expects {} argument(s), got {}", param_tys.len(), args.len()),
                span,
            );
        }
        let checked = args
            .iter()
            .enumerate()
            .map(|(i, a)| self.check_expr(a, param_tys.get(i).copied()))
            .collect();
        Expr { kind: ExprKind::Call { func: name, args: checked }, ty: ret, span }
    }

    /// Builtin `print`. M1: exactly one integer argument; prints decimal + newline,
    /// returns `()`. `bool`/string and a no-newline form arrive with `std.io` (M5).
    fn check_print(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'print' expects 1 argument, got {}", args.len()), span);
        }
        let checked = args
            .iter()
            .map(|a| {
                let e = self.check_expr(a, None);
                if !e.ty.is_int_like() && e.ty != Ty::Error {
                    self.diags
                        .error("'print' expects an integer (M1)".to_string(), e.span);
                }
                e
            })
            .collect();
        Expr {
            kind: ExprKind::Call { func: "print".to_string(), args: checked },
            ty: Ty::Unit,
            span,
        }
    }

    /// Builtin `Some(x)`. The payload resolves to a concrete scalar here (an
    /// unconstrained literal defaults), so the resulting `Option<T>` carries no
    /// inference variable.
    fn check_some(&mut self, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'Some' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let inner_expected = match expected {
            Some(Ty::Option(s)) => Some(scalar_to_ty(s)),
            _ => None,
        };
        let arg = self.check_expr(&args[0], inner_expected);
        let scalar = self.payload_scalar(arg.ty, args[0].span);
        let ty = Ty::Option(scalar);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::OptionSome(Box::new(arg)), ty, span }
    }

    /// Resolve a type to a concrete payload [`Scalar`], defaulting inference vars and
    /// reporting non-scalar payloads (M2 restriction).
    fn payload_scalar(&mut self, ty: Ty, span: Span) -> Scalar {
        let f = self.finalize(ty);
        match ty_to_scalar(f) {
            Some(s) => s,
            None => {
                if f != Ty::Error {
                    self.diags
                        .error(format!("Option payload must be a scalar in M2, got {}", ty_name(f)), span);
                }
                Scalar::Int(IntTy { bits: 64, signed: true })
            }
        }
    }

    /// Builtin `error(code)` → an `Error` value (M2: an i32 code).
    fn check_error_ctor(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'error' takes 1 argument, got {}", args.len()), span);
        }
        let arg = args
            .first()
            .map(|a| self.check_expr(a, Some(Ty::Int(IntTy { bits: 32, signed: true }))));
        let args_hir = arg.into_iter().collect();
        // Lower as a plain call to the runtime-less builtin; codegen treats `error` as
        // identity on the i32 code, but the Align type is `Error`.
        Expr { kind: ExprKind::Call { func: "error".to_string(), args: args_hir }, ty: Ty::ErrCode, span }
    }

    /// Builtins `Ok(x)` / `Err(e)`. Both payload types come from the expected
    /// `Result<T, E>` (so both arms are typed even though only one is supplied).
    fn check_result_ctor(&mut self, name: &str, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'{name}' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let (ok_exp, err_exp) = match expected {
            Some(Ty::Result(o, e)) => (Some(scalar_to_ty(o)), Some(scalar_to_ty(e))),
            _ => (None, None),
        };
        let is_ok = name == "Ok";
        let arg = self.check_expr(&args[0], if is_ok { ok_exp } else { err_exp });
        let arg_scalar = self.payload_scalar(arg.ty, args[0].span);

        // The other arm's scalar must be known from context; otherwise we cannot form
        // a complete Result type (M2 limitation).
        let other = if is_ok { err_exp } else { ok_exp };
        let other_scalar = match other.and_then(ty_to_scalar) {
            Some(s) => s,
            None => {
                self.diags.error(
                    format!("cannot infer the full Result type of `{name}` here (annotate the return type)"),
                    span,
                );
                return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
            }
        };
        let (ty, kind) = if is_ok {
            (Ty::Result(arg_scalar, other_scalar), ExprKind::ResultOk(Box::new(arg)))
        } else {
            (Ty::Result(other_scalar, arg_scalar), ExprKind::ResultErr(Box::new(arg)))
        };
        self.constrain(ty, expected, span);
        Expr { kind, ty, span }
    }

    /// `expr?` — propagate. The operand must be `Result<T, E>` and the enclosing
    /// function must return `Result<_, E>` (same `E`). Yields `T`.
    fn check_try(&mut self, inner: &ast::Expr, span: Span) -> Expr {
        let v = self.check_expr(inner, None);
        let (ok, err) = match self.resolve(v.ty) {
            Ty::Result(o, e) => (o, e),
            Ty::Error => return Expr { kind: ExprKind::Try(Box::new(v)), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("`?` expects a Result, got {}", ty_name(other)), span);
                return Expr { kind: ExprKind::Try(Box::new(v)), ty: Ty::Error, span };
            }
        };
        match self.resolve(self.ret_hint) {
            Ty::Result(_, ret_err) if ret_err == err => {}
            Ty::Result(_, ret_err) => self.diags.error(
                format!(
                    "`?` error type {} does not match the function's error type {}",
                    scalar_name(err),
                    scalar_name(ret_err)
                ),
                span,
            ),
            _ => self.diags.error(
                "`?` can only be used in a function that returns a Result".to_string(),
                span,
            ),
        }
        Expr { kind: ExprKind::Try(Box::new(v)), ty: scalar_to_ty(ok), span }
    }

    /// `opt else fallback`. The fallback either yields the payload type or diverges via
    /// `return` (only the braced `else { … }` form is supported in M2).
    fn check_else_unwrap(&mut self, opt: &ast::Expr, fallback: &ast::Expr, expected: Option<Ty>, span: Span) -> Expr {
        let o = self.check_expr(opt, None);
        let payload = match self.resolve(o.ty) {
            Ty::Option(s) => scalar_to_ty(s),
            Ty::Error => Ty::Error,
            other => {
                self.diags
                    .error(format!("`else` unwrap expects an Option, got {}", ty_name(other)), span);
                Ty::Error
            }
        };
        // A diverging `{ … return … }` block has no value; don't constrain it to payload.
        let fb = if block_diverges(fallback) {
            self.check_expr(fallback, None)
        } else {
            self.check_expr(fallback, Some(payload))
        };
        self.constrain(payload, expected, span);
        Expr { kind: ExprKind::ElseUnwrap { opt: Box::new(o), fallback: Box::new(fb) }, ty: payload, span }
    }

    fn check_if(&mut self, cond: &ast::Expr, then: &ast::Block, els: Option<&ast::Expr>, expected: Option<Ty>, span: Span) -> Expr {
        let c = self.check_expr(cond, Some(Ty::Bool));
        let then_b = self.check_block(then, expected);
        let els_b = match els {
            Some(ast::Expr { kind: ast::ExprKind::Block(b), .. }) => self.check_block(b, expected),
            Some(e) => {
                // `else if` chain: check as an expression and wrap as a block value.
                let v = self.check_expr(e, expected);
                Block { stmts: Vec::new(), value: Some(Box::new(v)) }
            }
            None => Block { stmts: Vec::new(), value: None },
        };

        // If both branches produce a value, the if has that (unified) type; else Unit.
        let ty = match (&then_b.value, &els_b.value) {
            (Some(t), Some(e)) => self.unify(t.ty, e.ty, span),
            _ => Ty::Unit,
        };
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::If { cond: Box::new(c), then: then_b, els: els_b }, ty, span }
    }

    // --- finalize ---

    fn finalize_block(&self, b: &mut Block) {
        for s in &mut b.stmts {
            match s {
                Stmt::Let { init, .. } => self.finalize_expr(init),
                Stmt::Assign { value, .. } => self.finalize_expr(value),
                Stmt::AssignField { value, .. } => self.finalize_expr(value),
                Stmt::Return(Some(e)) | Stmt::Expr(e) => self.finalize_expr(e),
                Stmt::Return(None) => {}
            }
        }
        if let Some(v) = &mut b.value {
            self.finalize_expr(v);
        }
    }

    fn finalize_expr(&self, e: &mut Expr) {
        e.ty = self.finalize(e.ty);
        match &mut e.kind {
            ExprKind::Unary { expr, .. } => self.finalize_expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.finalize_expr(lhs);
                self.finalize_expr(rhs);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.finalize_expr(a);
                }
            }
            ExprKind::If { cond, then, els } => {
                self.finalize_expr(cond);
                self.finalize_block(then);
                self.finalize_block(els);
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.finalize_expr(f);
                }
            }
            ExprKind::Block(b) => self.finalize_block(b),
            ExprKind::OptionSome(inner) | ExprKind::ResultOk(inner) | ExprKind::ResultErr(inner)
            | ExprKind::Try(inner) => self.finalize_expr(inner),
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.finalize_expr(opt);
                self.finalize_expr(fallback);
            }
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::Field { .. } => {}
        }
    }
}

/// Whether a braced `else { … }` fallback diverges (its last statement is `return`),
/// in which case it produces no value and need not match the payload type.
fn block_diverges(e: &ast::Expr) -> bool {
    match &e.kind {
        ast::ExprKind::Block(b) => b.tail.is_none() && matches!(b.stmts.last(), Some(ast::Stmt::Return(_))),
        _ => false,
    }
}

fn single_name(p: &ast::Path) -> Option<&str> {
    if p.segments.len() == 1 {
        Some(p.segments[0].name.as_str())
    } else {
        None
    }
}

fn ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(it) => it.name(),
        Ty::IntVar(_) => "int(undetermined)".to_string(),
        Ty::Float(ft) => ft.name(),
        Ty::FloatVar(_) => "float(undetermined)".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Option(s) => format!("Option<{}>", scalar_name(s)),
        Ty::Result(o, e) => format!("Result<{}, {}>", scalar_name(o), scalar_name(e)),
        Ty::ErrCode => "Error".to_string(),
        Ty::Struct(id) => format!("struct#{id}"),
        Ty::Unit => "()".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

/// A composite type argument must resolve to a concrete scalar in M2.
fn scalar_arg(ty: Ty, what: &str, span: Span, diags: &mut Diagnostics) -> Option<Scalar> {
    match ty_to_scalar(ty) {
        Some(s) => Some(s),
        None => {
            if ty != Ty::Error {
                diags.error(format!("{what} must be a scalar in M2, got {}", ty_name(ty)), span);
            }
            None
        }
    }
}

fn resolve_type(t: &ast::Type, struct_ids: &HashMap<String, u32>, diags: &mut Diagnostics) -> Ty {
    let name = t
        .path
        .segments
        .last()
        .map(|s| s.name.as_str())
        .unwrap_or("");
    match name {
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "f32" => Ty::Float(FloatTy { bits: 32 }),
        "f64" => Ty::Float(FloatTy { bits: 64 }),
        "()" => Ty::Unit,
        "Error" => Ty::ErrCode,
        "Option" => {
            let inner = match t.args.as_slice() {
                [a] => resolve_type(a, struct_ids, diags),
                _ => {
                    diags.error("Option takes exactly one type argument".to_string(), t.span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "Option payload", t.span, diags) {
                Some(s) => Ty::Option(s),
                None => Ty::Error,
            }
        }
        "Result" => {
            let (ok, err) = match t.args.as_slice() {
                [a, b] => (
                    resolve_type(a, struct_ids, diags),
                    resolve_type(b, struct_ids, diags),
                ),
                _ => {
                    diags.error("Result takes two type arguments".to_string(), t.span);
                    return Ty::Error;
                }
            };
            match (
                scalar_arg(ok, "Result ok payload", t.span, diags),
                scalar_arg(err, "Result err payload", t.span, diags),
            ) {
                (Some(o), Some(e)) => Ty::Result(o, e),
                _ => Ty::Error,
            }
        }
        _ => match parse_int_name(name) {
            Some(it) => Ty::Int(it),
            None => match struct_ids.get(name) {
                Some(&id) => Ty::Struct(id),
                None => {
                    diags.error(format!("unsupported type in M1: '{name}'"), t.span);
                    Ty::Error
                }
            },
        },
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
    fn fib_checks() {
        let src = "fn fib(n: i64) -> i64 {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\n";
        let (_p, d) = check(src);
        assert!(!d.has_errors(), "fib should type-check");
    }

    #[test]
    fn bool_condition_required() {
        let (_p, d) = check("fn f(n: i32) -> i32 {\n  if n { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "if condition must be bool");
    }

    #[test]
    fn assign_to_immutable_errors() {
        let (_p, d) = check("fn f() -> i32 {\n  x := 1\n  x = 2\n  return x\n}\n");
        assert!(d.has_errors());
    }

    const POINT: &str = "Point {\n  x: i32,\n  y: i32,\n}\n";

    #[test]
    fn struct_construct_and_read_checks() {
        let src = format!(
            "{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1, y: 2 }}\n  return p.x + p.y\n}}\n"
        );
        let (_p, d) = check(&src);
        assert!(!d.has_errors(), "a well-formed struct program should check");
    }

    #[test]
    fn missing_field_errors() {
        let src = format!("{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1 }}\n  return p.x\n}}\n");
        let (_p, d) = check(&src);
        assert!(d.has_errors(), "omitting field y must error");
    }

    #[test]
    fn unknown_field_access_errors() {
        let src = format!("{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1, y: 2 }}\n  return p.z\n}}\n");
        let (_p, d) = check(&src);
        assert!(d.has_errors(), "reading field z must error");
    }

    #[test]
    fn float_program_checks() {
        let (_p, d) = check("fn f(r: f64) -> f64 {\n  return r * r\n}\n");
        assert!(!d.has_errors(), "float arithmetic should check");
    }

    #[test]
    fn no_implicit_int_float_mix() {
        // An integer literal must not silently satisfy a float context.
        let (_p, d) = check("fn f() -> f64 {\n  return 1\n}\n");
        assert!(d.has_errors(), "returning int where f64 is expected must error");
    }

    #[test]
    fn char_is_not_arithmetic() {
        let (_p, d) = check("fn f() -> char {\n  return 'a' + 'b'\n}\n");
        assert!(d.has_errors(), "char does not support arithmetic");
    }

    #[test]
    fn option_program_checks() {
        let (_p, d) = check(
            "fn choose(b: bool) -> Option<i32> {\n  if b { return Some(1) }\n  return None\n}\nfn main() -> i32 {\n  return choose(true) else 0\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed Option program should check");
    }

    #[test]
    fn else_unwrap_requires_option() {
        // `else`-unwrap on a non-Option is an error.
        let (_p, d) = check("fn f() -> i32 {\n  return 1 else 0\n}\n");
        assert!(d.has_errors(), "else-unwrap on a plain int must error");
    }

    #[test]
    fn bare_none_without_context_errors() {
        let (_p, d) = check("fn f() -> i32 {\n  x := None\n  return 0\n}\n");
        assert!(d.has_errors(), "None with no inferable Option type must error");
    }

    #[test]
    fn result_program_checks() {
        let (_p, d) = check(
            "fn g(n: i32) -> Result<i32, Error> {\n  if n < 0 { return Err(error(1)) }\n  return Ok(n)\n}\nfn f() -> Result<i32, Error> {\n  x := g(2)?\n  return Ok(x)\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed Result program should check");
    }

    #[test]
    fn question_requires_result_returning_fn() {
        // `?` in a function that doesn't return Result is an error.
        let (_p, d) = check(
            "fn g() -> Result<i32, Error> {\n  return Ok(1)\n}\nfn f() -> i32 {\n  x := g()?\n  return x\n}\n",
        );
        assert!(d.has_errors(), "`?` in a non-Result function must error");
    }

    #[test]
    fn question_on_non_result_errors() {
        let (_p, d) = check("fn f() -> Result<i32, Error> {\n  x := 1?\n  return Ok(x)\n}\n");
        assert!(d.has_errors(), "`?` on a plain int must error");
    }

    #[test]
    fn field_assign_requires_mut() {
        let src = format!(
            "{POINT}fn main() -> i32 {{\n  p := Point {{ x: 1, y: 2 }}\n  p.x = 5\n  return p.x\n}}\n"
        );
        let (_p, d) = check(&src);
        assert!(d.has_errors(), "assigning a field of an immutable struct must error");
    }
}
