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
    /// A struct payload (the struct's id). Lets `Option`/`Result` carry a whole struct
    /// (e.g. `Result<User, Error>` from `json.decode`). No recursion — just the id.
    Struct(u32),
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
    /// `slice<T>` — a borrowed view `{ T* ptr, i64 len }` of scalar elements. Copy. M4.
    Slice(Scalar),
    /// `array<T>` — an *owned*, dynamic-length array of scalars, laid out like a slice
    /// (`{ T* ptr, i64 len }`) but Move and region-tracked. MMv2 slice 3: produced by a
    /// materializing terminal (`.to_array()`) and (this slice) arena-bump-allocated.
    DynArray(Scalar),
    /// `str` — an immutable string view `{ u8* ptr, i64 len }`. Copy. M5.
    Str,
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
pub fn ty_to_scalar(ty: Ty) -> Option<Scalar> {
    match ty {
        Ty::Int(it) => Some(Scalar::Int(it)),
        Ty::Float(ft) => Some(Scalar::Float(ft)),
        Ty::Bool => Some(Scalar::Bool),
        Ty::Char => Some(Scalar::Char),
        Ty::Unit => Some(Scalar::Unit),
        Ty::Struct(id) => Some(Scalar::Struct(id)),
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
        Scalar::Struct(id) => Ty::Struct(id),
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
                    // Fields are scalars or `str`. A `str` field may now hold an arena-backed
                    // str: the struct *carries* that field's region (MMv2 slice 2), so
                    // `EscapeCheck` lets it live inside the arena and only rejects the whole
                    // struct escaping. A scalar/literal-only struct stays `Static` (returnable).
                    // Other composite/region-bearing fields (box/slice/array/option/result/
                    // nested struct) the layout can't hold yet remain rejected. NOTE:
                    // `ty_to_scalar` now also accepts `Ty::Struct` (a valid Option/Result
                    // payload), so a nested struct field is rejected explicitly here.
                    let is_field_ok =
                        (ty_to_scalar(ty).is_some() && !matches!(ty, Ty::Struct(_))) || ty == Ty::Str || ty == Ty::Error;
                    if !is_field_ok {
                        diags.error(
                            format!("struct fields must be a primitive scalar or str for now, got {}", ty_name(ty)),
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
                int_parent: Vec::new(),
                float_vars: Vec::new(),
                float_parent: Vec::new(),
                locals: Vec::new(),
                scope: Vec::new(),
                ret_hint: Ty::Unit,
                arena_depth: 0,
            };
            Some(cx.check_fn(f))
        })
        .collect();
    let mut program = Program { fns, structs };
    // Pass 3 (partial): move / use-after-move checking + arena escape checking
    // (`03-types.md` §6–§7), then derive the per-function drop set (MMv2 slice 4).
    for f in &mut program.fns {
        let ever_moved = MoveCheck { f, diags, ever_moved: std::collections::HashSet::new() }.check();
        let region = {
            let mut ec = EscapeCheck {
                f,
                diags,
                region: std::collections::HashMap::new(),
                decl_depth: std::collections::HashMap::new(),
                local_backed_slice: std::collections::HashSet::new(),
            };
            ec.check();
            ec.region
        };
        // A free-standing owned `array<T>` (region `Static`) that is never moved out must be
        // dropped at every function exit. Arena-allocated ones (region `Arena(k)`) are
        // bulk-freed, and moved-out ones transfer ownership, so both are excluded.
        //
        // KNOWN LIMITATION (deferred to a "complete drop coverage" slice): a local moved on
        // *some* but not all paths is excluded outright, so it leaks on the path where it is
        // not moved. The robust fix is null-on-move drop flags (keep it in `drop_locals`, null
        // its slot at each move site so the exit `Drop` is a no-op `free(null)` when moved).
        // The leak is sound (no double-free / UAF) and bounded (no loops in the language yet).
        let drops: Vec<LocalId> = f
            .locals
            .iter()
            .filter(|l| matches!(l.ty, Ty::DynArray(_)))
            .map(|l| l.id)
            .filter(|id| !ever_moved.contains(id))
            .filter(|id| region.get(id).copied().unwrap_or(Region::Static) == Region::Static)
            .collect();
        f.drop_locals = drops;
    }
    program
}

/// A value's inferred lifetime region (Memory Model v2, `impl/08-memory-model-v2.md`).
/// Total order, longest-lived first: `Static ⊐ Frame ⊐ Arena(1) ⊐ … ⊐ Arena(d)`. Regions are
/// inferred, never written, and live only in this analysis — they are not part of `Ty`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    /// Process / program lifetime: literals, leaked allocations, owned-from-scalar values.
    Static,
    /// The current function's frame: a view created in-frame over frame-local storage. Cannot
    /// be returned. (A view *parameter* borrows the caller and is `Static` here — returnable.)
    /// Not yet produced — frame-local slices still use the `local_backed_slice` set; folding
    /// them onto this variant is a later MMv2 slice.
    #[allow(dead_code)]
    Frame,
    /// The k-th enclosing `arena {}` (1 = outermost). Freed at that block's end.
    Arena(u32),
}

impl Region {
    /// Ordinal in the lattice; smaller = longer-lived.
    fn ord(self) -> u32 {
        match self {
            Region::Static => 0,
            Region::Frame => 1,
            Region::Arena(k) => 1 + k,
        }
    }

    /// Whether a value of `self` may be stored into / returned to a location of region `dst`
    /// — i.e. `self` lives at least as long as `dst`. This is the single escape rule.
    fn outlives(self, dst: Region) -> bool {
        self.ord() <= dst.ord()
    }

    /// The region of a value allocated at arena nesting `depth` (0 = outside any arena, where
    /// the result is leaked / process-lifetime → `Static`).
    fn arena(depth: u32) -> Region {
        if depth == 0 {
            Region::Static
        } else {
            Region::Arena(depth)
        }
    }

    /// The shorter-lived (higher-ordinal) of two regions — a view over both lives only as
    /// long as the shorter source.
    fn shorter(self, other: Region) -> Region {
        if self.ord() >= other.ord() {
            self
        } else {
            other
        }
    }
}

/// Arena escape checking (`03-types.md` §7, generalized per `impl/08-memory-model-v2.md`):
/// every view / arena-allocated value carries an inferred [`Region`], and the one escape rule
/// ([`Region::outlives`]) forbids it being returned to / stored into a longer-lived location.
/// A `box<T>` / arena-backed `str` is `Arena(k)`; a frame-local-backed `slice` is `Frame`.
/// Regions are inferred — never written.
struct EscapeCheck<'a> {
    f: &'a Fn,
    diags: &'a mut Diagnostics,
    /// For each box/str local, the region at which its current value was allocated.
    region: std::collections::HashMap<LocalId, Region>,
    /// For each local, the arena depth at which it was declared.
    decl_depth: std::collections::HashMap<LocalId, u32>,
    /// Slice locals bound to a view of *function-local* array storage (an array literal or
    /// local array materialized in this frame). Such a slice borrows the stack frame and so
    /// must not be returned. A slice *parameter* borrows the caller and is never in this set.
    local_backed_slice: std::collections::HashSet<LocalId>,
}

impl<'a> EscapeCheck<'a> {
    fn check(&mut self) {
        self.block(&self.f.body, 0);
        // The body's trailing value is the function's return value (single-expression
        // bodies and fall-through blocks), so apply the same slice-escape check there.
        if let Some(v) = &self.f.body.value {
            if Self::tracks_region(v.ty) && !self.region_of(v, 0).outlives(Region::Static) {
                self.diags.error(
                    "cannot return a value allocated in an arena (it is freed at block end)".to_string(),
                    v.span,
                );
            }
            if matches!(v.ty, Ty::Slice(_)) && self.slice_is_local(v) {
                self.diags.error(
                    "cannot return a slice that views a local array (it is freed when the function returns)".to_string(),
                    v.span,
                );
            }
        }
    }

    /// Types whose values carry an inferred region and so must be escape-checked: `box<T>`
    /// (M3), arena-backed `str` (M5 — `template`/concat allocate in the arena), and a struct
    /// (MMv2 slice 2 — a struct's region is the max of its fields, so a struct holding an
    /// arena-backed `str` field carries that arena region). A scalar-only struct is `Static`.
    fn tracks_region(ty: Ty) -> bool {
        matches!(ty, Ty::Box(_) | Ty::Str | Ty::Struct(_) | Ty::DynArray(_))
    }

    /// The [`Region`] a region-bearing (`box`/`str`) value is bound to. `Static` = no region
    /// (a leaked/static str, a box param — none exist — etc.). Recurses through value forms so
    /// it can't slip out via an `if`/block value.
    fn region_of(&self, e: &Expr, depth: u32) -> Region {
        match &e.kind {
            // Allocating producers are bound to the enclosing arena (Static outside any arena,
            // where the result is leaked / process-lifetime and safe to return).
            ExprKind::HeapNew(_) | ExprKind::BoxClone(_) | ExprKind::Template(_) => Region::arena(depth),
            // `.to_array()` bump-allocates the owned array in the enclosing arena.
            ExprKind::ArrayToArray { .. } => Region::arena(depth),
            // `str + str` concatenation is also built in the enclosing arena.
            ExprKind::Binary { op: BinOp::Add, .. } if e.ty == Ty::Str => Region::arena(depth),
            ExprKind::Local(p) => self.region.get(p).copied().unwrap_or(Region::Static),
            // A struct's region is the shortest-lived of its fields (a view over it lives only
            // as long as the shortest source); a scalar/literal-only struct stays `Static`.
            ExprKind::StructLit { fields, .. } => fields
                .iter()
                .fold(Region::Static, |acc, f| acc.shorter(self.region_of(f, depth))),
            // A field read inherits its base struct's region (the field may be a view into it).
            ExprKind::Field { base, .. } => self.region.get(base).copied().unwrap_or(Region::Static),
            ExprKind::Block(b) => self.region_of_block(b, depth),
            // An `arena {}` *expression* yields its block value, evaluated one level deeper.
            // Without this, a binding that captures an arena's value (`p := arena { … }`) would
            // be inferred `Static` and could then escape undetected (a use-after-free across
            // nested arenas); the per-block walk only checks the immediate boundary, not a
            // later escape of the binding.
            ExprKind::Arena(b) => self.region_of_block(b, depth + 1),
            ExprKind::If { then, els, .. } => {
                self.region_of_block(then, depth).shorter(self.region_of_block(els, depth))
            }
            _ => Region::Static,
        }
    }

    fn region_of_block(&self, b: &Block, depth: u32) -> Region {
        b.value.as_ref().map(|v| self.region_of(v, depth)).unwrap_or(Region::Static)
    }

    /// Whether a `slice<T>`-typed expression borrows *function-local* array storage (and so
    /// cannot be returned). An array literal / local array materializes in this frame; a
    /// slice parameter borrows the caller (safe). A call returns a local-backed slice iff any
    /// argument it borrows is itself local-backed (the callee can only re-borrow its args).
    fn slice_is_local(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::ArrayToSlice(_) | ExprKind::ArrayLit { .. } => true,
            ExprKind::Local(p) => self.local_backed_slice.contains(p),
            ExprKind::Call { args, .. } => args.iter().any(|a| self.slice_is_local(a)),
            ExprKind::Block(b) => b.value.as_ref().map_or(false, |v| self.slice_is_local(v)),
            ExprKind::If { then, els, .. } => {
                then.value.as_ref().map_or(false, |v| self.slice_is_local(v))
                    || els.value.as_ref().map_or(false, |v| self.slice_is_local(v))
            }
            _ => false,
        }
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
                if Self::tracks_region(init.ty) {
                    let r = self.region_of(init, depth);
                    self.region.insert(*local, r);
                }
                if matches!(init.ty, Ty::Slice(_)) && self.slice_is_local(init) {
                    self.local_backed_slice.insert(*local);
                }
            }
            Stmt::Assign { local, value } => {
                self.walk(value, depth);
                // Conservative without a dataflow join: a binding that is *ever* assigned a
                // local-backed slice stays local-backed (a later branch could reach `return`
                // while the binding still holds the local array). We never clear the flag.
                if matches!(value.ty, Ty::Slice(_)) && self.slice_is_local(value) {
                    self.local_backed_slice.insert(*local);
                }
                if Self::tracks_region(value.ty) {
                    let r = self.region_of(value, depth);
                    let target = Region::arena(*self.decl_depth.get(local).unwrap_or(&0));
                    if !r.outlives(target) {
                        self.diags.error(
                            "this value is bound to an arena block and cannot escape it".to_string(),
                            value.span,
                        );
                    }
                    // Track the reassigned binding's region for later uses.
                    self.region.insert(*local, r);
                }
            }
            Stmt::AssignField { base, value, .. } => {
                self.walk(value, depth);
                // The base struct lives at its own (fixed) region; a stored value must outlive
                // it, else the value would escape its region via the longer-lived struct.
                if Self::tracks_region(value.ty) {
                    let target = self.region.get(base).copied().unwrap_or(Region::Static);
                    if !self.region_of(value, depth).outlives(target) {
                        self.diags.error(
                            "this value cannot be stored into a longer-lived struct field (it would escape its region)".to_string(),
                            value.span,
                        );
                    }
                }
            }
            Stmt::Return(Some(e)) => {
                self.walk(e, depth);
                // A returned value escapes to the caller (`Static`): only a `Static`-region
                // value may be returned (an arena/frame view cannot).
                if Self::tracks_region(e.ty) && !self.region_of(e, depth).outlives(Region::Static) {
                    self.diags.error(
                        "cannot return a value allocated in an arena (it is freed at block end)".to_string(),
                        e.span,
                    );
                }
                if matches!(e.ty, Ty::Slice(_)) && self.slice_is_local(e) {
                    self.diags.error(
                        "cannot return a slice that views a local array (it is freed when the function returns)".to_string(),
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
                    // The block's value escapes to the enclosing region (`Region::arena(depth)`);
                    // a value bound to this inner arena cannot outlive it.
                    if Self::tracks_region(v.ty) && !self.region_of(v, inner).outlives(Region::arena(depth)) {
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
                // No per-field rejection: the struct *carries* the region of its fields
                // (`region_of`), and escape is checked when the whole struct is returned /
                // stored / used as an arena block value. Just recurse for nested escapes.
                for f in fields {
                    self.walk(f, depth);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) | ExprKind::BoxGet(i)
            | ExprKind::BoxClone(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => self.walk(i, depth),
            ExprKind::ArrayReduce { source, init, .. } => {
                self.walk(source, depth);
                self.walk(init, depth);
            }
            ExprKind::ArrayLit { elems, .. } => {
                for e in elems {
                    self.walk(e, depth);
                }
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.walk(opt, depth);
                self.walk(fallback, depth);
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.walk(h, depth);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } => self.walk(input, depth),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::Field { .. }
            | ExprKind::IndexField { .. } => {}
        }
    }
}

/// Flow analysis that flags use-after-move. A Move-typed value (M3: `box<T>`) is
/// consumed when bound/assigned/passed/returned by value; using it afterwards is an
/// error. Borrowing positions (`.get()`/`.clone()` receiver, operands) do not consume.
struct MoveCheck<'a> {
    f: &'a Fn,
    diags: &'a mut Diagnostics,
    /// Every local moved out anywhere in the body (the union over all paths). Used to decide
    /// which owned locals still need a drop (MMv2 slice 4): a moved-out local does not.
    ever_moved: std::collections::HashSet<LocalId>,
}

impl<'a> MoveCheck<'a> {
    fn check(mut self) -> std::collections::HashSet<LocalId> {
        let mut moved = std::collections::HashSet::new();
        // If the function returns a Move type, its body's trailing expression is consumed by
        // the return: a bare owned local there (`fn make() -> array<i32> { ys := ...; ys }`) is
        // moved out to the caller. Without this, such a local would stay in `drop_locals` and be
        // freed at exit while the caller also frees it — a double-free / use-after-free.
        let ret_is_move = matches!(self.f.ret, Ty::Box(_) | Ty::DynArray(_));
        self.block(&self.f.body, &mut moved, ret_is_move);
        self.ever_moved
    }

    fn is_move(&self, id: LocalId) -> bool {
        matches!(self.f.locals.get(id as usize).map(|l| l.ty), Some(Ty::Box(_) | Ty::DynArray(_)))
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
                    self.ever_moved.insert(*id);
                }
            }
            ExprKind::Field { base, .. } | ExprKind::IndexField { base, .. } => {
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
            ExprKind::BoxGet(i) | ExprKind::BoxClone(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => {
                self.expr(i, moved, false)
            }
            ExprKind::ArrayReduce { source, init, .. } => {
                self.expr(source, moved, false);
                self.expr(init, moved, false);
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
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        // A hole value is read (copied) into the builder, not moved out.
                        self.expr(h, moved, false);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } => self.expr(input, moved, false),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
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
    // Integer/float inference variables. `*_vars[i]` is the binding for the *root* of var
    // `i`; `*_parent[i]` is its union-find parent (self when `i` is a root). Linking two
    // unconstrained vars (rather than dropping one) means a later constraint on either
    // reaches both — without it they would diverge and resolve to different concrete types.
    int_vars: Vec<Option<IntTy>>,
    int_parent: Vec<u32>,
    float_vars: Vec<Option<FloatTy>>,
    float_parent: Vec<u32>,
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
        self.int_parent.push(id);
        Ty::IntVar(id)
    }

    fn fresh_float_var(&mut self) -> Ty {
        let id = self.float_vars.len() as u32;
        self.float_vars.push(None);
        self.float_parent.push(id);
        Ty::FloatVar(id)
    }

    /// Union-find root of an int/float var (no path compression — callers only read).
    fn root_int(&self, mut v: u32) -> u32 {
        while self.int_parent[v as usize] != v {
            v = self.int_parent[v as usize];
        }
        v
    }
    fn root_float(&self, mut v: u32) -> u32 {
        while self.float_parent[v as usize] != v {
            v = self.float_parent[v as usize];
        }
        v
    }

    fn resolve(&self, ty: Ty) -> Ty {
        match ty {
            Ty::IntVar(v) => {
                let r = self.root_int(v);
                match self.int_vars[r as usize] {
                    Some(it) => Ty::Int(it),
                    None => Ty::IntVar(r),
                }
            }
            Ty::FloatVar(v) => {
                let r = self.root_float(v);
                match self.float_vars[r as usize] {
                    Some(ft) => Ty::Float(ft),
                    None => Ty::FloatVar(r),
                }
            }
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
                // `v` is a resolved root (see `resolve`); bind it.
                self.int_vars[v as usize] = Some(it);
                Ty::Int(it)
            }
            (Ty::IntVar(v1), Ty::IntVar(v2)) => {
                // Both unconstrained: link their roots so a later binding reaches both.
                if v1 != v2 {
                    self.int_parent[v2 as usize] = v1;
                }
                Ty::IntVar(v1)
            }
            (Ty::FloatVar(v), Ty::Float(ft)) | (Ty::Float(ft), Ty::FloatVar(v)) => {
                self.float_vars[v as usize] = Some(ft);
                Ty::Float(ft)
            }
            (Ty::FloatVar(v1), Ty::FloatVar(v2)) => {
                if v1 != v2 {
                    self.float_parent[v2 as usize] = v1;
                }
                Ty::FloatVar(v1)
            }
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
            drop_locals: Vec::new(),
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
                        // A slice-annotated binding borrows its array source (mirrors a call arg).
                        _ => match ann {
                            Some(Ty::Slice(ps)) => self.check_slice_init(init, ps),
                            _ => self.check_expr(init, ann),
                        },
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
                        // Mirror the `let` path: a slice place borrows its array source.
                        let v = match ty {
                            Ty::Slice(ps) => self.check_slice_init(value, ps),
                            _ => self.check_expr(value, Some(ty)),
                        };
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
            ast::ExprKind::Str(s) => {
                self.constrain(Ty::Str, expected, e.span);
                Expr { kind: ExprKind::Str(s.clone()), ty: Ty::Str, span: e.span }
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
            ast::ExprKind::Template(parts) => self.check_template(parts, expected, e.span),
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
            ast::ExprKind::Try(inner) => self.check_try(inner, expected, e.span),
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
            ast::ExprKind::StructLit { name, fields } => {
                // A struct literal is a value expression (constructed, then passed/returned/
                // stored). The `let` path checks it directly to store fields in place.
                self.check_struct_lit(name, fields, e.span)
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
        // A struct is a value: it may be read whole (copied), passed, and returned.
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
                // `str + str` is concatenation; other ops on str are errors.
                if t == Ty::Str && op != BinOp::Add {
                    self.diags.error("str supports only `+` (concatenation)", span);
                } else if t != Ty::Str && !t.is_numeric() && t != Ty::Error {
                    self.diags.error("arithmetic expects numbers (int or float)", span);
                }
                ty = t;
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                l = self.check_expr(lhs, None);
                r = self.check_expr(rhs, Some(l.ty));
                let t = self.unify(l.ty, r.ty, span);
                // `str` supports only equality (no ordering yet).
                if t == Ty::Str && !matches!(op, BinOp::Eq | BinOp::Ne) {
                    self.diags
                        .error("str supports only == and != (ordering is not available)".to_string(), span);
                }
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
            .map(|(i, a)| self.check_arg(a, param_tys.get(i).copied()))
            .collect();
        Expr { kind: ExprKind::Call { func: name, args: checked }, ty: ret, span }
    }

    /// Check a call argument against a parameter type, applying an array → slice borrow
    /// when the parameter is a `slice<T>` and the argument is a matching array.
    fn check_arg(&mut self, a: &ast::Expr, param: Option<Ty>) -> Expr {
        if let Some(Ty::Slice(ps)) = param {
            return self.check_slice_init(a, ps);
        }
        self.check_expr(a, param)
    }

    /// Check an expression expected to be a `slice<T>`, applying the array → slice borrow
    /// (`ArrayToSlice`) when the source is a matching array. Shared by call arguments and
    /// slice-annotated `let` bindings so both produce a real slice value (not a bare array).
    fn check_slice_init(&mut self, a: &ast::Expr, ps: Scalar) -> Expr {
        // An inline array literal takes the slice's element type.
        let e = match &a.kind {
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, Some(scalar_to_ty(ps)), a.span),
            _ => self.check_expr(a, None),
        };
        if let Ty::Array(es, _) = e.ty {
            if es == ps {
                // The borrow lowers via the same slot-materialization as a pipeline source,
                // so the same restriction applies: only a literal or a named local.
                if !matches!(e.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
                    self.diags.error(
                        "an array coerced to a slice must be an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                        e.span,
                    );
                    return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span };
                }
                let span = e.span;
                return Expr { kind: ExprKind::ArrayToSlice(Box::new(e)), ty: Ty::Slice(ps), span };
            }
        }
        // Already a slice, or a mismatch: let unification report any error.
        if e.ty != Ty::Slice(ps) {
            self.constrain(e.ty, Some(Ty::Slice(ps)), e.span);
        }
        e
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
                if !is_printable(e.ty) {
                    self.diags
                        .error("'print' expects an int, float, str, bool, or char".to_string(), e.span);
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
            if single_name(p) == Some("json") && method == "encode" {
                return self.check_json_encode(args, span);
            }
            if single_name(p) == Some("json") && method == "decode" {
                return self.check_json_decode(args, expected, span);
            }
        }
        // `sum` / `reduce` are the terminals of a fused pipeline.
        if method == "sum" {
            return self.check_array_sum(recv, args, expected, span);
        }
        if method == "reduce" {
            return self.check_array_reduce(recv, args, expected, span);
        }
        if method == "count" {
            return self.check_array_count(recv, args, span);
        }
        if method == "any" || method == "all" {
            return self.check_array_any_all(recv, args, method == "all", span);
        }
        if method == "to_array" {
            return self.check_array_to_array(recv, args, span);
        }
        // `.len()` of a `str`/`slice`/array — the element count (an `i64`).
        if method == "len" {
            return self.check_len(recv, args, span);
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
    /// `template "...{hole}..."` — each hole is a local of int or str type; the result
    /// is a `str`.
    fn check_template(&mut self, parts: &[ast::TemplatePart], expected: Option<Ty>, span: Span) -> Expr {
        let mut hparts = Vec::new();
        for p in parts {
            match p {
                ast::TemplatePart::Text(s) => hparts.push(TemplatePart::Text(s.clone())),
                ast::TemplatePart::Hole(expr) => {
                    let e = self.check_expr(expr, None);
                    if !is_printable(e.ty) {
                        self.diags.error(
                            format!("a template hole must be an int, float, str, bool, or char, got {}", ty_name(e.ty)),
                            e.span,
                        );
                    }
                    hparts.push(TemplatePart::Hole(e));
                }
            }
        }
        self.constrain(Ty::Str, expected, span);
        Expr { kind: ExprKind::Template(hparts), ty: Ty::Str, span }
    }

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
    /// Collect and type-check a pipeline `src.map(f).where(p).field…`, returning the
    /// checked source, its stages, and the final element type. `elem_expected_no_stages`
    /// is the element type to push into an inline literal when there are no stages.
    fn check_pipeline(&mut self, recv: &ast::Expr, elem_expected_no_stages: Option<Ty>, span: Span) -> Option<(Expr, Vec<Stage>, Ty)> {
        let (source_ast, raw_stages) = self.collect_pipeline(recv);
        // The expected element type for an inline scalar literal source: the first Map
        // stage's parameter, or (with no stages) the caller-provided hint.
        let elem_expected = match raw_stages.first() {
            Some(RawStage::Map(fname)) => self.sigs.get(&fname.name).and_then(|s| s.params.first().copied()),
            None => elem_expected_no_stages,
            _ => None,
        };
        let source = match &source_ast.kind {
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, elem_expected, span),
            _ => self.check_expr(source_ast, None),
        };
        let mut elem = match source.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => scalar_to_ty(s),
            Ty::StructArray(id, _) => Ty::Struct(id),
            Ty::Error => return None,
            other => {
                self.diags
                    .error(format!("a pipeline source must be an array, got {}", ty_name(other)), span);
                return None;
            }
        };
        // MIR materializes an array source only when it is an array literal or a named
        // local (slot-addressable); an arbitrary array-valued expression (e.g. an `if` or
        // block) would otherwise crash lowering. A slice source is fine — it lowers as a
        // value. Reject the unsupported array shape cleanly here.
        if matches!(source.ty, Ty::Array(..) | Ty::StructArray(..))
            && !matches!(source.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_))
        {
            self.diags.error(
                "a pipeline over an array must start from an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return None;
        }

        // Field projection / field-predicate stages index the source by element
        // (`IndexField(slot, …)` in MIR), which needs a slot-backed source — a stack array
        // or struct array. A `{ptr,len}` view (`slice`/owned `array`) has no such slot, so
        // projecting a field out of one is not supported (it would miscompile).
        let slot_backed = matches!(source.ty, Ty::Array(..) | Ty::StructArray(..));
        let mut stages = Vec::new();
        for raw in raw_stages {
            match raw {
                RawStage::Project(field) => {
                    if !slot_backed {
                        self.diags.error(
                            format!("'.{}' field projection needs an array source, not a slice/array view", field.name),
                            field.span,
                        );
                        return None;
                    }
                    if !matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'.{}' projection needs a struct element, got {}", field.name, ty_name(elem)),
                            field.span,
                        );
                        return None;
                    }
                    match self.field_of(elem, &field.name, field.span) {
                        Some((index, ty)) => {
                            stages.push(Stage { kind: StageKind::Project { field: index }, out_ty: ty });
                            elem = ty;
                        }
                        None => return None,
                    }
                }
                RawStage::Map(fname) => {
                    // A struct element must be projected to a scalar first: MIR keeps a
                    // struct array addressed by index until a `.field`, so a `map`/`where`
                    // over the whole struct has nothing loaded and would crash lowering.
                    if matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'map' over a struct element is not supported yet (project a field first), got {}", ty_name(elem)),
                            fname.span,
                        );
                        return None;
                    }
                    let ret = self.check_stage_fn(&fname, elem, false);
                    stages.push(Stage { kind: StageKind::Map { func: fname.name }, out_ty: ret });
                    elem = ret;
                }
                RawStage::Where(fname) => {
                    if matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'where' over a struct element is not supported yet (use 'where(.field)' or project first), got {}", ty_name(elem)),
                            fname.span,
                        );
                        return None;
                    }
                    self.check_stage_fn(&fname, elem, true);
                    stages.push(Stage { kind: StageKind::Where { func: fname.name }, out_ty: elem });
                }
                RawStage::WhereField(field) => {
                    if !slot_backed {
                        self.diags.error(
                            format!("'where(.{})' needs an array source, not a slice/array view", field.name),
                            field.span,
                        );
                        return None;
                    }
                    if !matches!(elem, Ty::Struct(_)) {
                        self.diags.error(
                            format!("'where(.{})' needs a struct element, got {}", field.name, ty_name(elem)),
                            field.span,
                        );
                        return None;
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
                        None => return None,
                    }
                }
            }
        }
        Some((source, stages, elem))
    }

    /// `src.…​.sum()` — fold the (numeric) post-stage elements with `+`.
    fn check_array_sum(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'sum' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, elem)) = self.check_pipeline(recv, expected, span) else {
            return err;
        };
        if !elem.is_numeric() {
            self.diags
                .error(format!("'sum' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArraySum { source: Box::new(source), stages }, ty: elem, span }
    }

    /// `source.….count()` — the count of elements surviving the stages, as an `i64`. The
    /// element type is unconstrained (a struct element needs no projection), unlike `sum`.
    fn check_array_count(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'count' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, _elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        Expr {
            kind: ExprKind::ArrayCount { source: Box::new(source), stages },
            ty: Ty::Int(IntTy { bits: 64, signed: true }),
            span,
        }
    }

    /// `src.….to_array()` — materialize the surviving (scalar) elements into an *owned*
    /// `array<T>`. MMv2 slice 3: the result is arena-bump-allocated (bulk-freed), so it is
    /// only allowed inside an `arena {}`; free-standing (heap + drop) arrives in slice 4.
    fn check_array_to_array(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'to_array' takes no arguments".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Inside an arena → bump-allocated (bulk-freed). Outside → free-standing heap with a
        // per-binding drop (MMv2 slice 4). Both are fine now.
        let Some((source, stages, elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        let Some(scalar) = ty_to_scalar(elem) else {
            self.diags.error(
                format!("'to_array' needs a scalar element, got {} (project a field first)", ty_name(elem)),
                span,
            );
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'to_array' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        Expr {
            kind: ExprKind::ArrayToArray { source: Box::new(source), stages, elem },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `src.….any(p)` / `.all(p)` — whether predicate `p: E -> bool` holds for any / all
    /// surviving elements. The element must be a scalar (project a struct field first), so
    /// the fused loop has a concrete value to test. Always returns `bool`.
    fn check_array_any_all(&mut self, recv: &ast::Expr, args: &[ast::Expr], all: bool, span: Span) -> Expr {
        let name = if all { "all" } else { "any" };
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags
                .error(format!("'{name}' takes 1 argument (a predicate function), got {}", args.len()), span);
            return err;
        };
        let Some(fname) = self.pipeline_fn_name(fn_arg) else {
            self.diags.error(format!("'{name}' needs a named predicate function"), span);
            return err;
        };
        // The predicate's parameter type guides an inline source's element type.
        let elem_hint = self.sigs.get(&fname.name).and_then(|s| s.params.first().copied());
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if ty_to_scalar(elem).is_none() {
            self.diags.error(
                format!("'{name}' needs a scalar element, got {} (project a field first)", ty_name(elem)),
                span,
            );
            return err;
        }
        // Predicate must be `(elem) -> bool`. On a bad/undefined predicate, return the error
        // sentinel — a Call to a missing/mistyped function must not reach MIR/codegen.
        match self.sigs.get(&fname.name) {
            Some(sig) if sig.params.len() == 1 && sig.params[0] == elem && sig.ret == Ty::Bool => {}
            Some(_) => {
                self.diags.error(
                    format!("'{name}' predicate '{}' must have type ({}) -> bool", fname.name, ty_name(elem)),
                    fname.span,
                );
                return err;
            }
            None => {
                self.diags.error(format!("undefined function: '{}'", fname.name), fname.span);
                return err;
            }
        }
        Expr {
            kind: ExprKind::ArrayAnyAll { source: Box::new(source), stages, func: fname.name, all },
            ty: Ty::Bool,
            span,
        }
    }

    /// `src.…​.reduce(f, init)` — fold the post-stage elements with `f: (A, E) -> A`,
    /// starting from `init: A`.
    fn check_array_reduce(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg, init_arg] = args else {
            self.diags.error(format!("'reduce' takes 2 arguments (a function and an initial value), got {}", args.len()), span);
            return err;
        };
        let Some(fname) = self.pipeline_fn_name(fn_arg) else {
            self.diags.error("'reduce' needs a named function as its first argument".to_string(), span);
            return err;
        };
        let Some(sig) = self.sigs.get(&fname.name) else {
            self.diags.error(format!("undefined function: '{}'", fname.name), fname.span);
            return err;
        };
        let (params, acc_ty) = (sig.params.clone(), sig.ret);
        // The element type the fold expects (its 2nd parameter) guides an inline source.
        let elem_hint = params.get(1).copied();
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if params.len() != 2 || params[0] != acc_ty || params[1] != elem {
            self.diags.error(
                format!("'reduce' function '{}' must have type ({}, {}) -> {}", fname.name, ty_name(acc_ty), ty_name(elem), ty_name(acc_ty)),
                fname.span,
            );
        }
        let init = self.check_expr(init_arg, Some(acc_ty));
        self.constrain(acc_ty, expected, span);
        Expr {
            kind: ExprKind::ArrayReduce { source: Box::new(source), stages, func: fname.name, init: Box::new(init) },
            ty: acc_ty,
            span,
        }
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
        // A box payload must be a true scalar — `ty_to_scalar` now also accepts structs (a
        // valid Option/Result payload), so reject a struct box explicitly (codegen can't size it).
        if matches!(arg.ty, Ty::Struct(_)) {
            self.diags
                .error("a box payload must be a primitive scalar (struct boxes are not supported)".to_string(), args[0].span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let scalar = self.payload_scalar(arg.ty, args[0].span);
        Expr { kind: ExprKind::HeapNew(Box::new(arg)), ty: Ty::Box(scalar), span }
    }

    /// `json.encode(s)` — encode a flat struct into a JSON object `str`. Desugars to the
    /// string-builder `template` machinery: static JSON syntax interleaved with per-field
    /// value holes (`str` fields are emitted as JSON-escaped string literals). M5: fields
    /// must be int/float/bool/str; nested structs/arrays/options are not supported yet. The
    /// result is arena-backed when inside an `arena {}` (else leaked), like any built string.
    fn check_json_encode(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'json.encode' expects 1 argument, got {}", args.len()), span);
            return err;
        }
        let Some((base, ty)) = self.place_local(&args[0]) else {
            self.diags
                .error("'json.encode' expects a struct or struct-array value (a local binding)".to_string(), args[0].span);
            return err;
        };
        let mut parts = vec![];
        let mut ok = true;
        match ty {
            // A single struct → a JSON object.
            Ty::Struct(sid) => {
                self.json_object_parts(base, sid, None, &mut parts, args[0].span, &mut ok);
            }
            // A fixed struct-array → a JSON array of objects (unrolled; length is static).
            Ty::StructArray(sid, n) => {
                parts.push(TemplatePart::Text("[".to_string()));
                for i in 0..n {
                    if i > 0 {
                        parts.push(TemplatePart::Text(",".to_string()));
                    }
                    self.json_object_parts(base, sid, Some(i), &mut parts, args[0].span, &mut ok);
                }
                parts.push(TemplatePart::Text("]".to_string()));
            }
            _ => {
                self.diags
                    .error(format!("'json.encode' expects a struct or struct-array, got {}", ty_name(ty)), args[0].span);
                return err;
            }
        }
        // An unsupported field left a `"name":` with no value part: return the error
        // sentinel rather than a malformed template (matches the other checks' convention).
        if !ok {
            return err;
        }
        Expr { kind: ExprKind::Template(parts), ty: Ty::Str, span }
    }

    /// `json.decode(input)` — parse a `str` into a struct at runtime, yielding
    /// `Result<Struct, Error>`. The target struct `T` is taken from the expected type
    /// (a `Result<T, _>`, e.g. from `let u: T := json.decode(d)?`); `<T>` call syntax is
    /// future. M5 cut: a flat struct of `i64`/`i32`/`bool` fields (str/float/nested later).
    fn check_json_decode(&mut self, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if args.len() != 1 {
            self.diags
                .error(format!("'json.decode' expects 1 argument, got {}", args.len()), span);
            return err;
        }
        // The decode target is the Ok type of the expected `Result<T, _>`.
        let sid = match expected.map(|e| self.resolve(e)) {
            Some(Ty::Result(Scalar::Struct(id), _)) => id,
            _ => {
                self.diags.error(
                    "cannot infer the decode target type; annotate the binding, e.g. `u: T := json.decode(d)?`".to_string(),
                    span,
                );
                return err;
            }
        };
        // Every field must be a decodable scalar (int / float / bool). `str`/array fields
        // need zero-copy borrow-region decode → deferred to Memory Model v2.
        let fields = self.structs[sid as usize].fields.clone();
        for f in &fields {
            if !matches!(f.ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool) {
                self.diags.error(
                    format!("'json.decode' field '{}' has type {} (only int/float/bool decode for now)", f.name, ty_name(f.ty)),
                    span,
                );
                return err;
            }
        }
        let input = self.check_expr(&args[0], Some(Ty::Str));
        if input.ty != Ty::Str && input.ty != Ty::Error {
            self.diags
                .error(format!("'json.decode' input must be a str, got {}", ty_name(input.ty)), args[0].span);
        }
        Expr {
            kind: ExprKind::JsonDecode { struct_id: sid, input: Box::new(input) },
            ty: Ty::Result(Scalar::Struct(sid), Scalar::ErrCode),
            span,
        }
    }

    /// Emit the `{"field":value,...}` template parts for one struct value: either the struct
    /// local `base` itself (`elem` = None) or element `elem` of the struct-array local `base`.
    /// Sets `*ok = false` (and reports) on a field type `json.encode` can't render yet.
    fn json_object_parts(
        &mut self,
        base: LocalId,
        sid: u32,
        elem: Option<u32>,
        parts: &mut Vec<TemplatePart>,
        span: Span,
        ok: &mut bool,
    ) {
        // `self.structs` is a `&'a [StructDef]`, so this borrow is tied to `'a`, not `self`
        // — `self.diags` stays mutably borrowable in the loop (no clone needed).
        let fields = &self.structs[sid as usize].fields;
        parts.push(TemplatePart::Text("{".to_string()));
        for (i, f) in fields.iter().enumerate() {
            let sep = if i == 0 { "" } else { "," };
            parts.push(TemplatePart::Text(format!("{sep}\"{}\":", f.name)));
            let kind = match elem {
                None => ExprKind::Field { base, index: i as u32 },
                Some(e) => ExprKind::IndexField { base, index: e, field: i as u32 },
            };
            let field_expr = Expr { kind, ty: f.ty, span };
            match f.ty {
                Ty::Str => parts.push(TemplatePart::JsonStr(field_expr)),
                t if t.is_numeric() || t == Ty::Bool => parts.push(TemplatePart::Hole(field_expr)),
                _ => {
                    self.diags.error(
                        format!(
                            "'json.encode' field '{}' has unsupported type {} (int/float/bool/str only for now)",
                            f.name,
                            ty_name(f.ty)
                        ),
                        span,
                    );
                    *ok = false;
                }
            }
        }
        parts.push(TemplatePart::Text("}".to_string()));
    }

    /// `.len()` — the element count of a `str`, `slice<T>`, or fixed array, as an `i64`.
    fn check_len(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        if !args.is_empty() {
            self.diags.error(format!("'.len()' takes no arguments, got {}", args.len()), span);
        }
        let r = self.check_expr(recv, None);
        match r.ty {
            // `str`/`slice` carry a runtime length in their `{ ptr, len }` view.
            Ty::Str | Ty::Slice(_) | Ty::DynArray(_) => Expr { kind: ExprKind::Len(Box::new(r)), ty: i64_ty, span },
            // A fixed array's length is known at compile time.
            Ty::Array(_, n) | Ty::StructArray(_, n) => Expr { kind: ExprKind::Int(n as i128), ty: i64_ty, span },
            Ty::Error => Expr { kind: ExprKind::Int(0), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.len()' is not defined on {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
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
    fn check_try(&mut self, inner: &ast::Expr, expected: Option<Ty>, span: Span) -> Expr {
        // Thread the expected unwrapped (Ok) type inward as a `Result<expected, ret_err>`, so
        // a context type can drive inference inside the `?` operand (e.g. `json.decode`'s T
        // from `let u: User := json.decode(d)?`). The err type comes from the function's
        // return Result, matching the `?` propagation rule below.
        let inner_expected = match (expected, self.resolve(self.ret_hint)) {
            (Some(ok), Ty::Result(_, err)) => ty_to_scalar(ok).map(|o| Ty::Result(o, err)),
            _ => None,
        };
        let v = self.check_expr(inner, inner_expected);
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
            | ExprKind::BoxClone(inner) | ExprKind::ArraySum { source: inner, .. } | ExprKind::ArrayCount { source: inner, .. } | ExprKind::ArrayAnyAll { source: inner, .. } | ExprKind::ArrayToArray { source: inner, .. } | ExprKind::ArrayToSlice(inner)
            | ExprKind::Len(inner) => {
                self.finalize_expr(inner)
            }
            ExprKind::ArrayReduce { source, init, .. } => {
                self.finalize_expr(source);
                self.finalize_expr(init);
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
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.finalize_expr(h);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } => self.finalize_expr(input),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::Field { .. }
            | ExprKind::IndexField { .. } => {}
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

/// Types `print` and a `template` hole can render: integers, floats, `str`, `bool`, `char`
/// (and the error sentinel, to avoid cascading diagnostics).
fn is_printable(ty: Ty) -> bool {
    ty.is_numeric() || matches!(ty, Ty::Str | Ty::Bool | Ty::Char | Ty::Error)
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
        Ty::Slice(s) => format!("slice<{}>", scalar_name(s)),
        Ty::DynArray(s) => format!("array<{}>", scalar_name(s)),
        Ty::Str => "str".to_string(),
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
        "str" => Ty::Str,
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
            // `scalar_arg` now also accepts a struct (a valid Option/Result payload), but a
            // box payload must be a true scalar (codegen can't size a struct box) — reject it.
            match scalar_arg(inner, "box payload", t.span, diags) {
                Some(Scalar::Struct(_)) => {
                    diags.error("a box payload must be a primitive scalar (struct boxes are not supported)".to_string(), t.span);
                    Ty::Error
                }
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
        "slice" => {
            let inner = match t.args.as_slice() {
                [a] => resolve_type(a, struct_ids, diags),
                _ => {
                    diags.error("slice takes exactly one type argument".to_string(), t.span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "slice element", t.span, diags) {
                Some(s) => Ty::Slice(s),
                None => Ty::Error,
            }
        }
        // `array<T>` — an owned, dynamic-length array (MMv2). Currently usable as a return
        // type so a function can hand back a free-standing owned array.
        "array" => {
            let inner = match t.args.as_slice() {
                [a] => resolve_type(a, struct_ids, diags),
                _ => {
                    diags.error("array takes exactly one type argument".to_string(), t.span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "array element", t.span, diags) {
                Some(s) => Ty::DynArray(s),
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
    fn region_lattice_outlives() {
        // Static ⊐ Frame ⊐ Arena(1) ⊐ Arena(2): longer-lived outlives shorter-lived.
        assert!(Region::Static.outlives(Region::Frame));
        assert!(Region::Static.outlives(Region::Arena(1)));
        assert!(Region::Frame.outlives(Region::Arena(1)));
        assert!(Region::Arena(1).outlives(Region::Arena(2)));
        assert!(Region::Static.outlives(Region::Static));
        // …and not the reverse.
        assert!(!Region::Frame.outlives(Region::Static));
        assert!(!Region::Arena(1).outlives(Region::Frame));
        assert!(!Region::Arena(2).outlives(Region::Arena(1)));
        // `arena(0)` is the leaked / process-lifetime case → Static; deeper = shorter-lived.
        assert_eq!(Region::arena(0), Region::Static);
        assert!(!Region::arena(2).outlives(Region::arena(1)));
        // `shorter` picks the shorter-lived (the one that bounds a view over both).
        assert_eq!(Region::Static.shorter(Region::Arena(1)), Region::Arena(1));
        assert_eq!(Region::Arena(2).shorter(Region::Frame), Region::Arena(2));
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
    fn string_program_checks() {
        let (_p, d) = check("fn g() -> str = \"hi\"\nfn main() -> i32 {\n  print(g())\n  print(\"x\")\n  return 0\n}\n");
        assert!(!d.has_errors(), "string literals + print(str) should check");
    }

    #[test]
    fn str_equality_checks_but_ordering_errors() {
        let (_p, ok) = check("fn f(s: str) -> bool = s == \"x\"\n");
        assert!(!ok.has_errors(), "str == str should check");
        let (_q, bad) = check("fn f(s: str) -> bool = s < \"x\"\n");
        assert!(bad.has_errors(), "str ordering must error");
    }

    #[test]
    fn struct_payload_does_not_leak_into_fields_or_box() {
        // Allowing struct Option/Result payloads must NOT accidentally allow nested struct
        // fields or struct boxes (both would panic in codegen).
        let (_p, nested) = check("A { v: i32 }\nB { a: A }\nfn main() -> i32 { return 0 }\n");
        assert!(nested.has_errors(), "a nested struct field must still be rejected");
        let (_q, boxed) = check("P { x: i32 }\nfn main() -> i32 {\n  arena {\n    b := heap.new(P{x: 1})\n  }\n  return 0\n}\n");
        assert!(boxed.has_errors(), "a struct box payload must still be rejected");
        let (_r, boxann) = check("P { x: i32 }\nfn f(b: box<P>) -> i32 = 0\nfn main() -> i32 { return 0 }\n");
        assert!(boxann.has_errors(), "a box<Struct> annotation must still be rejected");
    }

    #[test]
    fn json_decode_checks_and_infers_target() {
        // T is inferred from the binding annotation through `?`.
        let (_p, ok) = check("User { id: i64, active: bool }\nfn parse(s: str) -> Result<User, Error> {\n  u: User := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> i32 { return 0 }\n");
        assert!(!ok.has_errors(), "json.decode into an annotated struct should check");
        // Without an inferable target type, decode errors.
        let (_q, noty) = check("fn main() -> i32 {\n  x := json.decode(\"{}\")\n  return 0\n}\n");
        assert!(noty.has_errors(), "json.decode needs an inferable target type");
        // A non-int/bool field is rejected for now.
        let (_r, badf) = check("U { name: str }\nfn parse(s: str) -> Result<U, Error> {\n  u: U := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> i32 { return 0 }\n");
        assert!(badf.has_errors(), "a str field is not decodable yet");
    }

    #[test]
    fn result_option_struct_payload_checks() {
        // A struct can be an Ok/Some payload; `?` unwraps to the struct, `else` to it too.
        let (_p, r) = check("Pt { x: i32 }\nfn mk() -> Result<Pt, Error> {\n  p := Pt{x: 1}\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  q := mk()?\n  print(q.x)\n  return Ok(())\n}\n");
        assert!(!r.has_errors(), "Result<Struct, Error> should check");
        let (_q, o) = check("Pt { x: i32 }\nfn pick() -> Option<Pt> {\n  p := Pt{x: 1}\n  return Some(p)\n}\nfn main() -> i32 {\n  q := pick() else { return 9 }\n  return q.x\n}\n");
        assert!(!o.has_errors(), "Option<Struct> should check");
    }

    #[test]
    fn struct_str_field_ok() {
        // A `str` struct field is allowed; reading it back is fine.
        let (_p, d) = check("User { name: str }\nfn main() -> i32 {\n  u := User{name: \"ada\"}\n  print(u.name)\n  return 0\n}\n");
        assert!(!d.has_errors(), "str struct fields are allowed (region-0 strs)");
    }

    #[test]
    fn struct_arena_str_field_ok_when_not_escaping() {
        // MMv2 slice 2: a struct may now hold an arena-backed str. As long as the struct does
        // not escape the arena (here it is only used inside it), this is safe and allowed.
        let (_p, d) = check("P { tag: str }\nfn main() -> i32 {\n  a := \"x\"\n  b := \"y\"\n  arena {\n    p := P{tag: a + b}\n    print(p.tag)\n  }\n  return 0\n}\n");
        assert!(!d.has_errors(), "a struct holding an arena str is fine if it does not escape");
    }

    #[test]
    fn struct_with_arena_str_field_cannot_escape() {
        // The struct carries its field's arena region, so returning it out of the arena (as the
        // arena block's value, which becomes the function result) must be rejected.
        let (_p, d) = check("P { tag: str }\nfn mk(a: str, b: str) -> P {\n  arena {\n    P{tag: a + b}\n  }\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a struct holding an arena str must not escape its arena");
    }

    #[test]
    fn struct_nested_arena_escape_rejected() {
        // A binding that captures an inner arena's value must keep that arena's region, so it
        // cannot be assigned to an outer-arena binding (which would outlive it → use-after-free).
        let (_p, d) = check("P { tag: str }\nfn main() -> i32 {\n  arena {\n    mut out := P{tag: \"init\"}\n    arena {\n      x := \"a\" + \"b\"\n      p := arena {\n        P{tag: x}\n      }\n      out = p\n    }\n  }\n  return 0\n}\n");
        assert!(d.has_errors(), "a value captured from an inner arena must not escape to an outer one");
    }

    #[test]
    fn struct_with_literal_str_field_returns_ok() {
        // A struct whose str field is a literal (region-0 / Static) stays freely returnable.
        let (_p, d) = check("P { tag: str }\nfn mk() -> P {\n  return P{tag: \"lit\"}\n}\nfn main() -> i32 { return 0 }\n");
        assert!(!d.has_errors(), "a struct with a literal str field is Static and returnable");
    }

    #[test]
    fn arena_str_into_outer_struct_field_rejected() {
        // Assigning an arena str into a field of a struct declared in an outer (longer-lived)
        // scope would let it outlive the arena via that struct.
        let (_p, d) = check("P { tag: str }\nfn main() -> i32 {\n  a := \"x\"\n  b := \"y\"\n  mut p := P{tag: \"init\"}\n  arena {\n    p.tag = a + b\n  }\n  print(p.tag)\n  return 0\n}\n");
        assert!(d.has_errors(), "storing an arena str into an outer struct's field must be rejected");
    }

    #[test]
    fn struct_box_field_still_rejected() {
        // box fields remain unsupported (only scalars and str for now).
        let (_p, d) = check("B { b: box<i32> }\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "box struct fields are still rejected");
    }

    #[test]
    fn struct_float_field_ok() {
        let (_p, d) = check("P { x: f64, y: f64 }\nfn main() -> i32 {\n  p := P{x: 1.5, y: 2.5}\n  if p.x + p.y > 3.0 { return 1 }\n  return 0\n}\n");
        assert!(!d.has_errors(), "float struct fields should check");
    }

    #[test]
    fn struct_by_value_param_return_copy() {
        // Pass a struct by value, copy it, and return it; construct via a struct-literal body.
        let (_p, d) = check("P { x: i32, y: i32 }\nfn sum(p: P) -> i32 = p.x + p.y\nfn dup(p: P) -> P {\n  q := p\n  return q\n}\nfn mk(v: i32) -> P = P{x: v, y: v}\nfn main() -> i32 {\n  a := mk(21)\n  b := dup(a)\n  return sum(b)\n}\n");
        assert!(!d.has_errors(), "struct pass/return/copy + struct-literal expressions should check");
    }

    #[test]
    fn whole_struct_reassign_ok() {
        let (_p, d) = check("P { x: i32 }\nfn mk(v: i32) -> P = P{x: v}\nfn main() -> i32 {\n  mut p := P{x: 1}\n  p = mk(7)\n  return p.x\n}\n");
        assert!(!d.has_errors(), "whole-struct reassignment should check");
    }

    #[test]
    fn arena_backed_str_cannot_escape() {
        let (_p, d) = check("fn f() -> str {\n  arena {\n    \"x\" + \"y\"\n  }\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "an arena-backed str must not escape its arena");
    }

    #[test]
    fn slice_of_local_array_cannot_be_returned() {
        // A slice that views a stack-local array literal dies when the function returns.
        let (_p, d) = check("fn bad() -> slice<i64> {\n  s: slice<i64> := [1, 2, 3]\n  return s\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a slice into a local array must not escape via return");
    }

    #[test]
    fn slice_borrowing_local_array_via_call_cannot_be_returned() {
        // first() re-borrows its arg; returning it leaks a view into bad()'s temp array.
        let (_p, d) = check("fn first(xs: slice<i64>) -> slice<i64> = xs\nfn bad() -> slice<i64> {\n  return first([1, 2, 3])\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a slice re-borrowed from a local array must not escape");
    }

    #[test]
    fn slice_local_backed_via_conditional_assign_cannot_escape() {
        // Without a dataflow join we must stay conservative: a binding ever holding a
        // local-backed slice cannot be returned, even if a branch reassigns a param slice.
        let (_p, d) = check("fn pick(p: slice<i32>) -> slice<i32> {\n  mut s: slice<i32> := [1, 2, 3]\n  if true { s = p }\n  return s\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a conditionally-reassigned local-backed slice must not escape");
    }

    #[test]
    fn slice_array_literal_reassign_cannot_escape() {
        // Reassigning an array literal to a slice local borrows frame-local storage (and is
        // coerced like a `let`), so the binding becomes local-backed and cannot be returned.
        let (_p, d) = check("fn bad(p: slice<i32>) -> slice<i32> {\n  mut s: slice<i32> := p\n  s = [1, 2, 3]\n  return s\n}\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "a slice reassigned from a local array must not escape");
    }

    #[test]
    fn slice_param_passthrough_returns_ok() {
        // A slice parameter borrows the caller, so returning it (directly or re-borrowed) is fine.
        let (_p, d) = check("fn id(xs: slice<i64>) -> slice<i64> = xs\nfn g(ys: slice<i64>) -> slice<i64> = id(ys)\n");
        assert!(!d.has_errors(), "returning a slice parameter is safe (it borrows the caller)");
    }

    #[test]
    fn slice_local_used_in_function_is_ok() {
        // A slice into a local array is fine as long as it does not outlive the frame.
        let (_p, d) = check("fn main() -> i32 {\n  s: slice<i32> := [10, 20, 12]\n  return s.sum()\n}\n");
        assert!(!d.has_errors(), "a non-escaping slice local should check");
    }

    #[test]
    fn non_arena_str_returns_ok() {
        let (_p, d) = check("fn g(a: str, b: str) -> str = a + b\nfn h() -> str = \"lit\"\n");
        assert!(!d.has_errors(), "a non-arena str is returnable (leaked / process-lifetime)");
    }

    #[test]
    fn str_concat_checks_but_other_ops_error() {
        let (_p, ok) = check("fn f(a: str, b: str) -> str = a + b\n");
        assert!(!ok.has_errors(), "str + str should check");
        let (_q, bad) = check("fn f(a: str, b: str) -> str = a - b\n");
        assert!(bad.has_errors(), "str only supports +");
    }

    #[test]
    fn template_checks() {
        let (_p, d) = check("fn main() -> i32 {\n  n := \"x\"\n  k := 1\n  m := template \"{n}={k}\"\n  print(m)\n  return 0\n}\n");
        assert!(!d.has_errors(), "a template with str/int holes should check");
    }

    #[test]
    fn template_undefined_hole_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  m := template \"hi {who}\"\n  return 0\n}\n");
        assert!(d.has_errors(), "an undefined template hole must error");
    }

    #[test]
    fn template_expression_holes_check() {
        // `{expr}` holes: arithmetic and str concatenation are both valid.
        let (_p, d) = check("fn main() -> i32 {\n  a := 20\n  b := 22\n  n := \"x\"\n  m := template \"{a + b} {a * 2} {n + \\\"!\\\"}\"\n  print(m)\n  return 0\n}\n");
        assert!(!d.has_errors(), "arithmetic and str-concat holes should check");
    }

    #[test]
    fn template_bool_and_char_holes_check() {
        // bool and char holes are interpolatable.
        let (_p, d) = check("fn main() -> i32 {\n  c := 'x'\n  print(template \"{1 > 2} {c}\")\n  return 0\n}\n");
        assert!(!d.has_errors(), "bool and char template holes should check");
    }

    #[test]
    fn template_float_hole_checks() {
        // A float hole is interpolatable (rendered via the runtime's shortest round-trip).
        let (_p, d) = check("fn main() -> i32 {\n  print(template \"{1.5} {2.0 + 0.5}\")\n  return 0\n}\n");
        assert!(!d.has_errors(), "a float template hole should check");
    }

    #[test]
    fn print_accepts_bool_char_float() {
        let (_p, d) = check("fn main() -> i32 {\n  print(true)\n  print('a')\n  print(3.14)\n  return 0\n}\n");
        assert!(!d.has_errors(), "print accepts bool, char, and float");
    }

    #[test]
    fn len_checks_on_str_slice_array() {
        let (_p, d) = check("fn slen(xs: slice<i32>) -> i64 = xs.len()\nfn main() -> i32 {\n  s := \"hi\"\n  a := [1, 2, 3]\n  print(s.len())\n  print(a.len())\n  print(slen([4, 5]))\n  return 0\n}\n");
        assert!(!d.has_errors(), ".len() should check on str, array, and slice");
    }

    #[test]
    fn len_rejects_non_sequence() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 5\n  print(x.len())\n  return 0\n}\n");
        assert!(d.has_errors(), ".len() is not defined on an integer");
    }

    #[test]
    fn json_encode_struct_checks() {
        let (_p, d) = check("User { id: i64, name: str, active: bool }\nfn main() -> i32 {\n  u := User{id: 1, name: \"a\", active: true}\n  print(json.encode(u))\n  return 0\n}\n");
        assert!(!d.has_errors(), "json.encode of a flat struct should check");
    }

    #[test]
    fn json_encode_struct_array_checks() {
        let (_p, d) = check("User { id: i64, name: str }\nfn main() -> i32 {\n  us := [User{id: 1, name: \"a\"}, User{id: 2, name: \"b\"}]\n  print(json.encode(us))\n  return 0\n}\n");
        assert!(!d.has_errors(), "json.encode of a struct array should check");
    }

    #[test]
    fn json_encode_rejects_non_struct() {
        let (_p, d) = check("fn main() -> i32 {\n  x := 5\n  print(json.encode(x))\n  return 0\n}\n");
        assert!(d.has_errors(), "json.encode requires a struct");
    }

    #[test]
    fn json_encode_rejects_unsupported_field() {
        // A char field is a valid struct field but not encodable yet; json.encode must error
        // (and not return a malformed template).
        let (_p, d) = check("C { ch: char, n: i32 }\nfn main() -> i32 {\n  c := C{ch: 'x', n: 1}\n  print(json.encode(c))\n  return 0\n}\n");
        assert!(d.has_errors(), "json.encode rejects a struct with an unsupported field type");
    }

    #[test]
    fn print_rejects_non_scalar() {
        // An Option is not a printable scalar.
        let (_p, d) = check("fn main() -> i32 {\n  print(Some(1))\n  return 0\n}\n");
        assert!(d.has_errors(), "print rejects non-scalar values like Option");
    }

    #[test]
    fn reduce_checks() {
        let (_p, d) = check(
            "fn add(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  return [1, 2, 3].reduce(add, 0)\n}\n",
        );
        assert!(!d.has_errors(), "reduce with a matching fold should check");
    }

    #[test]
    fn any_all_check_and_require_scalar_element() {
        let (_p, ok) = check("fn big(x: i64) -> bool = x > 4\nfn pos(x: i64) -> bool = x > 0\nfn main() -> i32 {\n  if [1, 2, 3].any(big) { return 1 }\n  if [1, 2, 3].all(pos) { return 2 }\n  return 0\n}\n");
        assert!(!ok.has_errors(), "any/all over a scalar array should check");
        // A struct element (no projection) is rejected — project a field first.
        let (_q, bad) = check("fn f(e: i32) -> bool = e > 0\nE { pay: i32 }\nfn main() -> i32 {\n  if [E{pay: 1}].any(f) { return 1 }\n  return 0\n}\n");
        assert!(bad.has_errors(), "any on a struct element must error");
        // An undefined predicate errors (and returns Ty::Error, not a valid bool node).
        let (_r, undef) = check("fn main() -> i32 {\n  if [1, 2, 3].any(nope) { return 1 }\n  return 0\n}\n");
        assert!(undef.has_errors(), "any with an undefined predicate must error");
    }

    #[test]
    fn count_checks_on_scalar_and_struct_arrays() {
        // count returns i64 and needs no scalar element (a struct element is fine).
        let (_p, d) = check("fn big(x: i64) -> bool = x > 2\nE { active: bool }\nfn main() -> i32 {\n  a := [1, 2, 3].where(big).count()\n  b := [E{active: true}, E{active: false}].where(.active).count()\n  if a + b == 3 { return 1 }\n  return 0\n}\n");
        assert!(!d.has_errors(), "count should check on scalar and struct array pipelines");
    }

    #[test]
    fn field_projection_from_slice_source_rejected() {
        // A `slice<Struct>` parameter is constructible, but a `.field` projection needs a
        // slot-backed source (MIR `IndexField`); projecting from a `{ptr,len}` view would
        // miscompile, so reject it cleanly.
        let (_p, d) = check("P { pay: i32, active: bool }\nfn total(xs: slice<P>) -> i32 = xs.pay.sum()\nfn main() -> i32 { return 0 }\n");
        assert!(d.has_errors(), "field projection from a slice source must be rejected");
    }

    #[test]
    fn to_array_inside_arena_checks() {
        // MMv2 slice 3: `.to_array()` inside an arena yields an owned array (consumed here).
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  arena {\n    return [1, 2, 3].map(double).to_array().sum()\n  }\n}\n");
        assert!(!d.has_errors(), "to_array inside an arena should check");
    }

    #[test]
    fn to_array_outside_arena_now_allowed() {
        // MMv2 slice 4: `.to_array()` outside an arena is free-standing (heap + drop), so it
        // checks (the owned array is dropped at function exit).
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  return [1, 2, 3].map(double).to_array().sum()\n}\n");
        assert!(!d.has_errors(), "to_array outside an arena is now free-standing (heap + drop)");
    }

    #[test]
    fn to_array_owned_cannot_escape_arena() {
        // The owned array is arena-allocated (region Arena(k)); letting it escape as the arena
        // block's value (bound outside the arena) must be rejected.
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn main() -> i32 {\n  bad := arena {\n    [1, 2, 3].map(double).to_array()\n  }\n  return 0\n}\n");
        assert!(d.has_errors(), "an arena-allocated owned array must not escape its arena");
    }

    #[test]
    fn reduce_fold_type_mismatch_errors() {
        // fold that takes the wrong element type.
        let (_p, d) = check(
            "fn add(acc: i32, x: bool) -> i32 = acc\nfn main() -> i32 {\n  return [1, 2, 3].reduce(add, 0)\n}\n",
        );
        assert!(d.has_errors(), "a fold whose element param mismatches must error");
    }

    #[test]
    fn slice_param_pipeline_checks() {
        let (_p, d) = check(
            "fn total(xs: slice<i32>) -> i32 = xs.sum()\nfn main() -> i32 {\n  return total([1, 2, 3])\n}\n",
        );
        assert!(!d.has_errors(), "array → slice<i32> + sum over a slice should check");
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
