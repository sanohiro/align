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
    /// `box<T>` — an owning heap pointer to a scalar (a Move type). M3.
    Box(Scalar),
    /// `array<T>` of a fixed length — contiguous scalars. M4 (length known from the
    /// literal; dynamic-length arrays/slices come later).
    Array(Scalar, u32),
    /// A fixed-length array of structs (AoS); `(struct_id, length)`. M4.
    StructArray(u32, u32),
    /// An arena handle (internal; produced by `arena {}`, never written by the user).
    ArenaHandle,
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

/// A pipeline stage as collected from the AST (before type checking).
enum RawStage {
    Map(ast::Ident),
    Where(ast::Ident),
    WhereField(ast::Ident),
    Project(ast::Ident),
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
                            "struct fields must be primitive types (nested structs are not supported yet)"
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
        let params: Vec<Ty> = f
            .params
            .iter()
            .map(|p| resolve_type(&p.ty, &struct_ids, diags))
            .collect();
        // A box across a call boundary would escape its arena, so M3 forbids box
        // parameters and returns (boxes are arena-local). This also closes escape
        // holes via call results.
        for (p, ty) in f.params.iter().zip(&params) {
            if matches!(ty, Ty::Box(_)) {
                diags.error(
                    "a box cannot be a function parameter (boxes are arena-local in M3)".to_string(),
                    p.ty.span,
                );
            }
        }
        let ret = match &f.ret {
            Some(t) => {
                let r = resolve_type(t, &struct_ids, diags);
                if matches!(r, Ty::Box(_)) {
                    diags.error(
                        "a box cannot be a function return type (it would escape its arena)".to_string(),
                        t.span,
                    );
                }
                r
            }
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
                arena_depth: 0,
            };
            Some(cx.check_fn(f))
        })
        .collect();
    let program = Program { fns, structs };
    // Pass 3 (partial): move / use-after-move checking + arena escape checking
    // (`03-types.md` §6–§7).
    for f in &program.fns {
        MoveCheck { f, diags }.check();
        EscapeCheck {
            f,
            diags,
            region: std::collections::HashMap::new(),
            decl_depth: std::collections::HashMap::new(),
        }
        .check();
    }
    program
}

/// Arena escape checking (`03-types.md` §7): a `box<T>` allocated in an arena must not
/// outlive its block. The "region" of a box is the arena depth at which it was
/// allocated; escaping to a shallower depth (return, assignment to an outer binding, or
/// the arena block's own value) is an error. Regions are inferred — never written.
struct EscapeCheck<'a> {
    f: &'a Fn,
    diags: &'a mut Diagnostics,
    /// For each box local, the arena depth at which its current box was allocated.
    region: std::collections::HashMap<LocalId, u32>,
    /// For each local, the arena depth at which it was declared.
    decl_depth: std::collections::HashMap<LocalId, u32>,
}

impl<'a> EscapeCheck<'a> {
    fn check(&mut self) {
        self.block(&self.f.body, 0);
    }

    fn is_box(ty: Ty) -> bool {
        matches!(ty, Ty::Box(_))
    }

    /// The arena depth of the box a (box-typed) expression yields; 0 = no arena region.
    /// Recurses through value-producing forms so a box can't slip out via an `if`/block
    /// value. (Calls cannot yield a box — box params/returns are forbidden.)
    fn region_of(&self, e: &Expr, depth: u32) -> u32 {
        match &e.kind {
            ExprKind::HeapNew(_) | ExprKind::BoxClone(_) => depth,
            ExprKind::Local(p) => *self.region.get(p).unwrap_or(&0),
            ExprKind::Block(b) => self.region_of_block(b, depth),
            ExprKind::If { then, els, .. } => {
                self.region_of_block(then, depth).max(self.region_of_block(els, depth))
            }
            _ => 0,
        }
    }

    fn region_of_block(&self, b: &Block, depth: u32) -> u32 {
        b.value.as_ref().map(|v| self.region_of(v, depth)).unwrap_or(0)
    }

    fn block(&mut self, b: &Block, depth: u32) {
        for s in &b.stmts {
            self.stmt(s, depth);
        }
        if let Some(v) = &b.value {
            self.walk(v, depth);
        }
    }

    fn stmt(&mut self, s: &Stmt, depth: u32) {
        match s {
            Stmt::Let { local, init } => {
                self.walk(init, depth);
                self.decl_depth.insert(*local, depth);
                if Self::is_box(init.ty) {
                    let r = self.region_of(init, depth);
                    self.region.insert(*local, r);
                }
            }
            Stmt::Assign { local, value } => {
                self.walk(value, depth);
                if Self::is_box(value.ty) {
                    let r = self.region_of(value, depth);
                    let d = *self.decl_depth.get(local).unwrap_or(&0);
                    if d < r {
                        self.diags.error(
                            "this value is bound to an arena block and cannot escape it".to_string(),
                            value.span,
                        );
                    }
                    // Track the reassigned binding's region for later uses.
                    self.region.insert(*local, r);
                }
            }
            Stmt::AssignField { value, .. } => self.walk(value, depth),
            Stmt::Return(Some(e)) => {
                self.walk(e, depth);
                if Self::is_box(e.ty) && self.region_of(e, depth) >= 1 {
                    self.diags.error(
                        "cannot return a value allocated in an arena (it is freed at block end)".to_string(),
                        e.span,
                    );
                }
            }
            Stmt::Return(None) => {}
            Stmt::Expr(e) => self.walk(e, depth),
        }
    }

    /// Recurse to find nested arenas and value positions that let a box escape.
    fn walk(&mut self, e: &Expr, depth: u32) {
        match &e.kind {
            ExprKind::Arena(b) => {
                let inner = depth + 1;
                self.block(b, inner);
                if let Some(v) = &b.value {
                    if Self::is_box(v.ty) && self.region_of(v, inner) >= inner {
                        self.diags.error(
                            "a value allocated in this arena cannot escape as the block's value".to_string(),
                            v.span,
                        );
                    }
                }
            }
            ExprKind::Block(b) => self.block(b, depth),
            ExprKind::If { cond, then, els } => {
                self.walk(cond, depth);
                self.block(then, depth);
                self.block(els, depth);
            }
            ExprKind::Unary { expr, .. } => self.walk(expr, depth),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.walk(lhs, depth);
                self.walk(rhs, depth);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.walk(a, depth);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.walk(f, depth);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) | ExprKind::BoxGet(i)
            | ExprKind::BoxClone(i) | ExprKind::ArraySum { source: i, .. } => self.walk(i, depth),
            ExprKind::ArrayLit { elems, .. } => {
                for e in elems {
                    self.walk(e, depth);
                }
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.walk(opt, depth);
                self.walk(fallback, depth);
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

/// Flow analysis that flags use-after-move. A Move-typed value (M3: `box<T>`) is
/// consumed when bound/assigned/passed/returned by value; using it afterwards is an
/// error. Borrowing positions (`.get()`/`.clone()` receiver, operands) do not consume.
struct MoveCheck<'a> {
    f: &'a Fn,
    diags: &'a mut Diagnostics,
}

impl<'a> MoveCheck<'a> {
    fn check(&mut self) {
        let mut moved = std::collections::HashSet::new();
        // A function never returns a box (forbidden), so the body value is not consumed.
        self.block(&self.f.body, &mut moved, false);
    }

    fn is_move(&self, id: LocalId) -> bool {
        matches!(self.f.locals.get(id as usize).map(|l| l.ty), Some(Ty::Box(_)))
    }

    /// `tail_consuming` = whether the block's trailing value is consumed by its context.
    fn block(&mut self, b: &Block, moved: &mut std::collections::HashSet<LocalId>, tail_consuming: bool) {
        for s in &b.stmts {
            match s {
                Stmt::Let { local, init } => {
                    self.expr(init, moved, true);
                    moved.remove(local);
                }
                Stmt::Assign { local, value } => {
                    self.expr(value, moved, true);
                    moved.remove(local);
                }
                Stmt::AssignField { value, .. } => self.expr(value, moved, true),
                Stmt::Return(Some(e)) => self.expr(e, moved, true),
                Stmt::Return(None) => {}
                Stmt::Expr(e) => self.expr(e, moved, false),
            }
        }
        if let Some(v) = &b.value {
            self.expr(v, moved, tail_consuming);
        }
    }

    /// `consuming` = this position takes a Move value by value (so it moves it).
    fn expr(&mut self, e: &Expr, moved: &mut std::collections::HashSet<LocalId>, consuming: bool) {
        match &e.kind {
            ExprKind::Local(id) => {
                if moved.contains(id) {
                    let name = &self.f.locals[*id as usize].name;
                    self.diags.error(format!("use of moved value '{name}'"), e.span);
                } else if consuming && self.is_move(*id) {
                    moved.insert(*id);
                }
            }
            ExprKind::Field { base, .. } => {
                if moved.contains(base) {
                    let name = &self.f.locals[*base as usize].name;
                    self.diags.error(format!("use of moved value '{name}'"), e.span);
                }
            }
            ExprKind::Unary { expr, .. } => self.expr(expr, moved, false),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs, moved, false);
                self.expr(rhs, moved, false);
            }
            // Value arguments / wrapped payloads are consumed.
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.expr(a, moved, true);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.expr(f, moved, true);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) => self.expr(i, moved, true),
            // The receiver is borrowed, not consumed.
            ExprKind::BoxGet(i) | ExprKind::BoxClone(i) | ExprKind::ArraySum { source: i, .. } => {
                self.expr(i, moved, false)
            }
            ExprKind::ArrayLit { elems, .. } => {
                for e in elems {
                    self.expr(e, moved, true);
                }
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.expr(opt, moved, true);
                self.expr(fallback, moved, false);
            }
            ExprKind::Block(b) | ExprKind::Arena(b) => self.block(b, moved, consuming),
            ExprKind::If { cond, then, els } => {
                self.expr(cond, moved, false);
                let mut m1 = moved.clone();
                self.block(then, &mut m1, consuming);
                let mut m2 = moved.clone();
                self.block(els, &mut m2, consuming);
                // Conservative join: moved if moved on either path.
                *moved = &m1 | &m2;
            }
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::OptionNone => {}
        }
    }
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
    /// Nesting depth of `arena {}` blocks (0 = not in an arena).
    arena_depth: u32,
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
        // M2 `main` takes no arguments; `main(args: array<str>)` (draft.md §17) is future.
        if f.name.name == "main" && !f.params.is_empty() {
            self.diags
                .error("main takes no arguments (argv support comes later)".to_string(), f.span);
        }
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
        // `local.field = v`
        if let ast::ExprKind::FieldAccess { recv, field } = &place.kind {
            let Some((id, local_ty)) = self.place_local(recv) else {
                self.diags.error("invalid assignment target", place.span);
                return Place::Err;
            };
            if !self.locals[id as usize].is_mut {
                let name = self.locals[id as usize].name.clone();
                self.diags.error(
                    format!("cannot assign to a field of immutable '{name}' (declare with `mut`)"),
                    place.span,
                );
            }
            return match self.field_of(local_ty, &field.name, place.span) {
                Some((index, ty)) => Place::Field { base: id, index, ty },
                None => Place::Err,
            };
        }
        // `local = v`
        let Some((id, local_ty)) = self.place_local(place) else {
            self.diags.error("invalid assignment target", place.span);
            return Place::Err;
        };
        if !self.locals[id as usize].is_mut {
            let name = self.locals[id as usize].name.clone();
            self.diags
                .error(format!("cannot assign to immutable '{name}' (declare with `mut`)"), place.span);
        }
        if matches!(local_ty, Ty::Struct(_)) {
            self.diags.error(
                "cannot assign a whole struct; assign individual fields".to_string(),
                place.span,
            );
            return Place::Err;
        }
        Place::Local { id, ty: local_ty }
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
            ast::ExprKind::FieldAccess { recv, field } => {
                self.check_field_access(recv, field, expected, e.span)
            }
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, None, e.span),
            ast::ExprKind::FieldShorthand(_) => {
                self.diags.error(
                    "`.field` is only valid as a pipeline stage argument (e.g. `where(.active)`)".to_string(),
                    e.span,
                );
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span }
            }
            ast::ExprKind::ElseUnwrap { opt, fallback } => {
                self.check_else_unwrap(opt, fallback, expected, e.span)
            }
            ast::ExprKind::Try(inner) => self.check_try(inner, e.span),
            ast::ExprKind::Arena(b) => {
                let diverges = ast_block_diverges(b);
                self.arena_depth += 1;
                let block = self.check_block(b, if diverges { None } else { expected });
                self.arena_depth -= 1;
                let ty = if diverges {
                    expected.unwrap_or(Ty::Unit)
                } else {
                    let t = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                    self.constrain(t, expected, e.span);
                    t
                };
                Expr { kind: ExprKind::Arena(block), ty, span: e.span }
            }
            ast::ExprKind::StructLit { .. } => {
                self.diags.error(
                    "struct literals are only allowed as `name := Type { ... }`".to_string(),
                    e.span,
                );
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span }
            }
            ast::ExprKind::If { cond, then, els } => self.check_if(cond, then, els.as_deref(), expected, e.span),
            ast::ExprKind::Block(b) => {
                // A block that always returns never yields a value; let it take the
                // expected type so it fits any value position.
                if ast_block_diverges(b) {
                    let block = self.check_block(b, None);
                    let ty = expected.unwrap_or(Ty::Unit);
                    return Expr { kind: ExprKind::Block(block), ty, span: e.span };
                }
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
        if matches!(local_ty, Ty::Struct(_)) {
            self.diags.error(
                "cannot use a struct value directly (access its fields)".to_string(),
                span,
            );
            return err(span);
        }
        self.constrain(local_ty, expected, span);
        Expr { kind: ExprKind::Local(id), ty: local_ty, span }
    }

    /// `recv.field` (not a method call) — a struct field read. M4: the receiver must be
    /// a local (chained field access on a value comes later).
    fn check_field_access(&mut self, recv: &ast::Expr, field: &ast::Ident, expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Local(u32::MAX), ty: Ty::Error, span };
        let base = match self.place_local(recv) {
            Some((id, _)) => id,
            None => {
                self.diags
                    .error("field access is only supported on a local binding".to_string(), span);
                return err;
            }
        };
        let base_ty = self.locals[base as usize].ty;
        match self.field_of(base_ty, &field.name, span) {
            Some((index, ty)) => {
                self.constrain(ty, expected, span);
                Expr { kind: ExprKind::Field { base, index }, ty, span }
            }
            None => err,
        }
    }

    /// If `e` is a bare local name, return its id and type.
    fn place_local(&self, e: &ast::Expr) -> Option<(LocalId, Ty)> {
        if let ast::ExprKind::Path(p) = &e.kind {
            if let Some(name) = single_name(p) {
                if let Some(id) = self.lookup(name) {
                    return Some((id, self.locals[id as usize].ty));
                }
            }
        }
        None
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
        // Method call `recv.method(...)`: a module builtin (`heap.new`) or a method on a
        // value (`box.get()`, `box.clone()`).
        if let ast::ExprKind::FieldAccess { recv, field } = &callee.kind {
            return self.check_method_call(recv, &field.name, args, expected, span);
        }
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
                        .error("'print' expects an integer".to_string(), e.span);
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
                        .error(format!("Option payload must be a scalar (composite payloads are not supported yet), got {}", ty_name(f)), span);
                }
                Scalar::Int(IntTy { bits: 64, signed: true })
            }
        }
    }

    /// A method call `recv.method(args)`: the `heap.new` builtin, or a method on a value
    /// (`box.get()`, `box.clone()`).
    fn check_method_call(&mut self, recv: &ast::Expr, method: &str, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // `heap.new(...)` — `heap` is a module name, not a value.
        if let ast::ExprKind::Path(p) = &recv.kind {
            if single_name(p) == Some("heap") && method == "new" {
                return self.check_heap_new(args, expected, span);
            }
        }
        // `sum` is the terminal of a fused pipeline `src.map(f).where(p)….sum()`.
        if method == "sum" {
            return self.check_array_sum(recv, args, expected, span);
        }
        // `map`/`where` are only valid as pipeline stages under a terminal reduction.
        if method == "map" || method == "where" {
            self.diags.error(
                format!("'.{method}()' must be part of a pipeline ending in a reduction like `.sum()`"),
                span,
            );
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let recv_expr = self.check_expr(recv, None);
        let recv_ty = recv_expr.ty;
        match method {
            "get" => self.check_box_get(recv_expr, recv_ty, args, span),
            "clone" => self.check_box_clone(recv_expr, recv_ty, args, span),
            _ => {
                if recv_ty != Ty::Error {
                    self.diags
                        .error(format!("unknown method '.{method}()' on {}", ty_name(recv_ty)), span);
                }
                err
            }
        }
    }

    /// `[e1, e2, ...]` — a fixed-length array literal. Elements share one scalar type
    /// (resolved here; an unconstrained literal defaults). Empty literals need a type
    /// annotation, which is not supported yet.
    fn check_array_lit(&mut self, elems: &[ast::Expr], elem_expected: Option<Ty>, span: Span) -> Expr {
        if elems.is_empty() {
            self.diags
                .error("an empty array literal needs a type annotation (not supported yet)".to_string(), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let n = elems.len() as u32;
        // An array of struct literals → a struct array (AoS).
        if let ast::ExprKind::StructLit { .. } = &elems[0].kind {
            let mut checked = Vec::new();
            let mut sid = None;
            for e in elems {
                let ast::ExprKind::StructLit { name, fields } = &e.kind else {
                    self.diags.error("array elements must all be struct literals here".to_string(), e.span);
                    continue;
                };
                let lit = self.check_struct_lit(name, fields, e.span);
                if let Ty::Struct(id) = lit.ty {
                    match sid {
                        None => sid = Some(id),
                        Some(prev) if prev != id => {
                            self.diags.error("array elements must be the same struct type".to_string(), e.span);
                        }
                        _ => {}
                    }
                }
                checked.push(lit);
            }
            return match sid {
                Some(id) => Expr {
                    kind: ExprKind::ArrayLit { elems: checked, elem: Ty::Struct(id) },
                    ty: Ty::StructArray(id, n),
                    span,
                },
                None => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            };
        }
        // Otherwise a scalar array.
        let first = self.check_expr(&elems[0], elem_expected);
        let elem_ty = first.ty;
        let mut checked = vec![first];
        for e in &elems[1..] {
            checked.push(self.check_expr(e, Some(elem_ty)));
        }
        let scalar = self.payload_scalar(elem_ty, span);
        Expr { kind: ExprKind::ArrayLit { elems: checked, elem: scalar_to_ty(scalar) }, ty: Ty::Array(scalar, n), span }
    }

    /// Collect a pipeline `src.map(f).where(p)…` from the AST: the innermost receiver is
    /// the source array; `.map`/`.where` calls become ordered stages (source-first).
    /// Check a `map`/`where` stage function against the current element type, returning
    /// its return type. `is_pred` requires a `bool` result.
    fn check_stage_fn(&mut self, fname: &ast::Ident, elem: Ty, is_pred: bool) -> Ty {
        let Some(sig) = self.sigs.get(&fname.name) else {
            self.diags.error(format!("undefined function: '{}'", fname.name), fname.span);
            return Ty::Error;
        };
        let (params, ret) = (sig.params.clone(), sig.ret);
        if params.len() != 1 || params[0] != elem {
            self.diags.error(
                format!("'{}' must take one {} argument here", fname.name, ty_name(elem)),
                fname.span,
            );
        }
        if is_pred && ret != Ty::Bool {
            self.diags
                .error(format!("'where' predicate '{}' must return bool", fname.name), fname.span);
        }
        ret
    }

    fn collect_pipeline<'e>(&mut self, e: &'e ast::Expr) -> (&'e ast::Expr, Vec<RawStage>) {
        match &e.kind {
            // `.map(f)` / `.where(p)`
            ast::ExprKind::Call { callee, args } => {
                if let ast::ExprKind::FieldAccess { recv, field } = &callee.kind {
                    let is_map = field.name == "map";
                    let is_where = field.name == "where";
                    if is_map || is_where {
                        let arg = if args.len() == 1 { Some(&args[0]) } else { None };
                        let (src, mut stages) = self.collect_pipeline(recv);
                        // `where(.field)` — a field predicate.
                        if is_where {
                            if let Some(ast::Expr { kind: ast::ExprKind::FieldShorthand(f), .. }) = arg {
                                stages.push(RawStage::WhereField(f.clone()));
                                return (src, stages);
                            }
                        }
                        match arg.and_then(|a| self.pipeline_fn_name(a)) {
                            Some(f) if is_map => stages.push(RawStage::Map(f)),
                            Some(f) => stages.push(RawStage::Where(f)),
                            None => self.diags.error(
                                format!("'.{}()' needs a single named function or `.field`", field.name),
                                e.span,
                            ),
                        }
                        return (src, stages);
                    }
                }
                (e, Vec::new())
            }
            // `.field` projection on an array.
            ast::ExprKind::FieldAccess { recv, field } => {
                let (src, mut stages) = self.collect_pipeline(recv);
                stages.push(RawStage::Project(field.clone()));
                (src, stages)
            }
            _ => (e, Vec::new()),
        }
    }

    fn pipeline_fn_name(&self, a: &ast::Expr) -> Option<ast::Ident> {
        if let ast::ExprKind::Path(p) = &a.kind {
            if p.segments.len() == 1 {
                return Some(p.segments[0].clone());
            }
        }
        None
    }

    /// `src.map(f).where(p).field….sum()` — a fused reduction. Threads the element type
    /// through each stage (a struct array is projected to a scalar) and folds the final
    /// numeric element type with `+`.
    fn check_array_sum(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'sum' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let (source_ast, raw_stages) = self.collect_pipeline(recv);
        // Choose the expected element type for an inline scalar literal source: the
        // first Map stage's parameter, or (with no stages) the sum's result type.
        let elem_expected = match raw_stages.first() {
            Some(RawStage::Map(fname)) => self.sigs.get(&fname.name).and_then(|s| s.params.first().copied()),
            None => expected,
            _ => None,
        };
        let source = match &source_ast.kind {
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, elem_expected, span),
            _ => self.check_expr(source_ast, None),
        };
        let mut elem = match source.ty {
            Ty::Array(s, _) => scalar_to_ty(s),
            Ty::StructArray(id, _) => Ty::Struct(id),
            Ty::Error => return err,
            other => {
                self.diags
                    .error(format!("a pipeline source must be an array, got {}", ty_name(other)), span);
                return err;
            }
        };

        let mut stages = Vec::new();
        for raw in raw_stages {
            match raw {
                RawStage::Project(field) => {
                    if !matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'.{}' projection needs a struct element, got {}", field.name, ty_name(elem)),
                            field.span,
                        );
                        return err;
                    }
                    match self.field_of(elem, &field.name, field.span) {
                        Some((index, ty)) => {
                            stages.push(Stage { kind: StageKind::Project { field: index }, out_ty: ty });
                            elem = ty;
                        }
                        None => return err,
                    }
                }
                RawStage::Map(fname) => {
                    let ret = self.check_stage_fn(&fname, elem, false);
                    stages.push(Stage { kind: StageKind::Map { func: fname.name }, out_ty: ret });
                    elem = ret;
                }
                RawStage::Where(fname) => {
                    self.check_stage_fn(&fname, elem, true);
                    stages.push(Stage { kind: StageKind::Where { func: fname.name }, out_ty: elem });
                }
                RawStage::WhereField(field) => {
                    if !matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'where(.{})' needs a struct element, got {}", field.name, ty_name(elem)),
                            field.span,
                        );
                        return err;
                    }
                    match self.field_of(elem, &field.name, field.span) {
                        Some((index, fty)) => {
                            if fty != Ty::Bool {
                                self.diags.error(
                                    format!("'where(.{})' field must be bool, got {}", field.name, ty_name(fty)),
                                    field.span,
                                );
                            }
                            stages.push(Stage { kind: StageKind::WhereField { field: index }, out_ty: elem });
                        }
                        None => return err,
                    }
                }
            }
        }

        if !elem.is_numeric() {
            self.diags
                .error(format!("'sum' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArraySum { source: Box::new(source), stages }, ty: elem, span }
    }

    /// `b.clone()` — deep-copy a `box<T>`. Allocates a fresh box, so it needs an arena.
    fn check_box_clone(&mut self, recv: Expr, recv_ty: Ty, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'clone' takes no arguments".to_string(), span);
        }
        match recv_ty {
            Ty::Box(s) => {
                if self.arena_depth == 0 {
                    self.diags
                        .error("clone allocates; it must be used inside an `arena {}` block".to_string(), span);
                }
                Expr { kind: ExprKind::BoxClone(Box::new(recv)), ty: Ty::Box(s), span }
            }
            Ty::Error => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.clone()' is only available on box<T> in M3, got {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
    }

    /// `heap.new(x)` — allocate `box<T>` in the enclosing arena. M3 requires an arena.
    fn check_heap_new(&mut self, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if self.arena_depth == 0 {
            self.diags
                .error("heap.new must be used inside an `arena {}` block".to_string(), span);
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'heap.new' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let inner_expected = match expected {
            Some(Ty::Box(s)) => Some(scalar_to_ty(s)),
            _ => None,
        };
        let arg = self.check_expr(&args[0], inner_expected);
        let scalar = self.payload_scalar(arg.ty, args[0].span);
        Expr { kind: ExprKind::HeapNew(Box::new(arg)), ty: Ty::Box(scalar), span }
    }

    /// `b.get()` — copy the value out of a `box<T>`.
    fn check_box_get(&mut self, recv: Expr, recv_ty: Ty, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'get' takes no arguments".to_string(), span);
        }
        match recv_ty {
            Ty::Box(s) => Expr { kind: ExprKind::BoxGet(Box::new(recv)), ty: scalar_to_ty(s), span },
            Ty::Error => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.get()' is only available on box<T>, got {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
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
            ExprKind::Block(b) | ExprKind::Arena(b) => self.finalize_block(b),
            ExprKind::OptionSome(inner) | ExprKind::ResultOk(inner) | ExprKind::ResultErr(inner)
            | ExprKind::Try(inner) | ExprKind::HeapNew(inner) | ExprKind::BoxGet(inner)
            | ExprKind::BoxClone(inner) | ExprKind::ArraySum { source: inner, .. } => {
                self.finalize_expr(inner)
            }
            ExprKind::ArrayLit { elems, .. } => {
                for e in elems {
                    self.finalize_expr(e);
                }
            }
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

/// Whether a block always diverges (no tail value and its last statement is `return`),
/// so it never yields a value and need not match an expected value type.
fn ast_block_diverges(b: &ast::Block) -> bool {
    b.tail.is_none() && matches!(b.stmts.last(), Some(ast::Stmt::Return(_)))
}

/// Whether a braced `else { … }` fallback diverges (its last statement is `return`),
/// in which case it produces no value and need not match the payload type.
fn block_diverges(e: &ast::Expr) -> bool {
    match &e.kind {
        ast::ExprKind::Block(b) => ast_block_diverges(b),
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
        Ty::Box(s) => format!("box<{}>", scalar_name(s)),
        Ty::Array(s, n) => format!("array<{}>[{n}]", scalar_name(s)),
        Ty::StructArray(id, n) => format!("array<struct#{id}>[{n}]"),
        Ty::ArenaHandle => "arena".to_string(),
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
                diags.error(format!("{what} must be a scalar (composite payloads are not supported yet), got {}", ty_name(ty)), span);
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
        "box" => {
            let inner = match t.args.as_slice() {
                [a] => resolve_type(a, struct_ids, diags),
                _ => {
                    diags.error("box takes exactly one type argument".to_string(), t.span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "box payload", t.span, diags) {
                Some(s) => Ty::Box(s),
                None => Ty::Error,
            }
        }
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
                    diags.error(format!("unknown type: '{name}'"), t.span);
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
    fn arena_box_program_checks() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  r: i32 := arena {\n    p: box<i32> := heap.new(5)\n    p.get()\n  }\n  return r\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed arena/box program should check");
    }

    #[test]
    fn array_sum_checks() {
        let (_p, d) = check("fn main() -> i32 {\n  return [10, 20, 12].sum()\n}\n");
        assert!(!d.has_errors(), "a well-formed array sum should check");
    }

    #[test]
    fn fused_pipeline_checks() {
        let (_p, d) = check(
            "fn dbl(x: i32) -> i32 = x * 2\nfn big(x: i32) -> bool = x > 4\nfn main() -> i32 {\n  return [1, 2, 3].map(dbl).where(big).sum()\n}\n",
        );
        assert!(!d.has_errors(), "a well-formed map/where/sum pipeline should check");
    }

    #[test]
    fn struct_array_projection_checks() {
        let (_p, d) = check(
            "Pt { x: i32, y: i32 }\nfn main() -> i32 {\n  return [Pt{x: 1, y: 2}, Pt{x: 3, y: 4}].x.sum()\n}\n",
        );
        assert!(!d.has_errors(), "struct array projection + sum should check");
    }

    #[test]
    fn where_field_predicate_checks() {
        let (_p, d) = check(
            "Emp { pay: i32, active: bool }\nfn main() -> i32 {\n  return [Emp{pay: 1, active: true}].where(.active).pay.sum()\n}\n",
        );
        assert!(!d.has_errors(), "where(.field) + projection should check");
    }

    #[test]
    fn where_field_must_be_bool() {
        let (_p, d) = check(
            "Pt { x: i32, y: i32 }\nfn main() -> i32 {\n  return [Pt{x: 1, y: 2}].where(.x).x.sum()\n}\n",
        );
        assert!(d.has_errors(), "where(.field) on a non-bool field must error");
    }

    #[test]
    fn where_predicate_must_return_bool() {
        let (_p, d) = check(
            "fn dbl(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  return [1, 2, 3].where(dbl).sum()\n}\n",
        );
        assert!(d.has_errors(), "a where predicate returning non-bool must error");
    }

    #[test]
    fn map_without_terminal_errors() {
        let (_p, d) = check(
            "fn dbl(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  xs := [1, 2, 3].map(dbl)\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "map without a terminal reduction must error in M4");
    }

    #[test]
    fn empty_array_literal_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  return [].sum()\n}\n");
        assert!(d.has_errors(), "an empty array literal needs a type");
    }

    #[test]
    fn sum_on_non_array_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 1\n  return x.sum()\n}\n");
        assert!(d.has_errors(), "`.sum()` on a non-array must error");
    }

    #[test]
    fn use_after_move_errors() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(7)\n    q: box<i32> := p\n    return p.get()\n  }\n}\n",
        );
        assert!(d.has_errors(), "using a box after it is moved must error");
    }

    #[test]
    fn clone_does_not_move() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(7)\n    q: box<i32> := p.clone()\n    p.get() + q.get()\n  }\n}\n",
        );
        assert!(!d.has_errors(), "clone borrows; the original stays usable");
    }

    #[test]
    fn arena_box_value_escape_errors() {
        // Yielding a freshly-allocated box as the arena's value escapes the arena.
        let (_p, d) = check("fn main() -> i32 {\n  b := arena {\n    heap.new(7)\n  }\n  return 0\n}\n");
        assert!(d.has_errors(), "a box must not escape as the arena block's value");
    }

    #[test]
    fn return_box_escape_errors() {
        let (_p, d) = check(
            "fn make() -> box<i32> {\n  arena {\n    p: box<i32> := heap.new(7)\n    return p\n  }\n}\n",
        );
        assert!(d.has_errors(), "returning an arena box must error");
    }

    #[test]
    fn assign_box_to_outer_binding_escapes() {
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    mut saved: box<i32> := heap.new(0)\n    arena {\n      p: box<i32> := heap.new(7)\n      saved = p\n    }\n    saved.get()\n  }\n}\n",
        );
        assert!(d.has_errors(), "binding an inner-arena box to an outer binding must error");
    }

    #[test]
    fn box_escape_via_if_branches_errors() {
        // A box reaching the arena value through `if` branches must still be caught.
        let (_p, d) = check(
            "fn main() -> i32 {\n  b := arena {\n    if true { heap.new(1) } else { heap.new(2) }\n  }\n  return 0\n}\n",
        );
        assert!(d.has_errors(), "a box escaping via if-branch values must error");
    }

    #[test]
    fn box_parameter_and_return_forbidden() {
        let (_p, d) = check("fn id(b: box<i32>) -> box<i32> {\n  return b\n}\nfn main() -> i32 {\n  return 0\n}\n");
        assert!(d.has_errors(), "box params/returns are forbidden in M3");
    }

    #[test]
    fn move_through_block_value_is_tracked() {
        // The block's tail value consumes p, so reusing p afterwards is a move error.
        let (_p, d) = check(
            "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(1)\n    q: box<i32> := {\n      p\n    }\n    return p.get()\n  }\n}\n",
        );
        assert!(d.has_errors(), "a box moved through a block value must be tracked");
    }

    #[test]
    fn heap_new_outside_arena_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  p: box<i32> := heap.new(5)\n  return p.get()\n}\n");
        assert!(d.has_errors(), "heap.new outside an arena must error");
    }

    #[test]
    fn get_on_non_box_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 1\n  return x.get()\n}\n");
        assert!(d.has_errors(), "`.get()` on a non-box must error");
    }

    #[test]
    fn main_with_arguments_errors() {
        let (_p, d) = check("fn main(n: i32) -> i32 {\n  return n\n}\n");
        assert!(d.has_errors(), "main with arguments must error in M2");
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
