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
    /// An owned `string` payload (MMv2 slice 8a). Unlike the other scalars this is a **Move**
    /// type with a heap buffer, so an `Option<string>` / `Result<string, E>` that holds it owns
    /// that buffer: it is dropped (freed) when the aggregate local is dropped, and moved out on
    /// `?` / `else` unwrap. Lets a fallible function return an owned string
    /// (`fn f() -> Result<string, Error>`). Kept var-free (`Scalar: Copy`) — it carries no inner.
    String,
    /// An owned `array<T>` payload (MMv2 slice 8b), the owned-collection dual of [`Scalar::String`]
    /// — same `{ptr,len}` layout, Move, dropped/moved as a unit. Lets a fallible function return an
    /// owned array (`fn f() -> Result<array<i64>, Error>`). The element is a [`PrimScalar`] (not a
    /// full [`Scalar`]) so the variant stays non-recursive and `Copy`; owned arrays only ever hold
    /// primitive elements today (struct/dynamic-array elements are a later capability).
    DynArray(PrimScalar),
    /// An owned, dynamic-length array of structs (AoS), the struct dual of [`Scalar::DynArray`]
    /// (MMv2 slice 8d). Same `{ptr,len}` layout, Move, dropped/freed as a unit. Carries the struct
    /// id (non-recursive, so `Scalar` stays `Copy`). Produced by `json.decode<array<Struct>>`,
    /// whose decoded `str` fields are zero-copy views into the input — so unlike a scalar
    /// `array<T>`, a struct array is region-tied to that input and cannot escape it.
    DynStructArray(u32),
    /// A `str` view payload (`array<str>` / `slice<str>` element, `Option<str>` / `Result<str,E>`
    /// payload). A `{ptr,len}` borrow — **Copy, not Move** (no heap buffer of its own), but
    /// **region-tracked**: a composite carrying a `str` lives only as long as that `str`'s source
    /// (`tracks_region`), exactly the struct-with-`str`-field rule extended to scalars. Unlike
    /// `String`, it is never dropped (it borrows). A `box<str>` is rejected (a view is not boxable).
    Str,
    /// A sum-type payload (the enum's id) — a Copy tagged struct, like [`Scalar::Struct`]. Lets
    /// `Option`/`Result` carry an enum, notably `Result<T, MyError>` (4b). Non-recursive (just the
    /// id), so `Scalar` stays `Copy`.
    Enum(u32),
}

impl Scalar {
    /// Whether this payload scalar is an owned **Move** type (a heap buffer that the enclosing
    /// `Option`/`Result` owns and must drop / move out). Today: `string` (8a), `array<T>` (8b).
    pub fn is_move(self) -> bool {
        matches!(self, Scalar::String | Scalar::DynArray(_) | Scalar::DynStructArray(_))
    }
}

/// The element of an owned-`array<T>` payload ([`Scalar::DynArray`]). A primitive scalar only —
/// a deliberately small, `Copy`, **non-recursive** subset of [`Scalar`] so an `array` can sit
/// inside an `Option`/`Result` payload without making [`Scalar`]/[`Ty`] recursive (MMv2 slice 8b).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimScalar {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
}

/// A [`PrimScalar`] as a full [`Scalar`] (the array element type).
pub fn prim_to_scalar(p: PrimScalar) -> Scalar {
    match p {
        PrimScalar::Int(it) => Scalar::Int(it),
        PrimScalar::Float(ft) => Scalar::Float(ft),
        PrimScalar::Bool => Scalar::Bool,
        PrimScalar::Char => Scalar::Char,
    }
}

/// A [`Scalar`] as a [`PrimScalar`] if it is a primitive (the only valid `array` element today);
/// `None` for struct / string / array / unit / error elements.
pub fn scalar_to_prim(s: Scalar) -> Option<PrimScalar> {
    match s {
        Scalar::Int(it) => Some(PrimScalar::Int(it)),
        Scalar::Float(ft) => Some(PrimScalar::Float(ft)),
        Scalar::Bool => Some(PrimScalar::Bool),
        Scalar::Char => Some(PrimScalar::Char),
        _ => None,
    }
}

/// Memory layout of a struct array — a property of the array *type*, so AoS-vs-SoA is decided
/// once (at the type) and threaded into field-access lowering, not re-derived per use site
/// (`open-questions.md` Open "SoA layout"). Only [`Layout::Aos`] exists today; `Layout::Soa`
/// (column-oriented, `soa array<T>`) joins at M6. Keeping it in the type **now** means adding
/// `Soa` later turns every place that must handle the new layout into a compile error — the
/// layout decision can never be silently forgotten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Array-of-structs: elements are contiguous whole structs (`[... %Struct ...]`). The only
    /// layout today; field access GEPs `element, field`.
    Aos,
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
    /// An *owned*, dynamic-length array of structs, laid out like a slice
    /// (`{ Struct* ptr, i64 len }`) but Move and region-tracked — the dynamic struct dual of
    /// [`Ty::DynArray`] (MMv2 slice 8d). Produced by `json.decode<array<Struct>>`. Its `str`
    /// fields are zero-copy views into the decode input, so the array is region-tied to that
    /// input and dropped (buffer freed) at scope exit. Carries its [`Layout`] (AoS today; SoA at
    /// M6) — the memory layout is a property of the type, threaded into field-access lowering.
    DynStructArray(u32, Layout),
    /// `slice<T>` — a borrowed view `{ T* ptr, i64 len }` of scalar elements. Copy. M4.
    Slice(Scalar),
    /// `array<slice<T>>` — an *owned*, dynamic-length array whose elements are `slice<T>` views
    /// (each `{ T* ptr, i64 len }`). Laid out like a slice (`{ slice* ptr, i64 count }`), Move
    /// (owns the buffer of slice headers, freed at scope exit), and region-tracked (the element
    /// slices borrow a source array, so the whole thing cannot outlive it). Produced by
    /// `chunks(n)` — the unit of chunk parallelism (`draft.md` §11). `T` is a primitive scalar.
    DynSliceArray(PrimScalar),
    /// `array<T>` — an *owned*, dynamic-length array of scalars, laid out like a slice
    /// (`{ T* ptr, i64 len }`) but Move and region-tracked. MMv2 slice 3: produced by a
    /// materializing terminal (`.to_array()`) and (this slice) arena-bump-allocated.
    DynArray(Scalar),
    /// `str` — an immutable string view `{ u8* ptr, i64 len }`. Copy. M5.
    Str,
    /// `string` — an *owned* string `{ u8* ptr, i64 len }`, laid out like `str` but Move and
    /// region-tracked (MMv2 slice 7). Produced by `str.clone()`; free-standing values own a
    /// heap buffer freed by `Drop` (the same machinery as owned `array<T>`). A `string` is
    /// readable as a `str` (a borrow of itself).
    String,
    /// An arena handle (internal; produced by `arena {}`, never written by the user).
    ArenaHandle,
    /// `builder` — an append-oriented string writer (draft.md §12), the canonical way to
    /// construct a `string` (over `a + b` concat). An opaque owned handle to a heap builder
    /// object (a Move type): `builder()` opens it, `.write(...)` appends, `.to_string()` consumes
    /// it into an owned `string`. An unfinished builder is `Drop`-freed at scope exit (MMv2 7c).
    Builder,
    /// A struct type; the id indexes `Program::structs`.
    Struct(u32),
    /// An anonymous tuple type `(T, U, ...)`; the id indexes `Program::tuples`. PR1 elements
    /// are primitive scalars (Copy, `Static`) — a tuple is Copy and never dropped/region-tied
    /// yet; owned/`str` elements are a later, additive slice.
    Tuple(u32),
    /// A first-class function value type (`fn(params) -> ret`), indexed into `Program.fn_types`.
    /// A function pointer — Copy, `Static`, no environment (non-capturing functions, slice ①).
    Fn(u32),
    /// A sum type, indexed into `Program.enums`. S1a: tag-only variants — a Copy/`Static` value
    /// represented as the variant tag (`i32`); constructed `Type.Variant`, consumed by `match`.
    Enum(u32),
    /// `Task<R>` — a handle to a spawned task's result (`task_group`, slice ④). The payload is a
    /// scalar. ④a represents it identically to `R` (eager execution); ④b makes it a real future.
    Task(Scalar),
    Unit,
    /// Type-checking error sentinel (bottom). Distinct from the `Error` *type*
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
        Ty::String => Some(Scalar::String),
        // An owned `array<T>` is a payload only when its element is primitive (slice 8b).
        Ty::DynArray(elem) => scalar_to_prim(elem).map(Scalar::DynArray),
        // Only an AoS array is payload-able today; an SoA array as an Option/Result payload is a
        // later concern (so `Scalar::DynStructArray` stays layout-free — always AoS).
        Ty::DynStructArray(id, Layout::Aos) => Some(Scalar::DynStructArray(id)),
        Ty::Str => Some(Scalar::Str),
        // A sum type is a Copy value (a tagged struct of Copy fields), so it can be an
        // Option/Result payload — notably `Result<T, MyError>` with a user error enum (4b).
        Ty::Enum(id) => Some(Scalar::Enum(id)),
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
        Scalar::String => Ty::String,
        Scalar::DynArray(elem) => Ty::DynArray(prim_to_scalar(elem)),
        Scalar::DynStructArray(id) => Ty::DynStructArray(id, Layout::Aos),
        Scalar::Str => Ty::Str,
        Scalar::Enum(id) => Ty::Enum(id),
    }
}

fn scalar_name(s: Scalar) -> String {
    ty_name(scalar_to_ty(s))
}

/// Whether an `Option`/`Result` type carries an owned (Move) payload that the aggregate owns
/// — so the aggregate is itself a Move type and its drop must free that payload (MMv2 slice 8a).
pub fn payload_is_move(ty: Ty) -> bool {
    match ty {
        Ty::Option(s) => s.is_move(),
        Ty::Result(o, e) => o.is_move() || e.is_move(),
        _ => false,
    }
}

/// Whether `ty` is a tuple with at least one owned (Move) element — i.e. a Move tuple. Needs the
/// tuple table to read the element scalars. (Such tuples are restricted to temporaries in this
/// cut — returned or destructured — so they never occupy a drop slot; see `check`/`check_fn`.)
fn ty_tuple_is_move(ty: Ty, tuples: &[hir::TupleDef]) -> bool {
    matches!(ty, Ty::Tuple(id) if tuples[id as usize].elems.iter().any(|s| s.is_move()))
}

/// Parse an explicit-overflow arithmetic method name into its op and overflow mode (`core.math`).
/// `None` mode = `wrapping_*` (the default wrapping arithmetic — lowered to a plain `Binary`);
/// `Some(_)` = `saturating_*` / `checked_*`. Returns `None` for any other method name.
fn parse_int_arith(method: &str) -> Option<(BinOp, Option<hir::ArithMode>)> {
    let (prefix, opname) = method.rsplit_once('_')?;
    let op = match opname {
        "add" => BinOp::Add,
        "sub" => BinOp::Sub,
        "mul" => BinOp::Mul,
        _ => return None,
    };
    let mode = match prefix {
        "wrapping" => None,
        "saturating" => Some(hir::ArithMode::Saturating),
        "checked" => Some(hir::ArithMode::Checked),
        _ => return None,
    };
    Some((op, mode))
}

/// Whether `ty` is a Move (owned) type — used to reject capturing an owned value into a lambda
/// (slice ③ supports copy-value captures only; an owned capture needs move/region handling).
fn ty_capture_is_move(ty: Ty, tuples: &[hir::TupleDef]) -> bool {
    // `Task<R>` (④b) is a box in the task_group region — Move, like `box<T>`.
    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder | Ty::Box(_) | Ty::Task(_))
        || payload_is_move(ty)
        || ty_tuple_is_move(ty, tuples)
}

/// The pipeline stages of a stage-bearing pipeline node (else `None`). Lets the flow analyses
/// (`MoveCheck`/`EscapeCheck`) walk stage captures — a lifted lambda's captured enclosing locals,
/// which are reads of enclosing locals and must be analyzed like any other use.
fn pipeline_stages(kind: &ExprKind) -> Option<&[Stage]> {
    match kind {
        ExprKind::ArraySum { stages, .. }
        | ExprKind::ArrayCount { stages, .. }
        | ExprKind::ArrayAnyAll { stages, .. }
        | ExprKind::ArrayMinMax { stages, .. }
        | ExprKind::ArrayReduce { stages, .. }
        | ExprKind::ArrayScan { stages, .. }
        | ExprKind::ArraySort { stages, .. }
        | ExprKind::ArraySortBy { stages, .. }
        | ExprKind::ArrayToArray { stages, .. }
        | ExprKind::ArrayPartition { stages, .. }
        | ExprKind::ArrayParMap { stages, .. } => Some(stages),
        _ => None,
    }
}

/// The capture operands carried by a pipeline's stages (a lifted lambda's captured values).
fn stage_capture_exprs(stages: &[Stage]) -> impl Iterator<Item = &Expr> {
    stages.iter().flat_map(|s| match &s.kind {
        StageKind::Map { captures, .. } | StageKind::Where { captures, .. } => captures.as_slice(),
        StageKind::Project { .. } | StageKind::WhereField { .. } => &[][..],
    })
}

/// The capture operands carried by a reducer/terminal node's own function (a lifted lambda's
/// captured values for `reduce`/`scan`/`partition`/`par_map`/`any`/`all`). The flow analyses walk
/// these like stage captures.
fn node_captures(kind: &ExprKind) -> &[Expr] {
    match kind {
        ExprKind::ArrayReduce { captures, .. }
        | ExprKind::ArrayScan { captures, .. }
        | ExprKind::ArrayPartition { captures, .. }
        | ExprKind::ArrayParMap { captures, .. }
        | ExprKind::ArraySortBy { captures, .. }
        | ExprKind::ArrayAnyAll { captures, .. } => captures,
        _ => &[],
    }
}

/// Whether a local of `ty` owns a heap buffer that must be freed by a per-binding `Drop` (when its
/// region is `Static`) — the predicate the drop set is built from. A free-standing owned
/// collection/string/builder, or an `Option`/`Result` carrying a Move payload.
fn is_owned_droppable(ty: Ty) -> bool {
    // `Task<R>` (④b) is a box in the task_group region — bulk-freed with the region, never an
    // individually-dropped owned value (like `box<T>`).
    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder) || payload_is_move(ty)
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

#[derive(Clone)]
struct FnSig {
    params: Vec<Ty>,
    /// `out[i]` — whether parameter `i` is an `out` (writable, no-alias) output buffer.
    out: Vec<bool>,
    ret: Ty,
}

/// A pipeline stage as collected from the AST (before type checking).
/// A stage's function argument: either a reference to a named top-level function, or an inline
/// lambda (which sema lifts to a synthetic top-level function — see [`Checker::lift_lambda`]).
enum StageFn {
    Named(ast::Ident),
    Lambda { params: Vec<ast::LambdaParam>, body: ast::Block, span: Span },
}

enum RawStage {
    Map(StageFn),
    Where(StageFn),
    WhereField(ast::Ident),
    Project(ast::Ident),
}

/// An assignable location resolved by [`Checker::check_place`].
enum Place {
    Local { id: LocalId, ty: Ty },
    Field { base: LocalId, index: u32, ty: Ty },
    /// `base[index] = value` — an element store into a `mut` array local or an `out` slice
    /// parameter. `index` is the checked (`i64`) subscript; `elem` is the element type.
    Index { base: LocalId, index: Expr, elem: Ty },
    Err,
}

/// The tag of the builtin `Error` enum's `Code(i32)` variant (the generic error-code category).
/// Must match the variant order registered in `check_file`.
pub const ERROR_VARIANT_CODE: u32 = 3;

/// Analyze a file into a typed program. Errors are pushed to `diags`.
pub fn check_file(file: &ast::File, diags: &mut Diagnostics) -> Program {
    // Pass 0a: assign an id to every struct name (so field/sig types can refer to them
    // regardless of order).
    let mut struct_ids: HashMap<String, u32> = HashMap::new();
    let mut struct_decls: Vec<&ast::StructDecl> = Vec::new();
    let mut enum_ids: HashMap<String, u32> = HashMap::new();
    let mut enum_decls: Vec<&ast::EnumDecl> = Vec::new();
    for item in &file.items {
        match item {
            ast::Item::Struct(s) => {
                if s.name.name == "Error" {
                    diags.error("'Error' is a reserved type name (the builtin error sum type)".to_string(), s.span);
                }
                if struct_ids.insert(s.name.name.clone(), struct_decls.len() as u32).is_some()
                    || enum_ids.contains_key(&s.name.name)
                {
                    diags.error(format!("duplicate type declaration: '{}'", s.name.name), s.span);
                }
                struct_decls.push(s);
            }
            ast::Item::Enum(e) => {
                if e.name.name == "Error" {
                    diags.error("'Error' is a reserved type name (the builtin error sum type)".to_string(), e.span);
                }
                if enum_ids.insert(e.name.name.clone(), enum_decls.len() as u32).is_some()
                    || struct_ids.contains_key(&e.name.name)
                {
                    diags.error(format!("duplicate type declaration: '{}'", e.name.name), e.span);
                }
                enum_decls.push(e);
            }
            ast::Item::Fn(_) => {}
        }
    }

    // The shared tuple-type interner (anonymous `(T, U, …)`), built on demand as types resolve.
    let mut tuples: Vec<hir::TupleDef> = Vec::new();
    // The shared function-value-type interner (`Ty::Fn`), built on demand as lambdas become values.
    let mut fn_types: Vec<hir::FnTy> = Vec::new();

    // Pass 0b: resolve struct field types (before enum payloads, which may be structs).
    let mut structs: Vec<StructDef> = Vec::with_capacity(struct_decls.len());
    for s in &struct_decls {
        let mut fields = Vec::with_capacity(s.fields.len());
        for f in &s.fields {
            let ty = resolve_type(&f.ty, &struct_ids, &enum_ids, &mut tuples, &mut fn_types, diags);
            // Fields are **Copy** only: a primitive scalar (int/float/bool/char) or `str`. A
            // `str` field may hold an arena-backed str: the struct *carries* that field's region
            // (MMv2 slice 2), so `EscapeCheck` lets it live inside the arena and only rejects the
            // whole struct escaping; a scalar/literal-only struct stays `Static` (returnable).
            //
            // Owned (Move) fields — `string` / `array<T>` / `box` / `builder` / a `Move` tuple —
            // are rejected: a struct is treated as Copy (it has no per-binding `Drop` and is copied
            // by value on assignment / `arr[i]` indexing), so a Move field would let two struct
            // copies free the same buffer (double-free). `ty_to_scalar` is NOT a sufficient test
            // here — it returns `Some` for `string`/`array<T>` (they have a scalar repr) — so match
            // the Copy field types explicitly. (Nested struct / slice / option / result fields the
            // layout can't hold yet are likewise excluded.)
            let is_field_ok = matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str | Ty::Error);
            if !is_field_ok {
                diags.error(
                    format!("struct fields must be a primitive scalar or str for now, got {}", ty_name(ty)),
                    f.span,
                );
            }
            fields.push(FieldDef { name: f.name.name.clone(), ty });
        }
        // `align`: natural alignment today — the `align(N)` surface syntax + custom value
        // arrive at M6 (`open-questions.md`); the field is reserved so that is an additive
        // change at the alignment seam, not a retrofit.
        structs.push(StructDef { name: s.name.name.clone(), fields, align: None });
    }

    // Pass 0c: resolve enum variant payloads (all type names + structs are known). The enum lowers
    // to a non-union struct `{ i32 tag, <every variant's payload flattened> }`, so each variant's
    // `field_base` is `1 + (payload slots of earlier variants)`. Payloads are primitive scalars
    // (S1b) or a **plain-data struct** with no `str`/region-tracked field (S2) — a region-tied
    // struct payload would need enum region-tracking (deferred).
    let mut enums: Vec<hir::EnumDef> = Vec::with_capacity(enum_decls.len());
    for e in &enum_decls {
        let mut seen = std::collections::HashSet::new();
        let mut variants = Vec::with_capacity(e.variants.len());
        let mut field_base = 1u32;
        for v in &e.variants {
            if !seen.insert(v.name.name.clone()) {
                diags.error(format!("duplicate variant '{}' in '{}'", v.name.name, e.name.name), v.span);
            }
            let mut payload = Vec::with_capacity(v.payload.len());
            for t in &v.payload {
                let ty = resolve_type(t, &struct_ids, &enum_ids, &mut tuples, &mut fn_types, diags);
                match ty {
                    Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char => {
                        payload.push(ty_to_scalar(ty).expect("primitive scalar"));
                    }
                    Ty::Struct(id) if structs[id as usize].fields.iter().all(|f| f.ty != Ty::Str) => {
                        payload.push(Scalar::Struct(id));
                    }
                    Ty::Struct(_) => diags.error(
                        "a sum-type payload struct may not contain a `str` field yet (region tracking pending)".to_string(),
                        t.span(),
                    ),
                    Ty::Error => {}
                    other => diags.error(
                        format!("variant payloads must be a primitive scalar or plain struct for now, got {}", ty_name(other)),
                        t.span(),
                    ),
                }
            }
            let n = payload.len() as u32;
            variants.push(hir::EnumVariant { name: v.name.name.clone(), payload, field_base });
            field_base += n;
        }
        enums.push(hir::EnumDef { name: e.name.name.clone(), variants });
    }

    // The canonical builtin `Error` sum type (4b-2): universal categories + a generic `Code(i32)`
    // (the variant order must match `ERROR_VARIANT_CODE`). Registered after the user enums; `Error`
    // is a reserved type name (rejected in pass 0a). Every fallible builtin and `Result<_, Error>`
    // use this enum.
    let error_enum_id = enums.len() as u32;
    enum_ids.insert("Error".to_string(), error_enum_id);
    enums.push(hir::EnumDef {
        name: "Error".to_string(),
        variants: vec![
            hir::EnumVariant { name: "NotFound".to_string(), payload: Vec::new(), field_base: 1 },
            hir::EnumVariant { name: "Invalid".to_string(), payload: Vec::new(), field_base: 1 },
            hir::EnumVariant { name: "Denied".to_string(), payload: Vec::new(), field_base: 1 },
            hir::EnumVariant {
                name: "Code".to_string(),
                payload: vec![Scalar::Int(IntTy { bits: 32, signed: true })],
                field_base: 1,
            },
        ],
    });

    // Pass 1: collect function signatures so calls can resolve regardless of order.
    let mut sigs: HashMap<String, FnSig> = HashMap::new();
    for item in &file.items {
        let ast::Item::Fn(f) = item else { continue };
        let mut params: Vec<Ty> = Vec::with_capacity(f.params.len());
        for p in &f.params {
            params.push(resolve_type(&p.ty, &struct_ids, &enum_ids, &mut tuples, &mut fn_types, diags));
        }
        // A box across a call boundary would escape its arena, so M3 forbids box
        // parameters and returns (boxes are arena-local). This also closes escape
        // holes via call results.
        for (p, ty) in f.params.iter().zip(&params) {
            if matches!(ty, Ty::Box(_)) {
                diags.error(
                    "a box cannot be a function parameter (boxes are arena-local in M3)".to_string(),
                    p.ty.span(),
                );
            }
        }
        let ret = match &f.ret {
            Some(t) => {
                let r = resolve_type(t, &struct_ids, &enum_ids, &mut tuples, &mut fn_types, diags);
                if matches!(r, Ty::Box(_)) {
                    diags.error(
                        "a box cannot be a function return type (it would escape its arena)".to_string(),
                        t.span(),
                    );
                }
                // A returned function value would carry a frame-local closure environment out of
                // the frame (use-after-free); deferred until closures can own a region-backed env.
                if matches!(r, Ty::Fn(_)) {
                    diags.error(
                        "returning a function value is not supported yet (a closure's environment is frame-local)".to_string(),
                        t.span(),
                    );
                }
                r
            }
            None => Ty::Unit,
        };
        let out = f.params.iter().map(|p| p.is_out).collect();
        sigs.insert(f.name.name.clone(), FnSig { params, out, ret });
    }

    // Pass 2: check each function body. A function's inline lambdas are lifted to synthetic
    // top-level functions (`cx.lifted`) and appended to the program, so all later passes treat
    // them like ordinary named functions.
    let mut fns: Vec<hir::Fn> = Vec::new();
    for item in &file.items {
        let ast::Item::Fn(f) = item else { continue };
        let mut cx = Checker {
            diags,
            sigs: &sigs,
            struct_ids: &struct_ids,
            enum_ids: &enum_ids,
            enums: &enums,
            error_enum_id,
            structs: &structs,
            tuples: &mut tuples,
            fn_types: &mut fn_types,
            int_vars: Vec::new(),
            int_parent: Vec::new(),
            float_vars: Vec::new(),
            float_parent: Vec::new(),
            locals: Vec::new(),
            scope: Vec::new(),
            ret_hint: Ty::Unit,
            arena_depth: 0,
            task_group_depth: 0,
            wait_state: Vec::new(),
            task_group_fallible: Vec::new(),
            slice_bases: std::collections::HashMap::new(),
            cur_fn: String::new(),
            lifted: Vec::new(),
            capture: None,
        };
        let checked = cx.check_fn(f);
        let lifted = std::mem::take(&mut cx.lifted);
        fns.push(checked);
        fns.extend(lifted);
    }
    let mut program = Program { fns, structs, enums, tuples, fn_types };
    // Pass 3 (partial): move / use-after-move checking + arena escape checking
    // (`03-types.md` §6–§7), then derive the per-function drop set (MMv2 slice 4).
    // Destructure so the flow analyses can read `tuples` (a tuple may be region-tracked when it
    // holds a `str` element) while iterating `&mut fns`.
    let Program { fns, tuples, .. } = &mut program;
    let tuples: &[hir::TupleDef] = tuples;
    for f in fns.iter_mut() {
        MoveCheck { f, diags, tuples }.check();
        let region = {
            let mut ec = EscapeCheck {
                f,
                diags,
                tuples,
                region: std::collections::HashMap::new(),
                decl_depth: std::collections::HashMap::new(),
                local_backed_slice: std::collections::HashSet::new(),
            };
            ec.check();
            ec.region
        };
        // Every free-standing owned `array<T>` (region `Static`) is dropped at every function
        // exit. Arena-allocated ones (region `Arena(k)`) are bulk-freed by the arena, so they
        // are excluded. A moved-out local stays in this set, but MIR nulls its slot at the move
        // site (null-on-move drop flag), so its exit `Drop` is a no-op `free(null)` — no
        // double-free, and the path where it is *not* moved is still freed (no leak).
        let drops: Vec<LocalId> = f
            .locals
            .iter()
            .filter(|l| is_owned_droppable(l.ty) || ty_tuple_is_move(l.ty, tuples))
            .map(|l| l.id)
            .filter(|id| region.get(id).copied().unwrap_or(Region::Static) == Region::Static)
            .collect();
        f.drop_locals = drops;
    }
    // Pass 4: effect/purity inference + the `par_map` Pure requirement (`draft.md` §11).
    check_parallelism(&program, diags);
    program
}

/// Effect/purity inference + the rule that a `par_map` function must be **Pure** (`draft.md` §11,
/// a Settled decision). A function is **Impure** iff it (transitively) performs an observable
/// side effect — calling `print` / `io.stdout.write` / `fs.read_file`, or calling an Impure
/// function. Everything else (arithmetic, field/array reads, builder/arena/heap use, owned-value
/// moves) is Pure. A `par_map(f)` whose `f` is Impure is rejected. (`f` is `(T) -> R` with no `out`
/// parameter, so reaching a side effect is the only way it can be Impure — sound for the language
/// as it stands.)
fn check_parallelism(program: &Program, diags: &mut Diagnostics) {
    use std::collections::HashMap;
    // Per function: directly observable effect + the set of functions it calls (incl. pipeline
    // stage/reducer functions) + the `par_map` callees to verify.
    let mut direct: HashMap<&str, bool> = HashMap::new();
    let mut calls: HashMap<&str, Vec<String>> = HashMap::new();
    let mut parmaps: Vec<(String, Span)> = Vec::new();
    for f in &program.fns {
        let mut scan = EffectScan { impure_direct: false, calls: Vec::new(), parmaps: Vec::new() };
        scan.block(&f.body);
        direct.insert(f.name.as_str(), scan.impure_direct);
        calls.insert(f.name.as_str(), scan.calls);
        parmaps.extend(scan.parmaps);
    }
    // Fixpoint: a function is impure if it has a direct effect or calls an impure function.
    let mut impure: std::collections::HashSet<String> =
        direct.iter().filter(|(_, d)| **d).map(|(n, _)| n.to_string()).collect();
    loop {
        let mut changed = false;
        for f in &program.fns {
            if impure.contains(&f.name) {
                continue;
            }
            if calls[f.name.as_str()].iter().any(|c| impure.contains(c)) {
                impure.insert(f.name.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // The `par_map` function must be Pure.
    for (func, span) in parmaps {
        if impure.contains(&func) {
            diags.error(
                format!("'par_map' requires a Pure function, but '{func}' has a side effect (it reads/writes I/O); use `reduce` for an accumulation"),
                span,
            );
        }
    }
}

/// Walks a function body to collect its directly-observable effect, the functions it calls (incl.
/// pipeline stage/reducer functions), and any `par_map` callees. The match is exhaustive, so no
/// call edge or effect node can be silently missed.
struct EffectScan {
    impure_direct: bool,
    calls: Vec<String>,
    parmaps: Vec<(String, Span)>,
}

impl EffectScan {
    fn stage_funcs(&mut self, stages: &[Stage]) {
        for s in stages {
            match &s.kind {
                StageKind::Map { func, captures } | StageKind::Where { func, captures } => {
                    self.calls.push(func.clone());
                    // Capture operands are reads of enclosing locals — walk them so no call edge /
                    // effect they might contain is missed (exhaustiveness).
                    for c in captures {
                        self.expr(c);
                    }
                }
                StageKind::Project { .. } | StageKind::WhereField { .. } => {}
            }
        }
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            match s {
                Stmt::Let { init, .. } | Stmt::Assign { value: init, .. } | Stmt::AssignField { value: init, .. } | Stmt::LetTuple { init, .. } => self.expr(init),
                Stmt::AssignIndex { index, value, .. } => {
                    self.expr(index);
                    self.expr(value);
                }
                Stmt::Return(Some(e)) | Stmt::Expr(e) => self.expr(e),
                Stmt::Return(None) => {}
            }
        }
        if let Some(v) = &b.value {
            self.expr(v);
        }
    }

    fn expr(&mut self, e: &Expr) {
        // A reducer node may carry capture operands (a lifted lambda's captured enclosing locals);
        // walk them so no call edge / effect they contain is missed. (Stage captures are walked by
        // `stage_funcs`.)
        for c in node_captures(&e.kind) {
            self.expr(c);
        }
        match &e.kind {
            // Observable side effects.
            ExprKind::Call { func, args } => {
                if func == "print" {
                    self.impure_direct = true;
                } else {
                    self.calls.push(func.clone());
                }
                for a in args {
                    self.expr(a);
                }
            }
            // A function value (taking a fn's address) is not a call; an indirect call walks its
            // callee + args (the target is not statically known — not used in `par_map` contexts).
            ExprKind::FnValue(_) => {}
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.expr(c);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::IoStdoutWrite { arg } => {
                self.impure_direct = true;
                self.expr(arg);
            }
            ExprKind::IoStdoutWriteBuilder { builder } => {
                self.impure_direct = true;
                self.expr(builder);
            }
            ExprKind::FsReadFile { path } => {
                self.impure_direct = true;
                self.expr(path);
            }
            // Pipeline nodes carry a `source` (+ a stage/reducer function that is a call).
            ExprKind::ArraySum { source, stages } | ExprKind::ArrayCount { source, stages } => {
                self.stage_funcs(stages);
                self.expr(source);
            }
            ExprKind::ArrayMinMax { source, stages, .. } | ExprKind::ArraySort { source, stages, .. }
            | ExprKind::ArrayToArray { source, stages, .. } => {
                self.stage_funcs(stages);
                self.expr(source);
            }
            ExprKind::ArrayAnyAll { source, stages, func, .. }
            | ExprKind::ArrayReduce { source, stages, func, .. }
            | ExprKind::ArrayScan { source, stages, func, .. }
            | ExprKind::ArraySortBy { source, stages, key_func: func, .. }
            | ExprKind::ArrayPartition { source, stages, func, .. } => {
                self.stage_funcs(stages);
                self.calls.push(func.clone());
                self.expr(source);
            }
            ExprKind::ArrayParMap { source, stages, func, .. } => {
                self.stage_funcs(stages);
                self.calls.push(func.clone());
                self.parmaps.push((func.clone(), e.span));
                self.expr(source);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.expr(source);
                self.expr(n);
            }
            // Structural recursion (no effect of their own).
            ExprKind::Unary { expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::If { cond, then, els } => {
                self.expr(cond);
                self.block(then);
                self.block(els);
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.expr(f);
                }
            }
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.expr(el);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.expr(o);
                }
            }
            ExprKind::ArrayLit { elems, .. } => {
                for el in elems {
                    self.expr(el);
                }
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.expr(opt);
                self.expr(fallback);
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.expr(builder);
                self.expr(arg);
            }
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) => self.block(b),
            // Spawning / joining concurrent work is an observable effect (the enclosing function
            // is not pure); the spawned closure's own effects live in its lifted function.
            ExprKind::Spawn { closure, .. } => {
                self.impure_direct = true;
                self.expr(closure);
            }
            ExprKind::TaskGet(inner) => self.expr(inner),
            ExprKind::Wait => self.impure_direct = true,
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.expr(p);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for a in arms {
                    self.expr(&a.body);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.expr(result);
                self.expr(f);
            }
            ExprKind::TupleIndex { recv, .. } => self.expr(recv),
            ExprKind::Index { recv, index } => {
                self.expr(recv);
                self.expr(index);
            }
            ExprKind::ElemField { recv, index, .. } => {
                self.expr(recv);
                self.expr(index);
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i) | ExprKind::Try(i)
            | ExprKind::HeapNew(i) | ExprKind::BoxGet(i) | ExprKind::BoxClone(i) | ExprKind::StrClone(i)
            | ExprKind::StrBorrow(i) | ExprKind::BuilderToString(i) | ExprKind::Len(i)
            | ExprKind::ArrayToSlice(i) => self.expr(i),
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        self.expr(h);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. }
            | ExprKind::JsonDecodeStructArray { input, .. } => self.expr(input),
            // Leaves.
            ExprKind::Unit | ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Char(_)
            | ExprKind::Str(_) | ExprKind::Bool(_) | ExprKind::Local(_) | ExprKind::OptionNone
            | ExprKind::Field { .. } | ExprKind::IndexField { .. } | ExprKind::BuilderNew => {}
        }
    }
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
    /// Tuple defs (to decide whether a `Ty::Tuple` is region-tracked — true iff an element is).
    tuples: &'a [hir::TupleDef],
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
        // bodies and fall-through blocks), so apply the same escape check there.
        if let Some(v) = &self.f.body.value {
            self.check_return_escape(v, 0);
        }
    }

    /// Escape check for a returned value `e` (an explicit `return` or a body's trailing value):
    /// a region-tracked value must be `Static` (returnable), and a `slice` must not view a local
    /// array. The region-tracked diagnostic distinguishes a `Frame` borrow of local storage (use
    /// `.clone()`) from an arena allocation.
    fn check_return_escape(&mut self, e: &Expr, depth: u32) {
        let r = self.region_of(e, depth);
        if self.tracks_region(e.ty) && !r.outlives(Region::Static) {
            let msg = if r == Region::Frame {
                "cannot return a view that borrows local storage (it is freed when the function returns); use `.clone()` to return an owned value"
            } else {
                "cannot return a value allocated in an arena (it is freed at block end)"
            };
            self.diags.error(msg.to_string(), e.span);
        }
        if matches!(e.ty, Ty::Slice(_)) && self.slice_is_local(e) {
            self.diags.error(
                "cannot return a slice that views a local array (it is freed when the function returns)".to_string(),
                e.span,
            );
        }
    }

    /// Types whose values carry an inferred region and so must be escape-checked: `box<T>`
    /// (M3), arena-backed `str` (M5 — `template`/concat allocate in the arena), and a struct
    /// (MMv2 slice 2 — a struct's region is the max of its fields, so a struct holding an
    /// arena-backed `str` field carries that arena region). A scalar-only struct is `Static`.
    fn tracks_region(&self, ty: Ty) -> bool {
        match ty {
            Ty::Box(_) | Ty::Str | Ty::String | Ty::Struct(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) => true,
            // A tuple is region-tracked iff any element is (today: a `str` element — a view tied to
            // its source). A tuple of plain scalars is Copy / `Static`, freely returnable.
            Ty::Tuple(id) => self.tuples[id as usize].elems.iter().any(|s| self.tracks_region(scalar_to_ty(*s))),
            // A *fixed* `array<T>` (a stack value) is region-tracked iff its element is — an
            // `array<str>` holds `str` views (so an array of arena strs is arena-regioned and must
            // not escape), while an `array<i64>` is plain Copy data (Static, freely returnable).
            // A `slice<T>` likewise tracks iff its element does (its own backing is handled
            // separately by the local-backed-slice check). A fixed `array<Struct>` (AoS) always
            // tracks, like `Struct` itself — a struct may hold a region-tracked `str` field, so an
            // element / element-field read must inherit the array's region.
            Ty::Array(s, _) | Ty::Slice(s) => self.tracks_region(scalar_to_ty(s)),
            Ty::StructArray(..) => true,
            // An `Option`/`Result` is region-tracked iff its payload is. A `Struct` payload (e.g. a
            // `json.decode`-d struct) and now a `str` payload (a view) both track; scalars do not.
            Ty::Option(s) => self.tracks_region(scalar_to_ty(s)),
            Ty::Result(o, e) => self.tracks_region(scalar_to_ty(o)) || self.tracks_region(scalar_to_ty(e)),
            // `Task<R>` (④b) is a box in the task_group region — region-tracked like `box<T>`, so
            // a task handle cannot escape its `task_group` scope.
            Ty::Task(_) => true,
            _ => false,
        }
    }

    /// The [`Region`] a region-bearing (`box`/`str`) value is bound to. `Static` = no region
    /// (a leaked/static str, a box param — none exist — etc.). Recurses through value forms so
    /// it can't slip out via an `if`/block value.
    fn region_of(&self, e: &Expr, depth: u32) -> Region {
        match &e.kind {
            // Allocating producers are bound to the enclosing arena (Static outside any arena,
            // where the result is leaked / process-lifetime and safe to return).
            ExprKind::HeapNew(_) | ExprKind::BoxClone(_) | ExprKind::Template(_) => Region::arena(depth),
            // A spawned task's handle is a box in the enclosing `task_group` region.
            ExprKind::Spawn { .. } => Region::arena(depth),
            // `.to_array()` bump-allocates the owned array in the enclosing arena. `reduce` folds
            // its accumulator there too — when that accumulator is region-tracked (a `str` built by
            // concatenation, a struct), the result lives in the enclosing arena and must not escape
            // it. `arena(depth)` is the shortest-lived (most restrictive) region anything allocated
            // at this depth can have, so it conservatively covers an accumulator that instead just
            // forwards `init` or borrows a source element (both outlive `arena(depth)`).
            ExprKind::ArrayToArray { .. }
            | ExprKind::ArrayPartition { .. }
            | ExprKind::ArrayParMap { .. }
            | ExprKind::ArrayScan { .. }
            | ExprKind::ArraySort { .. }
            | ExprKind::ArraySortBy { .. }
            | ExprKind::ArrayReduce { .. } => Region::arena(depth),
            // `str + str` concatenation is also built in the enclosing arena.
            ExprKind::Binary { op: BinOp::Add, .. } if e.ty == Ty::Str => Region::arena(depth),
            // A decoded struct's `str`/array fields are zero-copy views into the input buffer
            // (MMv2 slice 6), so the struct is region-tied to that input — it cannot outlive it.
            // Conservative: even a scalar-only decoded struct is bound to the input region (no
            // struct-field lookup here); use `.clone()` to escape. `?` preserves the region.
            ExprKind::JsonDecode { input, .. } => self.region_of(input, depth),
            // A decoded `array<Struct>` (slice 8d) likewise carries the input's region — its
            // elements' `str` fields are zero-copy views into the input; `.clone()` to escape.
            ExprKind::JsonDecodeStructArray { input, .. } => self.region_of(input, depth),
            // `arr[i].field` reads a field of a struct-array element; a `str` field is a view into
            // the array's storage, so it inherits the array's region (it must not outlive it). A
            // scalar field is Copy → the default `Static` (handled below), but tying to the array
            // is conservatively correct for both.
            ExprKind::ElemField { recv, .. } => self.region_of(recv, depth),
            // `arr[i]` reads an element; a `str` element is a view into the array's storage, so it
            // inherits the array's region (it must not outlive it). A scalar element is Copy and
            // not region-tracked, so inheriting the array's region is harmless (never checked).
            ExprKind::Index { recv, .. } => self.region_of(recv, depth),
            // An array literal lives as long as its shortest-lived element — a `[str]` of arena
            // `str` views is arena-regioned (the same rule as a struct literal over its fields).
            ExprKind::ArrayLit { elems, .. } => elems
                .iter()
                .fold(Region::Static, |acc, el| acc.shorter(self.region_of(el, depth))),
            // A tuple lives as long as its shortest-lived element (same rule as an array literal):
            // a tuple holding an arena `str` view is arena-regioned and cannot escape.
            ExprKind::Tuple { elems, .. } => elems
                .iter()
                .fold(Region::Static, |acc, el| acc.shorter(self.region_of(el, depth))),
            // `t.N` reads an element; a `str` element is a view into the tuple, so it inherits the
            // tuple's region (a scalar element is Copy → harmless to inherit, never checked).
            ExprKind::TupleIndex { recv, .. } => self.region_of(recv, depth),
            // `chunks` makes an array of slices that borrow the source array — region-tied to it.
            ExprKind::ArrayChunks { source, .. } => self.region_of(source, depth),
            // Borrowing an array as a slice preserves the array's region — a `slice<str>` coerced
            // from an arena str-array must not outlive that arena.
            ExprKind::ArrayToSlice(inner) => self.region_of(inner, depth),
            // Wrapping/unwrapping preserves the payload's region: `Ok(decoded)` is as short-lived
            // as `decoded`, and `res?` re-exposes whatever region `res` carried. Without this a
            // region-tied struct could escape through a `Result`-typed local (use-after-free).
            ExprKind::Try(inner)
            | ExprKind::OptionSome(inner)
            | ExprKind::ResultOk(inner)
            | ExprKind::ResultErr(inner) => self.region_of(inner, depth),
            // `map_err` passes the `Ok` payload through unchanged, so its region is the source's
            // (a region-tied Ok payload must not escape via the converted result).
            ExprKind::ResultMapErr { result, .. } => self.region_of(result, depth),
            // `opt else fb` yields one of two values, so it lives only as long as the shorter.
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.region_of(opt, depth).shorter(self.region_of(fallback, depth))
            }
            // A `str` borrow of an owned `string` (slice 7b) views storage owned by *this* frame
            // (the `string` is `Drop`-freed at frame exit), so the view is `Frame`-regioned — it
            // must not escape the frame. This feeds `region_of(Call)`: passing a borrowed string
            // to a function that returns a borrow of its argument correctly blocks the escape.
            // We cap at the shorter of `Frame` and the borrowed value's own region: today every
            // `string` is heap-owned (`Static`), so this is exactly `Frame`; but if a later slice
            // arena-allocates a `string` (`Arena(k)`, shorter than `Frame`), the borrow must not
            // outlive that arena — taking the shorter keeps it sound for free.
            ExprKind::StrBorrow(inner) => Region::Frame.shorter(self.region_of(inner, depth)),
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
            // A call's result may be a view borrowing one of its arguments (`fn id(s: str) -> str
            // = s`), so conservatively it lives no longer than the shortest-lived argument — the
            // region analogue of `slice_is_local`'s arg propagation. Without this, returning
            // `f(arena_str)` out of the arena slips the escape check → use-after-free of the
            // freed buffer. A function that does *not* return a borrow of its args is
            // over-restricted here; precise per-fn "returns a borrow of arg i" inference is a
            // later slice. Non-tracked args (ints, literals) are `Static` and don't shorten.
            ExprKind::Call { args, .. } => args
                .iter()
                .fold(Region::Static, |acc, a| acc.shorter(self.region_of(a, depth))),
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
                if self.tracks_region(init.ty) {
                    let r = self.region_of(init, depth);
                    self.region.insert(*local, r);
                }
                if matches!(init.ty, Ty::Slice(_)) && self.slice_is_local(init) {
                    self.local_backed_slice.insert(*local);
                }
            }
            // `base[index] = value` — primitive element store (first cut), so no region to track;
            // just recurse into the index and value for nested escapes.
            Stmt::AssignIndex { index, value, .. } => {
                self.walk(index, depth);
                self.walk(value, depth);
            }
            Stmt::Assign { local, value } => {
                self.walk(value, depth);
                // Conservative without a dataflow join: a binding that is *ever* assigned a
                // local-backed slice stays local-backed (a later branch could reach `return`
                // while the binding still holds the local array). We never clear the flag.
                if matches!(value.ty, Ty::Slice(_)) && self.slice_is_local(value) {
                    self.local_backed_slice.insert(*local);
                }
                if self.tracks_region(value.ty) {
                    let r = self.region_of(value, depth);
                    // The binding's scope: at least the frame (a depth-0 binding lives the whole
                    // frame, region `Frame`), or the enclosing arena if declared inside one. Using
                    // `Frame` rather than `Static` here lets a `Frame`-region borrow (a `str` view
                    // of a local `string`, slice 7e) be held by a frame binding — escape past the
                    // frame is still caught by the return / struct-field-store checks. A deeper
                    // arena value assigned to a shallower binding stays rejected.
                    let target = Region::Frame.shorter(Region::arena(*self.decl_depth.get(local).unwrap_or(&0)));
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
                if self.tracks_region(value.ty) {
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
                self.check_return_escape(e, depth);
            }
            Stmt::Return(None) => {}
            Stmt::Expr(e) => self.walk(e, depth),
            // A tuple destructure binds each element to a local. If the tuple is region-tracked
            // (holds a `str` view, or owned arrays allocated in an arena), each bound local inherits
            // the tuple's region — else an arena-allocated destructured array would default to
            // `Static`, land in the drop set, and be freed both here and by the arena (double-free).
            // (The current producers — `partition`, owned-tuple returns — give all elements the same
            // region, so the tuple's region is exact; per-element regions are a later refinement.)
            Stmt::LetTuple { locals, init, .. } => {
                self.walk(init, depth);
                if self.tracks_region(init.ty) {
                    let r = self.region_of(init, depth);
                    for l in locals.iter().flatten() {
                        self.decl_depth.insert(*l, depth);
                        self.region.insert(*l, r);
                    }
                }
            }
        }
    }

    /// Recurse to find nested arenas and value positions that let a box escape.
    fn walk(&mut self, e: &Expr, depth: u32) {
        // A pipeline stage or reducer may carry capture operands (a lifted lambda's captured
        // enclosing locals); walk them so a captured value escaping its region is caught.
        if let Some(stages) = pipeline_stages(&e.kind) {
            for c in stage_capture_exprs(stages) {
                self.walk(c, depth);
            }
        }
        for c in node_captures(&e.kind) {
            self.walk(c, depth);
        }
        match &e.kind {
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.walk(el, depth);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.walk(o, depth);
                }
            }
            ExprKind::TupleIndex { recv, .. } => self.walk(recv, depth),
            ExprKind::Arena(b) => {
                let inner = depth + 1;
                self.block(b, inner);
                if let Some(v) = &b.value {
                    // The block's value escapes to the enclosing region (`Region::arena(depth)`);
                    // a value bound to this inner arena cannot outlive it.
                    if self.tracks_region(v.ty) && !self.region_of(v, inner).outlives(Region::arena(depth)) {
                        self.diags.error(
                            "a value allocated in this arena cannot escape as the block's value".to_string(),
                            v.span,
                        );
                    }
                }
            }
            ExprKind::Block(b) => self.block(b, depth),
            // ④b: `task_group` opens a region (its task boxes live there), like `arena {}` — so a
            // region value (e.g. a `Task` handle) cannot escape as the block's value.
            ExprKind::TaskGroup(b) => {
                let inner = depth + 1;
                self.block(b, inner);
                if let Some(v) = &b.value {
                    if self.tracks_region(v.ty) && !self.region_of(v, inner).outlives(Region::arena(depth)) {
                        self.diags.error(
                            "a value from this task_group cannot escape as the block's value".to_string(),
                            v.span,
                        );
                    }
                }
            }
            ExprKind::Spawn { closure, .. } => self.walk(closure, depth),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.walk(p, depth);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.walk(scrutinee, depth);
                for a in arms {
                    self.walk(&a.body, depth);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.walk(result, depth);
                self.walk(f, depth);
            }
            ExprKind::TaskGet(inner) => self.walk(inner, depth),
            ExprKind::Wait => {}
            ExprKind::If { cond, then, els } => {
                self.walk(cond, depth);
                self.block(then, depth);
                self.block(els, depth);
            }
            ExprKind::Unary { expr, .. } => self.walk(expr, depth),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.walk(lhs, depth);
                self.walk(rhs, depth);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.walk(a, depth);
                }
            }
            // A fn value is a `Static` pointer (no region); an indirect call recurses its parts.
            ExprKind::FnValue(_) => {}
            // A capturing closure's env is frame-local and the closure cannot leave the frame
            // (no fn-typed returns/fields/parameters), so there is nothing to escape-check; just
            // recurse the captured values.
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.walk(c, depth);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.walk(callee, depth);
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
            | ExprKind::BoxClone(i) | ExprKind::StrClone(i) | ExprKind::StrBorrow(i) | ExprKind::BuilderToString(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayMinMax { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayPartition { source: i, .. } | ExprKind::ArrayParMap { source: i, .. } | ExprKind::ArraySort { source: i, .. } | ExprKind::ArraySortBy { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => self.walk(i, depth),
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.walk(recv, depth);
                self.walk(index, depth);
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.walk(builder, depth);
                self.walk(arg, depth);
            }
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.walk(source, depth);
                self.walk(init, depth);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.walk(a, depth);
                self.walk(b, depth);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.walk(source, depth);
                self.walk(n, depth);
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
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } => self.walk(input, depth),
            ExprKind::FsReadFile { path } => self.walk(path, depth),
            ExprKind::IoStdoutWrite { arg } => self.walk(arg, depth),
            ExprKind::IoStdoutWriteBuilder { builder } => self.walk(builder, depth),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::Field { .. }
            | ExprKind::BuilderNew
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
    /// Tuple defs — so a Move tuple (one with an owned element) is recognised as a Move type and
    /// its consumption (pass / destructure / return) is tracked for use-after-move.
    tuples: &'a [hir::TupleDef],
}

/// What has been moved out of a local. A whole-local move (`a := xs`, `f(xs)`, destructure) and a
/// partial tuple-field move (`a := t.0`, moving one owned element) coexist: each owned tuple field
/// can be moved out independently, after which the tuple may no longer be used as a whole.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum MovedKey {
    Whole(LocalId),
    Field(LocalId, u32),
}

type MovedSet = std::collections::HashSet<MovedKey>;

/// `id` is unusable as a whole if it was wholly moved or *any* of its fields was moved.
fn whole_moved(moved: &MovedSet, id: LocalId) -> bool {
    moved.contains(&MovedKey::Whole(id)) || moved.iter().any(|k| matches!(k, MovedKey::Field(l, _) if *l == id))
}

/// Field `n` of `id` is unusable if it (or the whole local) was moved.
fn field_moved(moved: &MovedSet, id: LocalId, n: u32) -> bool {
    moved.contains(&MovedKey::Field(id, n)) || moved.contains(&MovedKey::Whole(id))
}

/// Re-binding a local (`x := …`) clears every move record for it (whole and per-field).
fn clear_moved(moved: &mut MovedSet, id: LocalId) {
    moved.retain(|k| !matches!(k, MovedKey::Whole(l) | MovedKey::Field(l, _) if *l == id));
}

impl<'a> MoveCheck<'a> {
    fn check(mut self) {
        let mut moved: MovedSet = std::collections::HashSet::new();
        // If the function returns a Move type, its body's trailing expression is consumed by
        // the return: a bare owned local there (`fn make() -> array<i32> { ys := ...; ys }`) is
        // moved out to the caller (MIR nulls its slot so it is not also freed at exit).
        let ret_is_move = self.is_move_ty(self.f.ret);
        self.block(&self.f.body, &mut moved, ret_is_move, true);
    }

    /// Whether `ty` is a Move type (owns a heap buffer consumed on move) — including a Move tuple.
    fn is_move_ty(&self, ty: Ty) -> bool {
        // `Task<R>` (④b) is a box in the task_group region — Move, like `box<T>`.
        matches!(ty, Ty::Box(_) | Ty::Task(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) | Ty::String | Ty::Builder)
            || payload_is_move(ty)
            || ty_tuple_is_move(ty, self.tuples)
    }

    fn is_move(&self, id: LocalId) -> bool {
        match self.f.locals.get(id as usize).map(|l| l.ty) {
            Some(ty) => self.is_move_ty(ty),
            None => false,
        }
    }

    /// `tail_consuming` = whether the block's trailing value is consumed by its context;
    /// `tail_direct` = whether that consuming position is a "direct" move site (a statement /
    /// return / the function tail) rather than nested inside a branching expression (`if`).
    /// MIR nulls a moved owned local's slot only at direct sites, so a move of a *bound* owned
    /// local through an `if`/`else` arm is rejected here (deferred — bind it to a local first).
    fn block(
        &mut self,
        b: &Block,
        moved: &mut MovedSet,
        tail_consuming: bool,
        tail_direct: bool,
    ) {
        for s in &b.stmts {
            match s {
                Stmt::Let { local, init } => {
                    self.expr(init, moved, true, true);
                    clear_moved(moved, *local);
                }
                Stmt::Assign { local, value } => {
                    self.expr(value, moved, true, true);
                    clear_moved(moved, *local);
                }
                Stmt::AssignField { value, .. } => self.expr(value, moved, true, true),
                // `base[index] = value` — writing an element is a use of `base` (an owned array
                // could have been moved away), so flag use-after-move on it; index and value are
                // read (not moved; Copy).
                Stmt::AssignIndex { base, index, value } => {
                    if whole_moved(moved, *base) {
                        let name = &self.f.locals[*base as usize].name;
                        self.diags.error(format!("use of moved value '{name}'"), index.span);
                    }
                    self.expr(index, moved, false, false);
                    self.expr(value, moved, false, false);
                }
                Stmt::Return(Some(e)) => self.expr(e, moved, true, true),
                Stmt::Return(None) => {}
                Stmt::Expr(e) => self.expr(e, moved, false, false),
                // Destructure consumes its tuple source whole (see the `Local` arm in `expr`).
                Stmt::LetTuple { locals, init, .. } => {
                    self.expr(init, moved, true, true);
                    for l in locals.iter().flatten() {
                        clear_moved(moved, *l);
                    }
                }
            }
        }
        if let Some(v) = &b.value {
            self.expr(v, moved, tail_consuming, tail_direct);
        }
    }

    /// `consuming` = this position takes a Move value by value (so it moves it). `direct` = the
    /// consuming position is a direct move site (see [`block`]); a non-direct owned-local move
    /// is a deferred-feature error.
    fn expr(
        &mut self,
        e: &Expr,
        moved: &mut MovedSet,
        consuming: bool,
        direct: bool,
    ) {
        // A pipeline stage or reducer may carry capture operands (a lifted lambda's captured
        // enclosing locals); walk them as borrows so use-after-move of a captured value is caught.
        if let Some(stages) = pipeline_stages(&e.kind) {
            for c in stage_capture_exprs(stages) {
                self.expr(c, moved, false, false);
            }
        }
        for c in node_captures(&e.kind) {
            self.expr(c, moved, false, false);
        }
        match &e.kind {
            ExprKind::Local(id) => {
                if whole_moved(moved, *id) {
                    let name = &self.f.locals[*id as usize].name;
                    self.diags.error(format!("use of moved value '{name}'"), e.span);
                } else if consuming && self.is_move(*id) {
                    if !direct {
                        let name = &self.f.locals[*id as usize].name;
                        self.diags.error(
                            format!(
                                "cannot move owned value '{name}' out through a conditional \
                                 expression yet; bind the `if`/`else` result to a local first"
                            ),
                            e.span,
                        );
                    }
                    moved.insert(MovedKey::Whole(*id));
                }
            }
            ExprKind::Field { base, .. } | ExprKind::IndexField { base, .. } => {
                if whole_moved(moved, *base) {
                    let name = &self.f.locals[*base as usize].name;
                    self.diags.error(format!("use of moved value '{name}'"), e.span);
                }
            }
            ExprKind::Unary { expr, .. } => self.expr(expr, moved, false, false),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.expr(lhs, moved, false, false);
                self.expr(rhs, moved, false, false);
            }
            // Value arguments / wrapped payloads are consumed (a direct move site). `print` is a
            // read-only builtin, so it *borrows* its argument (a `string` printed once is still
            // usable — `print(s); s.len()`); it never takes ownership.
            ExprKind::Call { func, args } => {
                let consuming = func != "print";
                for a in args {
                    self.expr(a, moved, consuming, consuming);
                }
            }
            // A fn value is Copy (a pointer); an indirect call's callee + args are reads.
            ExprKind::FnValue(_) => {}
            // A closure copies its captured (Copy) values into its env — reads, not moves.
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.expr(c, moved, false, false);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.expr(callee, moved, false, false);
                for a in args {
                    self.expr(a, moved, true, true);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.expr(f, moved, true, true);
                }
            }
            ExprKind::OptionSome(i) | ExprKind::ResultOk(i) | ExprKind::ResultErr(i)
            | ExprKind::Try(i) | ExprKind::HeapNew(i) => self.expr(i, moved, true, true),
            // `b.to_string()` consumes (moves) the builder; `b.write(...)` borrows it (and its
            // str/int arg). `builder()` is a leaf.
            ExprKind::BuilderToString(i) => self.expr(i, moved, true, true),
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.expr(builder, moved, false, false);
                self.expr(arg, moved, false, false);
            }
            ExprKind::BuilderNew => {}
            // The receiver is borrowed, not consumed.
            ExprKind::BoxGet(i) | ExprKind::BoxClone(i) | ExprKind::StrClone(i) | ExprKind::StrBorrow(i) | ExprKind::ArraySum { source: i, .. } | ExprKind::ArrayCount { source: i, .. } | ExprKind::ArrayAnyAll { source: i, .. } | ExprKind::ArrayMinMax { source: i, .. } | ExprKind::ArrayToArray { source: i, .. } | ExprKind::ArrayPartition { source: i, .. } | ExprKind::ArrayParMap { source: i, .. } | ExprKind::ArraySort { source: i, .. } | ExprKind::ArraySortBy { source: i, .. } | ExprKind::ArrayToSlice(i)
            | ExprKind::Len(i) => {
                self.expr(i, moved, false, false)
            }
            // `recv[index]` / `recv[index].field` borrow the receiver (read an element) and read
            // the index.
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.expr(recv, moved, false, false);
                self.expr(index, moved, false, false);
            }
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.expr(source, moved, false, false);
                self.expr(init, moved, false, false);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.expr(a, moved, false, false);
                self.expr(b, moved, false, false);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.expr(source, moved, false, false);
                self.expr(n, moved, false, false);
            }
            ExprKind::ArrayLit { elems, .. } => {
                for e in elems {
                    self.expr(e, moved, true, true);
                }
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                self.expr(opt, moved, true, true);
                // The fallback is an arm value: it inherits this position's `consuming` but is
                // not a direct move site (like an `if`/`else` arm). Today Option payloads are
                // scalar-only, so a Move-typed unwrap result is not constructible — but treating
                // the fallback consistently keeps the analysis sound if that ever changes.
                self.expr(fallback, moved, consuming, false);
            }
            // A plain block is transparent: its tail inherits this position's consuming/direct.
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) => self.block(b, moved, consuming, direct),
            ExprKind::Spawn { closure, .. } => self.expr(closure, moved, false, false),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.expr(p, moved, false, false);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee, moved, false, false);
                for a in arms {
                    self.expr(&a.body, moved, consuming, direct);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                // `map_err` unwraps/consumes the result (its Ok payload may be an owned Move type).
                self.expr(result, moved, true, true);
                self.expr(f, moved, false, false);
            }
            // `t.get()` moves the result out of the task when `R` is an owned/move type, so it
            // consumes the task (a second `get()` would double-free the buffer).
            ExprKind::TaskGet(inner) => {
                let consuming = is_owned_droppable(e.ty);
                self.expr(inner, moved, consuming, consuming);
            }
            ExprKind::Wait => {}
            ExprKind::If { cond, then, els } => {
                self.expr(cond, moved, false, false);
                // An `if`/`else` arm value is a consuming-but-NOT-direct position: moving a
                // bound owned local out through it is rejected (the `direct = false`).
                let mut m1 = moved.clone();
                self.block(then, &mut m1, consuming, false);
                let mut m2 = moved.clone();
                self.block(els, &mut m2, consuming, false);
                // Conservative join: moved if moved on either path.
                *moved = &m1 | &m2;
            }
            ExprKind::Template(parts) => {
                for p in parts {
                    if let TemplatePart::Hole(h) | TemplatePart::JsonStr(h) = p {
                        // A hole value is read (copied) into the builder, not moved out.
                        self.expr(h, moved, false, false);
                    }
                }
            }
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } => self.expr(input, moved, false, false),
            ExprKind::FsReadFile { path } => self.expr(path, moved, false, false),
            ExprKind::IoStdoutWrite { arg } => self.expr(arg, moved, false, false),
            ExprKind::IoStdoutWriteBuilder { builder } => self.expr(builder, moved, false, false),
            // PR1 tuple elements are primitive (Copy) — a tuple literal moves nothing; tuple index
            // borrows. Recurse to catch moves in element subexpressions.
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.expr(el, moved, true, true);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.expr(o, moved, false, false);
                }
            }
            // `t.N` of a bound tuple reads field `N` independently of the other fields: it is
            // invalid only if *that* field (or the whole tuple) was moved — NOT if some *other*
            // field was moved (that must not poison a Copy-field read). An owned field in a
            // consuming position (`a := t.0`) is moved out (marked per field); a Copy read, or a
            // borrowing read, moves nothing. A non-local receiver just recurses as a borrow.
            ExprKind::TupleIndex { recv, index } => {
                match &recv.kind {
                    ExprKind::Local(t) => {
                        if field_moved(moved, *t, *index) {
                            let name = &self.f.locals[*t as usize].name;
                            self.diags.error(format!("use of moved field '.{index}' of '{name}'"), e.span);
                        } else {
                            let owned = matches!(self.f.locals.get(*t as usize).map(|l| l.ty), Some(Ty::Tuple(tid))
                                if self.tuples.get(tid as usize).and_then(|td| td.elems.get(*index as usize)).is_some_and(|s| s.is_move()));
                            if owned && consuming {
                                moved.insert(MovedKey::Field(*t, *index));
                            }
                        }
                    }
                    _ => self.expr(recv, moved, false, false),
                }
            }
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

struct Checker<'a, 't> {
    diags: &'a mut Diagnostics,
    sigs: &'a HashMap<String, FnSig>,
    struct_ids: &'a HashMap<String, u32>,
    enum_ids: &'a HashMap<String, u32>,
    enums: &'a [hir::EnumDef],
    /// The id of the builtin `Error` enum (so `Result<_, Error>` builtins build the right payload).
    error_enum_id: u32,
    structs: &'a [StructDef],
    /// The shared tuple-type interner (anonymous `(T, U, …)` types). A separate lifetime from
    /// `'a` so each per-function `Checker` can reborrow it mutably without conflicting with the
    /// long-lived shared `structs`/`struct_ids` borrows.
    tuples: &'t mut Vec<hir::TupleDef>,
    /// The shared `Ty::Fn` interner (function-value types). Same lifetime as `tuples`.
    fn_types: &'t mut Vec<hir::FnTy>,
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
    /// Nesting depth of `task_group {}` blocks (0 = not in one). `spawn`/`wait` are valid only
    /// inside a `task_group` scope (slice ④).
    task_group_depth: u32,
    /// Per open `task_group` (innermost last): whether a `wait()` is guaranteed to have run at the
    /// current point (so `get()` is allowed). `spawn` clears it (a new task is pending), `wait`
    /// sets it, and `if`/`else` merge it by dominance (`then && else`). Slice ④c: the
    /// `get`-before-`wait` check.
    wait_state: Vec<bool>,
    /// Per open `task_group` (innermost last): whether any spawned task is fallible (its closure
    /// returns `Result`). When true, `wait()` yields `Result<(), Error>` (else `()`) and only a
    /// `wait()?` (not a bare `wait()`) makes `get()` safe. Slice ④c-2.
    task_group_fallible: Vec<bool>,
    /// For each slice local bound from an array/slice (`s: slice<T> := a`), the **root** buffer
    /// local it borrows. Used by the `out` no-alias check so `fill(a, s)` (where `s` views `a`)
    /// is caught even though `s` and `a` are different locals.
    slice_bases: std::collections::HashMap<LocalId, LocalId>,
    /// The enclosing function's name — used to generate unique names for lifted lambdas.
    cur_fn: String,
    /// Lambdas lifted to synthetic top-level functions while checking this function's body. Pass 2
    /// appends them to `program.fns` so later passes / codegen treat them like named functions.
    lifted: Vec<hir::Fn>,
    /// Set while checking a lambda body — lets a reference to an enclosing local become a capture
    /// (a synthetic value parameter of the lifted function, passed at the call site).
    capture: Option<CaptureScope>,
}

/// Captured-variable bookkeeping while lifting a lambda. A reference in the body that misses the
/// lambda's own scope but resolves to an enclosing local is *captured*: a synthetic value parameter
/// is appended to the lifted function and the enclosing local is passed at the call site.
struct CaptureScope {
    /// The enclosing function's visible names → (enclosing LocalId, type), snapshot at lambda entry.
    enclosing: Vec<(String, LocalId, Ty)>,
    /// Captured enclosing locals, in capture order: (name, lifted-fn param LocalId, enclosing LocalId).
    captured: Vec<(String, LocalId, LocalId)>,
}

impl<'a, 't> Checker<'a, 't> {
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

    /// Resolve a name to a local, capturing an enclosing local if we are in a lambda body. A miss
    /// in the lambda's own scope that resolves to an enclosing local becomes a capture: a synthetic
    /// value parameter of the lifted function (reused on repeat references). The captured local's
    /// type is taken as-is here; `lift_lambda` rejects capturing a Move (owned) value afterward.
    fn lookup_or_capture(&mut self, name: &str) -> Option<LocalId> {
        if let Some(id) = self.lookup(name) {
            return Some(id);
        }
        let cap = self.capture.as_mut()?;
        if let Some(&(_, param_id, _)) = cap.captured.iter().find(|(n, _, _)| n == name) {
            return Some(param_id);
        }
        let (enc_id, ty) = cap.enclosing.iter().rev().find(|(n, _, _)| n == name).map(|(_, id, t)| (*id, *t))?;
        // A captured value becomes a synthetic parameter local (tracked in `captured`, *not* pushed
        // into the visible scope so a nested-block exit can't truncate it).
        let param_id = self.locals.len() as LocalId;
        self.locals.push(Local { id: param_id, name: name.to_string(), ty, is_mut: false });
        cap.captured.push((name.to_string(), param_id, enc_id));
        Some(param_id)
    }

    fn check_fn(&mut self, f: &ast::FnDecl) -> Fn {
        self.cur_fn = f.name.name.clone();
        let sig = &self.sigs[&f.name.name];
        let ret = sig.ret;
        let param_tys = sig.params.clone();
        // `main` takes no arguments, or exactly `args: array<str>` (argv, draft.md §19) with a
        // `Result<(), Error>` return — the latter is the only form the C-`main` wrapper marshals
        // argv into (an `-> i32` argv `main` is a later follow-up).
        if f.name.name == "main" && !f.params.is_empty() {
            let args_ok = param_tys.as_slice() == [Ty::DynArray(Scalar::Str)]
                && matches!(ret, Ty::Result(Scalar::Unit, Scalar::Enum(eid)) if eid == self.error_enum_id);
            if !args_ok {
                self.diags.error(
                    "main takes no arguments, or exactly `args: array<str>` with a `Result<(), Error>` return".to_string(),
                    f.span,
                );
            }
        }
        self.ret_hint = ret;

        let mut params = Vec::new();
        for (p, ty) in f.params.iter().zip(param_tys) {
            // An `out` parameter is a writable output buffer — only a `slice<T>` (a borrow the
            // callee writes back through). Mark its local mutable so `dst[i] = v` is allowed.
            if p.is_out && !matches!(ty, Ty::Slice(_) | Ty::Error) {
                self.diags.error(
                    format!("an `out` parameter must be a slice (a writable output buffer), got {}", ty_name(ty)),
                    p.ty.span(),
                );
            }
            let id = self.declare(&p.name.name, ty, p.is_out);
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
                        // A slice/str-annotated binding borrows its source (mirrors a call arg):
                        // `slice<T>` borrows an array, `str` borrows an owned `string`.
                        _ => match ann {
                            Some(Ty::Slice(ps)) => self.check_slice_init(init, ps),
                            Some(Ty::Str) => self.check_str_init(init),
                            _ => self.check_expr(init, ann),
                        },
                    };
                    let local_ty = ann.unwrap_or(init.ty);
                    let local = self.declare(&name.name, local_ty, *is_mut);
                    // Record slice provenance (`s: slice<T> := a` → `s` borrows `a`'s buffer) so
                    // the `out` no-alias check can see through slice variables.
                    if matches!(local_ty, Ty::Slice(_)) {
                        if let Some(root) = self.expr_root_local(&init) {
                            self.slice_bases.insert(local, root);
                        }
                    }
                    stmts.push(Stmt::Let { local, init });
                }
                ast::Stmt::LetTuple { names, init, span } => {
                    // `(a, b, ...) := expr` — the RHS must be a tuple; bind each name to its
                    // element type (`_` binds nothing). Element types are inferred from the tuple.
                    let init = self.check_expr(init, None);
                    if let Ty::Tuple(id) = self.resolve(init.ty) {
                        let elem_tys: Vec<Ty> =
                            self.tuples[id as usize].elems.iter().map(|s| scalar_to_ty(*s)).collect();
                        if elem_tys.len() != names.len() {
                            self.diags.error(
                                format!("this destructuring binds {} name(s) but the tuple has {} element(s)", names.len(), elem_tys.len()),
                                *span,
                            );
                        }
                        let mut locals = Vec::with_capacity(names.len());
                        for (i, n) in names.iter().enumerate() {
                            let ety = elem_tys.get(i).copied().unwrap_or(Ty::Error);
                            match n {
                                Some(name) => {
                                    locals.push(Some(self.declare(&name.name, ety, false)));
                                }
                                // An *ignored* (`_`) owned element must still be dropped, not leaked:
                                // bind it to a fresh hidden local so it joins the normal drop path
                                // (freed once at scope exit, or bulk-freed if arena-regioned). A
                                // Copy / `str` element needs no cleanup, so `_` binds nothing.
                                None if is_owned_droppable(ety) => {
                                    locals.push(Some(self.declare(&format!("_drop{i}"), ety, false)));
                                }
                                None => locals.push(None),
                            }
                        }
                        stmts.push(Stmt::LetTuple { locals, tuple_id: id, init });
                    } else {
                        // Not a tuple: declare the names as `Ty::Error` (no cascade of "undefined
                        // name") and keep the RHS as a plain expression statement — never emit a
                        // `LetTuple` whose `TupleIndex` lowering would panic codegen.
                        if self.resolve(init.ty) != Ty::Error {
                            self.diags.error(
                                format!("destructuring needs a tuple value, got {}", ty_name(init.ty)),
                                *span,
                            );
                        }
                        for n in names.iter().flatten() {
                            self.declare(&n.name, Ty::Error, false);
                        }
                        stmts.push(Stmt::Expr(init));
                    }
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
                        // Mirror the `let` path: a slice/str place borrows its source.
                        let v = match ty {
                            Ty::Slice(ps) => self.check_slice_init(value, ps),
                            Ty::Str => self.check_str_init(value),
                            _ => self.check_expr(value, Some(ty)),
                        };
                        stmts.push(Stmt::Assign { local: id, value: v });
                    }
                    Place::Field { base, index, ty } => {
                        let v = self.check_expr(value, Some(ty));
                        stmts.push(Stmt::AssignField { base, index, value: v });
                    }
                    Place::Index { base, index, elem } => {
                        let v = self.check_expr(value, Some(elem));
                        stmts.push(Stmt::AssignIndex { base, index, value: v });
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
        resolve_type(t, self.struct_ids, self.enum_ids, self.tuples, self.fn_types, self.diags)
    }

    /// Resolve an assignable place: a `mut` local, or `mut_local.field`.
    fn check_place(&mut self, place: &ast::Expr) -> Place {
        // `local[index] = v` — element store into a `mut` array local or `out` slice parameter.
        if let ast::ExprKind::Index { recv, index } = &place.kind {
            let Some((id, local_ty)) = self.place_local(recv) else {
                self.diags.error("invalid assignment target".to_string(), place.span);
                return Place::Err;
            };
            if !self.locals[id as usize].is_mut {
                let name = self.locals[id as usize].name.clone();
                self.diags.error(
                    format!("cannot assign to an element of immutable '{name}' (declare with `mut`, or use an `out` parameter)"),
                    place.span,
                );
            }
            let elem = match local_ty {
                Ty::Slice(s) | Ty::Array(s, _) | Ty::DynArray(s) => scalar_to_ty(s),
                Ty::Error => return Place::Err,
                other => {
                    self.diags.error(
                        format!("cannot index-assign into {} (only an array or slice)", ty_name(other)),
                        place.span,
                    );
                    return Place::Err;
                }
            };
            // First cut: element stores are primitive-scalar only (int/float/bool/char). A `str`
            // element store would need a region check (storing a borrowed view into the buffer);
            // struct / Move elements need whole-struct / ownership handling. Both deferred.
            if ty_to_scalar(elem).and_then(scalar_to_prim).is_none() {
                self.diags.error(
                    format!("element assignment of {} is not supported yet (primitive elements only for now)", ty_name(elem)),
                    place.span,
                );
                return Place::Err;
            }
            let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
            if i.ty == Ty::Error {
                return Place::Err;
            }
            if !i.ty.is_int_like() {
                self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
                return Place::Err;
            }
            return Place::Index { base: id, index: i, elem };
        }
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
            ast::ExprKind::Index { recv, index } => self.check_index(recv, index, e.span),
            ast::ExprKind::Template(parts) => self.check_template(parts, expected, e.span),
            ast::ExprKind::FieldShorthand(_) => {
                self.diags.error(
                    "`.field` is only valid as a pipeline stage argument (e.g. `where(.active)`)".to_string(),
                    e.span,
                );
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span: e.span }
            }
            // A lambda used as a value (`f := fn x: i32 { … }`) is a first-class function value
            // (`Ty::Fn`): lift it like a stage lambda, but its parameter types come from explicit
            // annotations (there is no use site to infer from). Slice ②a: non-capturing only.
            ast::ExprKind::Lambda { params, body } => self.check_lambda_value(params, body, expected, e.span),
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
            ast::ExprKind::TaskGroup(b) => {
                let diverges = ast_block_diverges(b);
                // A `task_group` opens a region (like `arena {}`): spawned task handles are boxes
                // in it, region-tied to the scope (so a `Task` can't escape).
                self.task_group_depth += 1;
                self.arena_depth += 1;
                self.wait_state.push(false);
                self.task_group_fallible.push(false);
                let block = self.check_block(b, if diverges { None } else { expected });
                self.task_group_fallible.pop();
                self.wait_state.pop();
                self.arena_depth -= 1;
                self.task_group_depth -= 1;
                let ty = if diverges {
                    expected.unwrap_or(Ty::Unit)
                } else {
                    let t = block.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit);
                    self.constrain(t, expected, e.span);
                    t
                };
                Expr { kind: ExprKind::TaskGroup(block), ty, span: e.span }
            }
            ast::ExprKind::StructLit { name, fields } => {
                // A struct literal is a value expression (constructed, then passed/returned/
                // stored). The `let` path checks it directly to store fields in place.
                self.check_struct_lit(name, fields, e.span)
            }
            ast::ExprKind::Tuple(elems) => self.check_tuple(elems, expected, e.span),
            ast::ExprKind::TupleIndex { recv, index } => self.check_tuple_index(recv, *index, expected, e.span),
            ast::ExprKind::If { cond, then, els } => self.check_if(cond, then, els.as_deref(), expected, e.span),
            ast::ExprKind::Match { scrutinee, arms } => self.check_match(scrutinee, arms, expected, e.span),
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

    /// `(e0, e1, ...)` — a tuple literal. Element types are taken from the expected tuple type
    /// when context fixes one (e.g. a multi-value `return`), else each element defaults like a
    /// bare `:=` binding (int → i64, float → f64). PR1 cut: elements are primitive scalars.
    fn check_tuple(&mut self, elems: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // If the context fixes a concrete tuple type, use its element types to drive checking.
        let expected_elems: Option<Vec<Ty>> = match expected.map(|t| self.resolve(t)) {
            Some(Ty::Tuple(id)) => {
                Some(self.tuples[id as usize].elems.iter().map(|s| scalar_to_ty(*s)).collect())
            }
            _ => None,
        };
        if let Some(exp) = &expected_elems {
            if exp.len() != elems.len() {
                self.diags.error(
                    format!("expected a tuple of {} element(s), got {}", exp.len(), elems.len()),
                    span,
                );
                return err;
            }
        }
        let mut checked = Vec::with_capacity(elems.len());
        let mut scalars = Vec::with_capacity(elems.len());
        let mut ok = true;
        for (i, el) in elems.iter().enumerate() {
            let exp_i = expected_elems.as_ref().map(|v| v[i]);
            let ce = self.check_expr(el, exp_i);
            // Commit the element to a concrete scalar: bind any inference var to the expected type
            // or its default, so the interned tuple type (and later uses of the element) agree.
            let concrete = self.finalize(ce.ty);
            self.constrain(ce.ty, Some(concrete), ce.span);
            match ty_to_scalar(self.resolve(ce.ty)) {
                Some(s @ (Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char
                | Scalar::Str | Scalar::String | Scalar::DynArray(_) | Scalar::DynStructArray(_))) => scalars.push(s),
                _ => {
                    if ce.ty != Ty::Error {
                        self.diags.error(
                            format!("tuple elements must be a scalar, str, owned string, or owned array for now, got {}", ty_name(ce.ty)),
                            ce.span,
                        );
                    }
                    ok = false;
                }
            }
            checked.push(ce);
        }
        if !ok {
            return err;
        }
        let id = intern_tuple(self.tuples, scalars);
        let ty = Ty::Tuple(id);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::Tuple { tuple_id: id, elems: checked }, ty, span }
    }

    /// `recv.N` — positional tuple access.
    fn check_tuple_index(&mut self, recv: &ast::Expr, index: u32, expected: Option<Ty>, span: Span) -> Expr {
        // On any error return a dummy `Ty::Error` expr (not a `TupleIndex` node): a `TupleIndex`
        // whose receiver is not a tuple would panic codegen's `into_struct_value()`.
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let r = self.check_expr(recv, None);
        match self.resolve(r.ty) {
            Ty::Tuple(id) => {
                let elems = &self.tuples[id as usize].elems;
                match elems.get(index as usize) {
                    // Reading an *owned* element by index moves it out of the tuple, leaving the
                    // other elements usable (MoveCheck tracks the per-field move; MIR nulls the
                    // moved field so the tuple's `Drop` frees null there). This needs a bound
                    // tuple (a `Local`) to name the field being moved; a non-local tuple temporary
                    // (`f().0`) would orphan its other owned elements, so destructure that instead.
                    Some(s) if s.is_move() => {
                        if !matches!(r.kind, ExprKind::Local(_)) {
                            self.diags.error(
                                format!("`.{index}` would move the owned element {} out of a temporary tuple — bind it to a variable, or destructure with `(a, b) := …`", scalar_name(*s)),
                                span,
                            );
                            return err;
                        }
                        let ty = scalar_to_ty(*s);
                        self.constrain(ty, expected, span);
                        Expr { kind: ExprKind::TupleIndex { recv: Box::new(r), index }, ty, span }
                    }
                    Some(s) => {
                        let ty = scalar_to_ty(*s);
                        self.constrain(ty, expected, span);
                        Expr { kind: ExprKind::TupleIndex { recv: Box::new(r), index }, ty, span }
                    }
                    None => {
                        self.diags.error(
                            format!("tuple index .{index} is out of range (tuple has {} element(s))", elems.len()),
                            span,
                        );
                        err
                    }
                }
            }
            Ty::Error => err,
            other => {
                self.diags.error(
                    format!("`.{index}` needs a tuple value, got {}", ty_name(other)),
                    span,
                );
                err
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
        let Some(id) = self.lookup_or_capture(base) else {
            // A top-level function used as a value (`f := double`) is a first-class function
            // pointer (`Ty::Fn`). Slice ①: scalar params/ret, no `out` params.
            if let Some(sig) = self.sigs.get(base) {
                let params: Option<Vec<Scalar>> = sig.params.iter().map(|t| ty_to_scalar(*t)).collect();
                let ret = ty_to_scalar(sig.ret);
                match (params, ret) {
                    (Some(ps), Some(r)) if !sig.out.iter().any(|o| *o) => {
                        let fid = intern_fn_type(self.fn_types, ps, r);
                        let ty = Ty::Fn(fid);
                        self.constrain(ty, expected, span);
                        return Expr { kind: ExprKind::FnValue(base.to_string()), ty, span };
                    }
                    _ => {
                        self.diags.error(
                            format!("'{base}' cannot be used as a function value yet (only scalar parameters/return, no `out`)"),
                            span,
                        );
                        return err(span);
                    }
                }
            }
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
        // `Type.Variant` — a sum-type value (not field access on a value). The base is a sum-type
        // name, the field is a variant.
        if let ast::ExprKind::Path(p) = &recv.kind {
            if let Some(name) = single_name(p) {
                if let Some(&enum_id) = self.enum_ids.get(name) {
                    return match self.enums[enum_id as usize].variants.iter().position(|v| v.name == field.name) {
                        Some(idx) => {
                            let arity = self.enums[enum_id as usize].variants[idx].payload.len();
                            if arity != 0 {
                                self.diags.error(
                                    format!("variant '{}' takes {arity} argument(s); construct it as `{}.{}(…)`", field.name, name, field.name),
                                    span,
                                );
                                return err;
                            }
                            let ty = Ty::Enum(enum_id);
                            self.constrain(ty, expected, span);
                            Expr { kind: ExprKind::EnumValue { enum_id, variant: idx as u32, payload: Vec::new() }, ty, span }
                        }
                        None => {
                            self.diags.error(
                                format!("'{}' is not a variant of '{}'", field.name, self.enums[enum_id as usize].name),
                                span,
                            );
                            err
                        }
                    };
                }
            }
        }
        // `arr[i].field` — field access on a struct-array element. Fused into one bounds-checked
        // element-field load (MMv2 slice 8f); a whole-struct `arr[i]` value is not materialized.
        if let ast::ExprKind::Index { recv: arr, index } = &recv.kind {
            return self.check_index_field(arr, index, field, expected, span);
        }
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
    /// Follow a slice local to the root buffer it borrows (an array, or a slice parameter — its
    /// own root). A non-slice / unborrowed local is its own root.
    fn root_local(&self, id: LocalId) -> LocalId {
        let mut cur = id;
        let mut guard = 0;
        while let Some(&base) = self.slice_bases.get(&cur) {
            if base == cur || guard > self.locals.len() {
                break;
            }
            cur = base;
            guard += 1;
        }
        cur
    }

    /// The root buffer local an HIR expression borrows, if it resolves to one (a local or an
    /// array→slice borrow). Used to record slice provenance for the `out` no-alias check.
    fn expr_root_local(&self, e: &Expr) -> Option<LocalId> {
        match &e.kind {
            ExprKind::Local(id) => Some(self.root_local(*id)),
            ExprKind::ArrayToSlice(inner) => self.expr_root_local(inner),
            _ => None,
        }
    }

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

    /// `x.wrapping_add(y)` / `x.saturating_sub(y)` / `x.checked_mul(y)` etc. (`core.math`). The
    /// receiver and the single operand must be the same integer type. `wrapping_*` is the default
    /// (the language already wraps), so it lowers to a plain `Binary`; `saturating_*` clamps and
    /// yields the int type; `checked_*` yields `Option<T>` (`None` on overflow).
    fn check_int_arith_method(&mut self, recv: &ast::Expr, method: &str, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let (op, mode) = parse_int_arith(method).expect("dispatched only for an int-arith method");
        let r = self.check_expr(recv, None);
        if r.ty == Ty::Error {
            return err;
        }
        if !r.ty.is_int_like() {
            self.diags.error(
                format!("'{method}' is an integer overflow operation, but the receiver is {}", ty_name(r.ty)),
                span,
            );
            return err;
        }
        let [arg] = args else {
            self.diags.error(format!("'{method}' takes 1 argument (the other operand), got {}", args.len()), span);
            return err;
        };
        let a = self.check_expr(arg, Some(r.ty));
        if a.ty == Ty::Error {
            return err;
        }
        // Unify the operands (binds an unconstrained literal operand to the other's type) rather
        // than compare — `a.checked_add(5)` must accept the literal `5` as the same int.
        let t = self.unify(r.ty, a.ty, span);
        if t == Ty::Error {
            return err;
        }
        if !t.is_int_like() {
            self.diags.error(format!("'{method}' needs integer operands, got {}", ty_name(t)), span);
            return err;
        }
        let (lhs, rhs) = (Box::new(r), Box::new(a));
        match mode {
            // `wrapping_*` is the default wrapping arithmetic.
            None => Expr { kind: ExprKind::Binary { op, lhs, rhs }, ty: t, span },
            Some(m @ hir::ArithMode::Saturating) => Expr { kind: ExprKind::IntArith { op, mode: m, lhs, rhs }, ty: t, span },
            // `checked_*` yields `Option<T>`. The payload scalar must be concrete now (no inference
            // var inside a composite), so resolve `t` — an unconstrained literal pair defaults to i64.
            Some(m @ hir::ArithMode::Checked) => {
                let scalar = ty_to_scalar(self.finalize(t)).expect("an integer type is a scalar payload");
                Expr { kind: ExprKind::IntArith { op, mode: m, lhs, rhs }, ty: Ty::Option(scalar), span }
            }
        }
    }

    /// `x.abs()` / `a.min(b)` / `a.max(b)` (`core.math`). The receiver must be numeric; `min`/`max`
    /// take one operand of the same type. The result is that numeric type.
    fn check_scalar_math(&mut self, recv: &ast::Expr, fn_: hir::MathFn, args: &[ast::Expr], span: Span) -> Expr {
        use hir::MathFn::*;
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let name = match fn_ {
            Abs => "abs",
            Min => "min",
            Max => "max",
            Sqrt => "sqrt",
            Floor => "floor",
            Ceil => "ceil",
            Round => "round",
            Trunc => "trunc",
            Pow => "pow",
        };
        // `(want_args, float_only)`: `abs`/`min`/`max` accept any numeric; the rest are float-only.
        // `min`/`max`/`pow` take one operand; the others take none.
        let (want_args, float_only) = match fn_ {
            Abs => (0, false),
            Min | Max => (1, false),
            Sqrt | Floor | Ceil | Round | Trunc => (0, true),
            Pow => (1, true),
        };
        let r = self.check_expr(recv, None);
        if r.ty == Ty::Error {
            return err;
        }
        let ok_ty = if float_only { r.ty.is_float_like() } else { r.ty.is_numeric() };
        if !ok_ty {
            let want = if float_only { "a float" } else { "a numeric" };
            self.diags.error(format!("'{name}' needs {want} receiver, got {}", ty_name(r.ty)), span);
            return err;
        }
        if args.len() != want_args {
            self.diags.error(format!("'{name}' takes {want_args} argument(s), got {}", args.len()), span);
            return err;
        }
        let mut operands = vec![r];
        if let [arg] = args {
            let recv_ty = operands[0].ty;
            let a = self.check_expr(arg, Some(recv_ty));
            if a.ty == Ty::Error {
                return err;
            }
            let t = self.unify(recv_ty, a.ty, span);
            if t == Ty::Error {
                return err;
            }
            operands.push(a);
        }
        let ty = operands[0].ty;
        Expr { kind: ExprKind::MathOp { fn_, operands }, ty, span }
    }

    /// `f := fn x: i32 { … }` — a lambda used as a value. Lifts the lambda (its parameter types
    /// from the explicit annotations, the return type from the body) to a synthetic top-level
    /// function and yields a `Ty::Fn` value. Slice ②a: non-capturing only; scalar signatures.
    fn check_lambda_value(&mut self, params: &[ast::LambdaParam], body: &ast::Block, expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Parameter types come from the explicit annotations (no use site to infer from).
        let mut param_tys = Vec::with_capacity(params.len());
        for p in params {
            let Some(ann) = &p.ty else {
                self.diags.error(
                    format!("lambda parameter '{}' needs a type annotation to be used as a value (e.g. `fn {}: i32 {{ … }}`)", p.name.name, p.name.name),
                    p.name.span,
                );
                return err;
            };
            param_tys.push(resolve_type(ann, self.struct_ids, self.enum_ids, self.tuples, self.fn_types, self.diags));
        }
        // Lift with the annotated parameter types as the expected signature; the return type is
        // inferred from the body.
        let Some((name, ret, captures)) = self.lift_lambda(params, body, &param_tys, None, span) else {
            return err;
        };
        // A type error in the annotations or the body has already been reported — don't pile on a
        // confusing secondary "only scalar" message.
        if param_tys.iter().any(|t| self.finalize(*t) == Ty::Error) || self.finalize(ret) == Ty::Error {
            return err;
        }
        // Scalar signature only (slice ②a), matching named function values. The captures are
        // hidden from the closure's *type* — only the explicit parameters appear in `Ty::Fn`.
        let pscalars: Option<Vec<Scalar>> = param_tys.iter().map(|t| ty_to_scalar(self.finalize(*t))).collect();
        let rscalar = ty_to_scalar(self.finalize(ret));
        let (Some(ps), Some(r)) = (pscalars, rscalar) else {
            self.diags.error("a lambda value supports only scalar parameters and return type".to_string(), span);
            return err;
        };
        let fid = intern_fn_type(self.fn_types, ps, r);
        let ty = Ty::Fn(fid);
        self.constrain(ty, expected, span);
        // No captures → a plain function pointer (slice ②a). Captures → a closure carrying its
        // captured values in an environment (slice ②b-2); since a `Ty::Fn` value cannot leave its
        // frame yet (no fn-typed returns/fields/parameters), the environment is frame-local.
        if captures.is_empty() {
            Expr { kind: ExprKind::FnValue(name), ty, span }
        } else {
            Expr { kind: ExprKind::Closure { lifted: name, captures }, ty, span }
        }
    }

    /// `spawn(fn { … })` — defer a task in the enclosing `task_group`. The argument is a
    /// `fn() -> R` value (a no-parameter lambda, captures by value); the result is `Task<R>`.
    fn check_spawn(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        if self.task_group_depth == 0 {
            self.diags.error("'spawn' is only valid inside a `task_group { … }` scope".to_string(), span);
            return err;
        }
        let [arg] = args else {
            self.diags.error(format!("'spawn' takes one argument (a `fn {{ … }}`), got {}", args.len()), span);
            return err;
        };
        // `spawn` takes a literal lambda (consumed here, never a free `Ty::Fn` value), so it is
        // lifted directly — which lets the task closure return `Result<R, Error>` (a fallible
        // task), unlike a `Ty::Fn` value whose return is scalar-only.
        let ast::ExprKind::Lambda { params, body } = &arg.kind else {
            self.diags.error("'spawn' takes a `fn { … }` literal".to_string(), arg.span);
            return err;
        };
        if !params.is_empty() {
            self.diags.error("a spawned task takes no parameters (`spawn(fn { … })`)".to_string(), arg.span);
            return err;
        }
        let Some((name, ret, captures)) = self.lift_lambda(params, body, &[], None, arg.span) else {
            return err;
        };
        // Classify the result: `Result<ok, Error>` → a fallible task (`wait()?` surfaces the `Err`),
        // `Task<ok>`; a primitive scalar → an infallible task, `Task<scalar>`. The result is stored
        // in a `box` in the region, so `ok` must be a box-able primitive (owned/view results are a
        // later slice).
        let is_prim = |s: Scalar| matches!(s, Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char | Scalar::Unit);
        let (ok, fallible) = match self.finalize(ret) {
            Ty::Result(o, Scalar::Enum(eid)) if eid == self.error_enum_id && is_prim(o) => (o, true),
            // A type error in the lambda body was already reported — don't cascade.
            Ty::Error => return err,
            other => match ty_to_scalar(other) {
                Some(s) if is_prim(s) => (s, false),
                _ => {
                    self.diags.error(
                        format!("a spawned task must return a primitive scalar or `Result<scalar, Error>` for now, got {}", ty_name(other)),
                        arg.span,
                    );
                    return err;
                }
            },
        };
        // The closure value (the `{thunk, env}` machinery is reused as-is; the lifted function may
        // return `Result` — the thunk just forwards it). Its `Ty::Fn` tag uses the `ok` scalar as
        // the return (a repr-only tag — a closure value is a pointer pair regardless).
        let fid = intern_fn_type(self.fn_types, Vec::new(), ok);
        let cty = Ty::Fn(fid);
        let closure = if captures.is_empty() {
            Expr { kind: ExprKind::FnValue(name), ty: cty, span: arg.span }
        } else {
            Expr { kind: ExprKind::Closure { lifted: name, captures }, ty: cty, span: arg.span }
        };
        if fallible {
            if let Some(f) = self.task_group_fallible.last_mut() {
                *f = true;
            }
        }
        // A new task is now pending and unjoined, so a prior `wait()` no longer covers everything.
        if let Some(w) = self.wait_state.last_mut() {
            *w = false;
        }
        Expr { kind: ExprKind::Spawn { closure: Box::new(closure), fallible }, ty: Ty::Task(ok), span }
    }

    /// `wait()` — join all spawned tasks. ④a: a no-op marker (eager execution).
    fn check_wait(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if self.task_group_depth == 0 {
            self.diags.error("'wait' is only valid inside a `task_group { … }` scope".to_string(), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        if !args.is_empty() {
            self.diags.error(format!("'wait' takes no arguments, got {}", args.len()), span);
        }
        let fallible = self.task_group_fallible.last().copied().unwrap_or(false);
        if fallible {
            // A fallible group's `wait()` yields `Result<(), Error>`. `get()` is made safe only by a
            // *successful* `wait()` — i.e. `wait()?` (the `Try` sets the wait-state); a bare `wait()`
            // whose `Err` is ignored leaves failed tasks' slots uninitialised, so it does NOT enable
            // `get()` here.
            Expr { kind: ExprKind::Wait, ty: Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id)), span }
        } else {
            // Infallible group: `wait()` joins and yields `()`; all results are now readable.
            if let Some(w) = self.wait_state.last_mut() {
                *w = true;
            }
            Expr { kind: ExprKind::Wait, ty: Ty::Unit, span }
        }
    }

    /// `f(args)` where `f` is a `Ty::Fn` local — an indirect call through a function value.
    fn check_call_fn_value(&mut self, lid: LocalId, fid: u32, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        // Clone only the parameter scalars (needed across the `&mut self` `check_expr` calls);
        // the per-arg LLVM type is computed in the loop, with no intermediate `Vec<Ty>`.
        let params = self.fn_types[fid as usize].params.clone();
        let ret = scalar_to_ty(self.fn_types[fid as usize].ret);
        if args.len() != params.len() {
            self.diags.error(
                format!("this function value expects {} argument(s), got {}", params.len(), args.len()),
                span,
            );
            return err;
        }
        let mut checked = Vec::with_capacity(args.len());
        for (a, p) in args.iter().zip(&params) {
            let pt = scalar_to_ty(*p);
            let e = self.check_expr(a, Some(pt));
            if e.ty != Ty::Error && self.resolve(e.ty) != pt {
                self.diags.error(
                    format!("argument type mismatch: expected {}, got {}", ty_name(pt), ty_name(e.ty)),
                    e.span,
                );
            }
            checked.push(e);
        }
        let callee = Expr { kind: ExprKind::Local(lid), ty: Ty::Fn(fid), span };
        self.constrain(ret, expected, span);
        Expr { kind: ExprKind::CallFnValue { callee: Box::new(callee), args: checked }, ty: ret, span }
    }

    fn check_call(&mut self, callee: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        // `Type.Variant(args)` — constructing a sum-type value with a payload.
        if let ast::ExprKind::FieldAccess { recv, field } = &callee.kind {
            if let ast::ExprKind::Path(p) = &recv.kind {
                if let Some(name) = single_name(p) {
                    if let Some(&enum_id) = self.enum_ids.get(name) {
                        return self.check_variant_ctor(enum_id, field, args, expected, span);
                    }
                }
            }
        }
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
        if name == "builder" {
            return self.check_builder_new(args, span);
        }
        if name == "spawn" {
            return self.check_spawn(args, span);
        }
        if name == "wait" {
            return self.check_wait(args, span);
        }
        // An indirect call through a function-value local: `f(args)` where `f: Ty::Fn`.
        if let Some(lid) = self.lookup(&name) {
            if let Ty::Fn(fid) = self.resolve(self.locals[lid as usize].ty) {
                return self.check_call_fn_value(lid, fid, args, expected, span);
            }
        }
        let Some(sig) = self.sigs.get(&name) else {
            self.diags.error(format!("undefined function: '{name}'"), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        };
        let (param_tys, ret, out) = (sig.params.clone(), sig.ret, sig.out.clone());
        if args.len() != param_tys.len() {
            self.diags.error(
                format!("'{name}' expects {} argument(s), got {}", param_tys.len(), args.len()),
                span,
            );
        }
        // No-alias check: an `out` argument must not name the same local as any other argument
        // (the no-alias guarantee `out` lowers to LLVM `noalias`). A conservative base-local
        // comparison — every slice of an array `a` goes through `a` directly today, so distinct
        // locals are genuinely distinct buffers.
        let arg_root = |s: &Self, a: &ast::Expr| s.place_local(a).map(|(id, _)| s.root_local(id));
        for (i, is_out) in out.iter().enumerate() {
            if !is_out {
                continue;
            }
            let Some(arg_i) = args.get(i) else { continue };
            let Some(base) = arg_root(self, arg_i) else { continue };
            for (j, a) in args.iter().enumerate() {
                if j != i && arg_root(self, a) == Some(base) {
                    let lname = self.locals[base as usize].name.clone();
                    self.diags.error(
                        format!("the `out` argument also aliases '{lname}', another argument to '{name}' — an `out` buffer must not alias the other arguments"),
                        arg_i.span,
                    );
                    break;
                }
            }
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
        if let Some(Ty::Str) = param {
            return self.check_str_init(a);
        }
        self.check_expr(a, param)
    }

    /// Check an expression expected to be a `str`, applying the `string` → `str` borrow
    /// (`StrBorrow`) when the source is an owned `string` (MMv2 slice 7b/7e): zero-cost (same
    /// `{ptr,len}` layout), non-consuming (the `string` stays owned by its slot). Shared by call
    /// arguments, `str`-annotated `let` bindings, and `str`-place assignments. Pass `None` first so
    /// the source types as `string`, then wrap the borrow.
    fn check_str_init(&mut self, a: &ast::Expr) -> Expr {
        let e = self.check_expr(a, None);
        if e.ty == Ty::String {
            let span = e.span;
            return Expr { kind: ExprKind::StrBorrow(Box::new(e)), ty: Ty::Str, span };
        }
        if e.ty != Ty::Str {
            self.constrain(e.ty, Some(Ty::Str), e.span);
        }
        e
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
            if single_name(p) == Some("fs") && method == "read_file" {
                return self.check_fs_read_file(args, span);
            }
        }
        // `io.stdout.write(s)` — the receiver is the 2-segment `io.stdout`, so it parses as a
        // `FieldAccess` (`io` . `stdout`), not a single-name path.
        if method == "write" {
            if let ast::ExprKind::FieldAccess { recv: inner, field } = &recv.kind {
                if let ast::ExprKind::Path(p) = &inner.kind {
                    if single_name(p) == Some("io") && field.name == "stdout" {
                        return self.check_io_stdout_write(args, span);
                    }
                }
            }
        }
        // Explicit-overflow integer arithmetic (`core.math`): `x.{wrapping,saturating,checked}_{add,sub,mul}(y)`.
        if parse_int_arith(method).is_some() {
            return self.check_int_arith_method(recv, method, args, span);
        }
        // `sum` / `reduce` are the terminals of a fused pipeline.
        if method == "sum" {
            return self.check_array_sum(recv, args, expected, span);
        }
        if method == "reduce" {
            return self.check_array_reduce(recv, args, expected, span);
        }
        if method == "scan" {
            return self.check_array_scan(recv, args, span);
        }
        if method == "dot" {
            return self.check_array_dot(recv, args, expected, span);
        }
        if method == "sort" {
            return self.check_array_sort(recv, args, span);
        }
        if method == "sort_by_key" {
            return self.check_array_sort_by_key(recv, args, span);
        }
        if method == "count" {
            return self.check_array_count(recv, args, span);
        }
        if method == "any" || method == "all" {
            return self.check_array_any_all(recv, args, method == "all", span);
        }
        // `arr.min()` / `arr.max()` (no args) is the array reduction; `a.min(b)` / `a.max(b)`
        // (one arg) is the pairwise scalar math op.
        if (method == "min" || method == "max") && args.is_empty() {
            return self.check_array_min_max(recv, args, expected, method == "max", span);
        }
        if method == "abs" {
            return self.check_scalar_math(recv, hir::MathFn::Abs, args, span);
        }
        if method == "min" {
            return self.check_scalar_math(recv, hir::MathFn::Min, args, span);
        }
        if method == "max" {
            return self.check_scalar_math(recv, hir::MathFn::Max, args, span);
        }
        // Float-only math functions (`core.math`).
        let float_fn = match method {
            "sqrt" => Some(hir::MathFn::Sqrt),
            "floor" => Some(hir::MathFn::Floor),
            "ceil" => Some(hir::MathFn::Ceil),
            "round" => Some(hir::MathFn::Round),
            "trunc" => Some(hir::MathFn::Trunc),
            "pow" => Some(hir::MathFn::Pow),
            _ => None,
        };
        if let Some(f) = float_fn {
            return self.check_scalar_math(recv, f, args, span);
        }
        if method == "to_array" {
            return self.check_array_to_array(recv, args, span);
        }
        if method == "partition" {
            return self.check_array_partition(recv, args, span);
        }
        if method == "par_map" {
            return self.check_array_par_map(recv, args, span);
        }
        if method == "chunks" {
            return self.check_array_chunks(recv, args, span);
        }
        // Builder methods (MMv2 slice 7c/7d): typed `write*` appends, `to_string` finishes.
        if let Some(kind) = builder_write_kind(method) {
            return self.check_builder_write(recv, args, kind, span);
        }
        if method == "to_string" {
            return self.check_builder_to_string(recv, args, span);
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
            "map_err" if matches!(self.resolve(recv_ty), Ty::Result(..)) => {
                self.check_map_err(recv_expr, args, expected, span)
            }
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

    /// Resolve a stage's function argument (named or lambda) over element type `elem`, returning
    /// the (possibly synthetic) function name to lower to and its return type. A lambda is lifted
    /// to a synthetic top-level function (`lift_lambda`); a named function is checked in place.
    fn resolve_stage_fn(&mut self, sf: &StageFn, elem: Ty, is_pred: bool) -> Option<(String, Ty, Vec<Expr>)> {
        match sf {
            StageFn::Named(fname) => Some((fname.name.clone(), self.check_stage_fn(fname, elem, is_pred), Vec::new())),
            StageFn::Lambda { params, body, span } => {
                let expected_ret = if is_pred { Some(Ty::Bool) } else { None };
                self.lift_lambda(params, body, &[elem], expected_ret, *span)
            }
        }
    }

    /// Lift an inline lambda to a synthetic top-level function and return its generated name + its
    /// return type. The lambda is checked as an isolated function body (its own locals/scope/
    /// inference), with parameter types taken from `expected_params` (so a `slice<i32>.map(fn x …)`
    /// gives `x: i32`); an inline-literal element type defaults like any unconstrained literal. The
    /// lifted function joins `program.fns` (Pass 2 collects `self.lifted`), so move/escape/purity
    /// analysis and codegen treat it exactly like a named function — including the `par_map` Pure
    /// requirement. The fused-loop lowering is then identical to a named stage function.
    ///
    /// Slice ① cut: **non-capturing** only — the lambda body sees its parameters and top-level
    /// functions, but not enclosing locals (capturing those is a follow-up; such a reference
    /// surfaces as an undefined-variable error here).
    fn lift_lambda(
        &mut self,
        params: &[ast::LambdaParam],
        body: &ast::Block,
        expected_params: &[Ty],
        expected_ret: Option<Ty>,
        span: Span,
    ) -> Option<(String, Ty, Vec<Expr>)> {
        if params.len() != expected_params.len() {
            self.diags.error(
                format!("this lambda must take {} parameter(s), but has {}", expected_params.len(), params.len()),
                span,
            );
            return None;
        }
        // Parameter types must be concrete at the lambda boundary (a function signature can't carry
        // another function's inference variable), so resolve the element type now.
        let param_tys: Vec<Ty> = expected_params.iter().map(|t| self.finalize(*t)).collect();

        // Snapshot the enclosing scope (with finalized types) so a body reference to an enclosing
        // local can be captured. Finalizing now (enclosing inference still live) keeps the capture
        // parameter's type consistent with the enclosing local once both default the same way.
        let enclosing: Vec<(String, LocalId, Ty)> = self
            .scope
            .iter()
            .map(|(n, id)| (n.clone(), *id, self.finalize(self.locals[*id as usize].ty)))
            .collect();

        // Swap in fresh per-function state; the lambda is a separate function body.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_scope = std::mem::take(&mut self.scope);
        let saved_int_vars = std::mem::take(&mut self.int_vars);
        let saved_int_parent = std::mem::take(&mut self.int_parent);
        let saved_float_vars = std::mem::take(&mut self.float_vars);
        let saved_float_parent = std::mem::take(&mut self.float_parent);
        let saved_ret = self.ret_hint;
        let saved_arena = self.arena_depth;
        // A lambda body is a separate function: it is not lexically inside the enclosing
        // `task_group`, so reset the task-group / `wait`-state tracking (else a `wait()` inside the
        // lambda would set the enclosing group's flag at compile time and bypass the check).
        let saved_tg_depth = self.task_group_depth;
        let saved_wait_state = std::mem::take(&mut self.wait_state);
        let saved_tg_fallible = std::mem::take(&mut self.task_group_fallible);
        let saved_bases = std::mem::take(&mut self.slice_bases);
        let saved_capture = self.capture.take();
        self.ret_hint = expected_ret.unwrap_or(Ty::Unit);
        self.arena_depth = 0;
        self.task_group_depth = 0;
        self.capture = Some(CaptureScope { enclosing, captured: Vec::new() });

        let mut param_ids: Vec<LocalId> =
            params.iter().zip(&param_tys).map(|(p, ty)| self.declare(&p.name.name, *ty, false)).collect();
        let checked = self.check_block(body, expected_ret);
        let ret = match expected_ret {
            Some(t) => t,
            None => checked.value.as_ref().map(|v| v.ty).unwrap_or(Ty::Unit),
        };
        let mut body_fin = checked;
        self.finalize_block(&mut body_fin);

        // Collect captures: each becomes a trailing parameter of the lifted function, and the
        // enclosing local is passed at the call site. Slice ③ supports copy-value captures only.
        let captured = self.capture.take().unwrap().captured;
        let mut locals = std::mem::take(&mut self.locals);
        for l in &mut locals {
            l.ty = self.finalize(l.ty);
        }
        let mut capture_ops = Vec::new();
        for (cname, pid, enc_id) in &captured {
            let ty = locals[*pid as usize].ty;
            if ty_capture_is_move(ty, self.tuples) {
                self.diags.error(
                    format!("a lambda cannot capture the owned value '{cname}' yet (capture supports copy values like int/float/bool/char)"),
                    span,
                );
            }
            param_ids.push(*pid);
            capture_ops.push(Expr { kind: ExprKind::Local(*enc_id), ty, span });
        }

        let ret = self.finalize(ret);
        let name = format!("{}$lambda{}", self.cur_fn, self.lifted.len());
        self.lifted.push(hir::Fn {
            name: name.clone(),
            params: param_ids,
            ret,
            locals,
            body: body_fin,
            span,
            drop_locals: Vec::new(),
        });

        // Restore the enclosing function's state.
        self.locals = saved_locals;
        self.scope = saved_scope;
        self.int_vars = saved_int_vars;
        self.int_parent = saved_int_parent;
        self.float_vars = saved_float_vars;
        self.float_parent = saved_float_parent;
        self.ret_hint = saved_ret;
        self.arena_depth = saved_arena;
        self.task_group_depth = saved_tg_depth;
        self.wait_state = saved_wait_state;
        self.task_group_fallible = saved_tg_fallible;
        self.slice_bases = saved_bases;
        self.capture = saved_capture;
        // A lambda must not return a function value: the returned closure's environment is
        // frame-local to *this* lifted function and would dangle once it returns (the same rule as
        // a top-level fn — checked here too so a stage/value lambda can't slip a closure out).
        if matches!(ret, Ty::Fn(_)) {
            self.diags.error(
                "a lambda cannot return a function value (a closure's environment is frame-local)".to_string(),
                span,
            );
            return None;
        }
        Some((name, ret, capture_ops))
    }

    /// Resolve a reducer/terminal function argument — a named function or an inline lambda — given
    /// its expected parameter types and (optionally) return type. Returns the (possibly synthetic
    /// lifted) function name and its actual return type. A named function is validated against the
    /// expected signature; a lambda is lifted (`lift_lambda`). `label` names the operation for
    /// diagnostics. Used by `reduce`/`par_map`/`scan`/`partition`/`any`/`all` (the element/acc
    /// types are known after `check_pipeline`/the initial value).
    fn resolve_fn(&mut self, arg: &ast::Expr, expected_params: &[Ty], expected_ret: Option<Ty>, label: &str, span: Span) -> Option<(String, Ty, Vec<Expr>)> {
        if let ast::ExprKind::Lambda { params, body } = &arg.kind {
            return self.lift_lambda(params, body, expected_params, expected_ret, arg.span);
        }
        let Some(fname) = self.pipeline_fn_name(arg) else {
            self.diags.error(format!("'{label}' needs a function (named or `fn … {{ … }}`)"), span);
            return None;
        };
        let Some(sig) = self.sigs.get(&fname.name) else {
            self.diags.error(format!("undefined function: '{}'", fname.name), fname.span);
            return None;
        };
        let (params, ret) = (sig.params.clone(), sig.ret);
        // Resolve the expected types first: an unresolved inference variable (e.g. an inline
        // literal's element type) must not false-positive against the concrete signature.
        let expected_resolved: Vec<Ty> = expected_params.iter().map(|&t| self.resolve(t)).collect();
        if params.as_slice() != expected_resolved.as_slice() || expected_ret.is_some_and(|er| self.resolve(er) != ret) {
            let want_ret = self.resolve(expected_ret.unwrap_or(ret));
            self.diags.error(
                format!(
                    "'{}' must have type ({}) -> {} here",
                    fname.name,
                    expected_resolved.iter().map(|t| ty_name(*t)).collect::<Vec<_>>().join(", "),
                    ty_name(want_ret),
                ),
                fname.span,
            );
            return None;
        }
        Some((fname.name, ret, Vec::new()))
    }

    /// The `idx`-th parameter type of a *named* function argument, to seed an inline-literal source's
    /// element type. A lambda has no signature to peek (its parameters are inferred), so it yields
    /// `None` (the literal then defaults like any unconstrained value).
    fn named_param_hint(&self, arg: &ast::Expr, idx: usize) -> Option<Ty> {
        if matches!(arg.kind, ast::ExprKind::Lambda { .. }) {
            return None;
        }
        self.pipeline_fn_name(arg).and_then(|f| self.sigs.get(&f.name).cloned()).and_then(|s| s.params.get(idx).copied())
    }

    /// The signature of a *named* function argument (`None` for a lambda or an unresolved name) —
    /// used by `reduce`/`scan` to take the accumulator/element types from a named fold's signature.
    fn named_sig(&self, arg: &ast::Expr) -> Option<FnSig> {
        if matches!(arg.kind, ast::ExprKind::Lambda { .. }) {
            return None;
        }
        self.pipeline_fn_name(arg).and_then(|f| self.sigs.get(&f.name).cloned())
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
                        // An inline lambda (`map(fn x { … })`) or a named function.
                        let stage_fn = match arg {
                            Some(a) => match &a.kind {
                                ast::ExprKind::Lambda { params, body } => Some(StageFn::Lambda {
                                    params: params.clone(),
                                    body: body.clone(),
                                    span: a.span,
                                }),
                                _ => self.pipeline_fn_name(a).map(StageFn::Named),
                            },
                            None => None,
                        };
                        match stage_fn {
                            Some(f) if is_map => stages.push(RawStage::Map(f)),
                            Some(f) => stages.push(RawStage::Where(f)),
                            None => self.diags.error(
                                format!("'.{}()' needs a function (named or `fn … {{ … }}`) or `.field`", field.name),
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
            // A named first `map` fixes the element type from its parameter; a lambda's parameter
            // type is inferred (the literal defaults), so there is no hint to pull.
            Some(RawStage::Map(StageFn::Named(fname))) => self.sigs.get(&fname.name).and_then(|s| s.params.first().copied()),
            None => elem_expected_no_stages,
            _ => None,
        };
        let source = match &source_ast.kind {
            ast::ExprKind::ArrayLit(elems) => self.check_array_lit(elems, elem_expected, span),
            _ => self.check_expr(source_ast, None),
        };
        let mut elem = match source.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => scalar_to_ty(s),
            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => Ty::Struct(id),
            // An `array<slice<T>>` (a `chunks` result): each element is a `slice<T>` chunk —
            // the input to `chunks(n).par_map(f)`'s `f: (slice<T>) -> R`.
            Ty::DynSliceArray(p) => Ty::Slice(prim_to_scalar(p)),
            Ty::Error => return None,
            other => {
                self.diags
                    .error(format!("a pipeline source must be an array, got {}", ty_name(other)), span);
                return None;
            }
        };
        // MIR materializes a stack-array source only when it is an array literal or a named
        // local (slot-addressable); an arbitrary array-valued expression (e.g. an `if` or
        // block) would otherwise crash lowering. A `{ptr,len}` view (`slice`/owned array) is
        // fine as a value, but a dynamic `array<Struct>` must be a variable: its field
        // projection indexes through the buffer pointer (`IndexFieldPtr`), and binding it first
        // keeps the owned buffer alive across the loop. Reject other array shapes cleanly here.
        let needs_var = matches!(source.ty, Ty::Array(..) | Ty::StructArray(..) | Ty::DynStructArray(..));
        if needs_var && !matches!(source.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "a pipeline over an array must start from an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return None;
        }

        // Field projection / field-predicate stages index the source by element, which needs a
        // slot-backed stack array / struct array (`IndexField`) or a dynamic `array<Struct>`
        // view addressed through its buffer pointer (`IndexFieldPtr`, slice 8d-2). A scalar
        // `{ptr,len}` view (`slice` / owned scalar `array`) has no per-element struct to project.
        let slot_backed = matches!(source.ty, Ty::Array(..) | Ty::StructArray(..) | Ty::DynStructArray(..));
        let mut stages = Vec::new();
        // `.field` / `where(.field)` read the *source* element by index; after a `map` the logical
        // element is a computed value no longer in the source buffer, so those stages are only
        // valid before any `map`.
        let mut mapped = false;
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
                    // A struct element after a `map` is the map's (struct) result, not a source
                    // element — projection reads the source, so reject it (checked after the
                    // struct-type check so a non-struct gets the more fundamental diagnostic).
                    if mapped {
                        self.diags.error(
                            format!("'.{}' field projection after 'map' is not supported (map produces a computed value, not a source element)", field.name),
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
                RawStage::Map(sf) => {
                    // `map(f)` accepts a scalar element or a whole struct element: a struct array
                    // stays index-addressed until used, and a struct-consuming `map` loads the
                    // element by index in MIR (`lower_struct_elem`). The function (named or lambda)
                    // takes the current element type and returns the new one.
                    let Some((func, ret, captures)) = self.resolve_stage_fn(&sf, elem, false) else {
                        return None;
                    };
                    stages.push(Stage { kind: StageKind::Map { func, captures }, out_ty: ret });
                    elem = ret;
                    mapped = true;
                }
                RawStage::Where(sf) => {
                    // `where(f)` accepts a scalar element or a whole struct element (a multi-field
                    // predicate). A struct-consuming predicate loads the element by value in MIR
                    // (the same `lower_struct_elem` as `map`); `where` filters, so the element is
                    // unchanged (no `mapped`, and a later `.field` / `where(.field)` still reads the
                    // source).
                    let Some((func, _, captures)) = self.resolve_stage_fn(&sf, elem, true) else {
                        return None;
                    };
                    stages.push(Stage { kind: StageKind::Where { func, captures }, out_ty: elem });
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
                    // Same as projection: a struct element after a `map` is the map result, not a
                    // source element (struct-type check first so a non-struct reports that first).
                    if mapped {
                        self.diags.error(
                            format!("'where(.{})' after 'map' is not supported (map produces a computed value, not a source element)", field.name),
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

    /// `source.….min()` / `.max()` — the smallest / largest surviving (numeric scalar)
    /// element, as the element type. Like `sum`, it takes no arguments and an empty pipeline
    /// yields the fold identity (the type's extreme value).
    fn check_array_min_max(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, is_max: bool, span: Span) -> Expr {
        let name = if is_max { "max" } else { "min" };
        if !args.is_empty() {
            self.diags.error(format!("'{name}' takes no arguments"), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, elem)) = self.check_pipeline(recv, expected, span) else {
            return err;
        };
        if !elem.is_numeric() {
            self.diags
                .error(format!("'{name}' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArrayMinMax { source: Box::new(source), stages, is_max }, ty: elem, span }
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

    /// `source.….partition(p)` — split the surviving (scalar) elements into two owned arrays:
    /// those satisfying the predicate `p`, then the rest. Yields a tuple `(array<T>, array<T>)`,
    /// filled by one fused loop. The element must be a primitive scalar (the `array<T>` payload).
    fn check_array_partition(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags.error(
                format!("'partition' takes 1 argument (a predicate function), got {}", args.len()),
                span,
            );
            return err;
        };
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error(
                "'partition' over struct elements is not supported yet (project a field first)".to_string(),
                span,
            );
            return err;
        }
        // The predicate has type `(elem) -> bool` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[elem], Some(Ty::Bool), "partition", span) else {
            return err;
        };
        // The element must materialize into `array<T>`, i.e. be a primitive scalar.
        let prim_ok = ty_to_scalar(elem).and_then(scalar_to_prim).is_some();
        if !prim_ok {
            self.diags.error(
                format!("'partition' element must be a primitive scalar (int/float/bool/char), got {}", ty_name(elem)),
                span,
            );
            return err;
        }
        // Result: a tuple of two owned arrays `(array<T>, array<T>)`.
        let arr = ty_to_scalar(Ty::DynArray(ty_to_scalar(elem).unwrap())).expect("array<prim> is a payload scalar");
        let tuple_id = intern_tuple(self.tuples, vec![arr, arr]);
        Expr {
            kind: ExprKind::ArrayPartition { source: Box::new(source), stages, func, captures, elem },
            ty: Ty::Tuple(tuple_id),
            span,
        }
    }

    /// `arr.chunks(n)` — split an array/slice of a primitive scalar into length-`n` sub-slices
    /// (the last may be shorter), yielding an owned `array<slice<T>>` whose elements borrow `arr`.
    /// The result is region-tied to `arr` (the chunk slices view its storage).
    fn check_array_chunks(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [n_arg] = args else {
            self.diags.error(format!("'chunks' takes 1 argument (the chunk size), got {}", args.len()), span);
            return err;
        };
        let n = self.check_expr(n_arg, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if n.ty == Ty::Error {
            return err;
        }
        if !n.ty.is_int_like() {
            self.diags.error(format!("'chunks' size must be an integer, got {}", ty_name(n.ty)), n_arg.span);
            return err;
        }
        let src = self.check_expr(recv, None);
        let elem_scalar = match src.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => s,
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("'chunks' needs an array or slice, got {}", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        let Some(prim) = scalar_to_prim(elem_scalar) else {
            self.diags.error(
                format!("'chunks' element must be a primitive scalar (int/float/bool/char), got {}", ty_name(scalar_to_ty(elem_scalar))),
                span,
            );
            return err;
        };
        // A fixed stack array source must be a literal or a named local (slot-addressable, like a
        // pipeline source) so MIR can take its buffer address; a `{ptr,len}` view is fine as a value.
        if matches!(src.ty, Ty::Array(..)) && !matches!(src.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "'chunks' over a stack array must start from an array literal or a variable".to_string(),
                span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::ArrayChunks { source: Box::new(src), n: Box::new(n), elem: scalar_to_ty(prim_to_scalar(prim)) },
            ty: Ty::DynSliceArray(prim),
            span,
        }
    }

    /// `source.….par_map(f)` — apply the Pure function `f` to each surviving element and
    /// materialize the results into an owned `array<R>`. `f` must be Pure (checked later, over the
    /// whole call graph) and return a primitive scalar. The first cut runs sequentially.
    fn check_array_par_map(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags.error(
                format!("'par_map' takes 1 argument (a function), got {}", args.len()),
                span,
            );
            return err;
        };
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        // `f: (elem) -> R` (named or lambda); `R` is inferred.
        let Some((func, r, captures)) = self.resolve_fn(fn_arg, &[elem], None, "par_map", span) else {
            return err;
        };
        if r == Ty::Error {
            return err;
        }
        // The result must materialize into `array<R>`, i.e. be a primitive scalar.
        let Some(scalar) = ty_to_scalar(r).filter(|s| scalar_to_prim(*s).is_some()) else {
            self.diags.error(
                format!("'par_map' result must be a primitive scalar (int/float/bool/char), got {}", ty_name(r)),
                span,
            );
            return err;
        };
        Expr {
            kind: ExprKind::ArrayParMap { source: Box::new(source), stages, func, captures, elem: r },
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
        // The predicate's parameter type guides an inline source's element type (named only).
        let elem_hint = self.named_param_hint(fn_arg, 0);
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
        // Predicate must be `(elem) -> bool` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[elem], Some(Ty::Bool), name, span) else {
            return err;
        };
        Expr {
            kind: ExprKind::ArrayAnyAll { source: Box::new(source), stages, func, captures, all },
            ty: Ty::Bool,
            span,
        }
    }

    /// `src.…​.reduce(init, f)` — fold the post-stage elements with `f: (A, E) -> A`,
    /// starting from `init: A`.
    fn check_array_reduce(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [init_arg, fn_arg] = args else {
            self.diags.error(format!("'reduce' takes 2 arguments (an initial value and a function), got {}", args.len()), span);
            return err;
        };
        // The accumulator type + element hint: a named fold fixes both from its signature; a
        // lambda infers the accumulator from the initial value (and the element from the source).
        let named_sig = self.named_sig(fn_arg);
        let (acc_ty, elem_hint, init) = match &named_sig {
            Some(sig) => (sig.ret, sig.params.get(1).copied(), self.check_expr(init_arg, Some(sig.ret))),
            None => {
                let init = self.check_expr(init_arg, expected);
                (self.finalize(init.ty), None, init)
            }
        };
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        // A failed initial value leaves `acc_ty == Ty::Error`; bail before resolving the function
        // so it doesn't cascade into the lambda body / signature check (matching `scan`).
        if acc_ty == Ty::Error {
            return err;
        }
        // `f: (acc, elem) -> acc` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[acc_ty, elem], Some(acc_ty), "reduce", span) else {
            return err;
        };
        self.constrain(acc_ty, expected, span);
        Expr {
            kind: ExprKind::ArrayReduce { source: Box::new(source), stages, func, captures, init: Box::new(init) },
            ty: acc_ty,
            span,
        }
    }

    /// `source.….scan(init, f)` — a materializing prefix fold: emit the running accumulator
    /// after each surviving element, yielding an owned `array<A>`. `f: (A, E) -> A`, `init: A`.
    fn check_array_scan(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [init_arg, fn_arg] = args else {
            self.diags.error(format!("'scan' takes 2 arguments (an initial value and a function), got {}", args.len()), span);
            return err;
        };
        // Accumulator type + element hint: a named fold fixes both from its signature; a lambda
        // infers the accumulator from the initial value (and the element from the source).
        let named_sig = self.named_sig(fn_arg);
        let (acc_ty, elem_hint, init) = match &named_sig {
            Some(sig) => (sig.ret, sig.params.get(1).copied(), self.check_expr(init_arg, Some(sig.ret))),
            None => {
                let init = self.check_expr(init_arg, None);
                (self.finalize(init.ty), None, init)
            }
        };
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        // A failed initial value leaves `acc_ty == Ty::Error`; bail before the scalar check so it
        // doesn't cascade into a confusing "accumulator must be a scalar" diagnostic (matching reduce).
        if acc_ty == Ty::Error {
            return err;
        }
        // A struct element must be projected to a scalar first (the fused loop has no scalar
        // value loaded for a struct array, like `map`/`to_array`).
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error(
                "'scan' over struct elements is not supported yet (project a field first)".to_string(),
                span,
            );
            return err;
        }
        // The accumulator (output element) must be a *primitive* scalar to materialize into
        // `array<A>`. `ty_to_scalar` accepts `Ty::Struct` (a valid Option/Result payload), but
        // the buffer/PtrStore path has no struct-element support, so reject structs explicitly.
        if matches!(acc_ty, Ty::Struct(_)) {
            self.diags.error(
                "'scan' accumulator must be a primitive scalar (struct accumulators are not supported yet)".to_string(),
                span,
            );
            return err;
        }
        let Some(scalar) = ty_to_scalar(acc_ty) else {
            self.diags.error(
                format!("'scan' accumulator must be a scalar to materialize, got {}", ty_name(acc_ty)),
                span,
            );
            return err;
        };
        // `f: (acc, elem) -> acc` (named or lambda).
        let Some((func, _, captures)) = self.resolve_fn(fn_arg, &[acc_ty, elem], Some(acc_ty), "scan", span) else {
            return err;
        };
        Expr {
            kind: ExprKind::ArrayScan { source: Box::new(source), stages, func, captures, init: Box::new(init), elem: acc_ty },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `source.….sort()` — materialize the surviving elements into an owned `array<T>` and sort
    /// them ascending. First cut: numeric scalar elements only (an ordering exists), no
    /// comparator argument (a `sort(cmp)` overload is a follow-up).
    fn check_array_sort(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'sort' takes no arguments yet (a comparator overload is a follow-up)".to_string(), span);
        }
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some((source, stages, elem)) = self.check_pipeline(recv, None, span) else {
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'sort' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        if !elem.is_numeric() {
            self.diags.error(format!("'sort' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        let Some(scalar) = ty_to_scalar(elem) else {
            self.diags.error(format!("'sort' needs a scalar element, got {}", ty_name(elem)), span);
            return err;
        };
        Expr {
            kind: ExprKind::ArraySort { source: Box::new(source), stages, elem },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `source.….sort_by_key(f)` — materialize the surviving (primitive scalar) elements and sort
    /// them ascending by `f(element)`. Unlike `sort`, the element need not be numeric (it is ordered
    /// by the key); the key `f` must return an orderable scalar (int/float/char). `f` may be a named
    /// function or a lambda (which may capture).
    fn check_array_sort_by_key(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [fn_arg] = args else {
            self.diags.error(format!("'sort_by_key' takes 1 argument (a key function), got {}", args.len()), span);
            return err;
        };
        let elem_hint = self.named_param_hint(fn_arg, 0);
        let Some((source, stages, elem)) = self.check_pipeline(recv, elem_hint, span) else {
            return err;
        };
        if matches!(elem, Ty::Struct(_)) {
            self.diags.error("'sort_by_key' over struct elements is not supported yet (project a field first)".to_string(), span);
            return err;
        }
        // The element must materialize into `array<T>`, i.e. be a primitive scalar.
        let Some(scalar) = ty_to_scalar(elem).filter(|s| scalar_to_prim(*s).is_some()) else {
            self.diags.error(
                format!("'sort_by_key' element must be a primitive scalar (int/float/bool/char), got {}", ty_name(elem)),
                span,
            );
            return err;
        };
        // The key function `f: (elem) -> K`; `K` must be an orderable scalar.
        let Some((key_func, key_ty, captures)) = self.resolve_fn(fn_arg, &[elem], None, "sort_by_key", span) else {
            return err;
        };
        if key_ty == Ty::Error {
            return err;
        }
        if !(key_ty.is_numeric() || key_ty == Ty::Char) {
            self.diags.error(
                format!("'sort_by_key' key must be an orderable scalar (int/float/char), got {}", ty_name(key_ty)),
                span,
            );
            return err;
        }
        Expr {
            kind: ExprKind::ArraySortBy { source: Box::new(source), stages, key_func, captures, key_ty, elem },
            ty: Ty::DynArray(scalar),
            span,
        }
    }

    /// `a.dot(b)` — the inner product `Σ a[i]*b[i]`. First cut: both operands must be
    /// fixed-length arrays of the same numeric scalar element and the same statically known
    /// length (the SIMD/vector case; `slice`/`array<T>` dot with runtime lengths is a follow-up).
    fn check_array_dot(&mut self, recv: &ast::Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let [b_arg] = args else {
            self.diags.error(format!("'dot' takes 1 argument (another array), got {}", args.len()), span);
            return err;
        };
        // The receiver must be a bare fixed array — no pipeline stages on the left yet.
        let Some((a_src, stages, elem)) = self.check_pipeline(recv, expected, span) else {
            return err;
        };
        if !stages.is_empty() {
            self.diags.error("'dot' does not support map/where stages yet".to_string(), span);
            return err;
        }
        let na = match a_src.ty {
            Ty::Array(_, n) => n,
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("'dot' needs a fixed-length array on the left, got {} (slice/array<T> dot is not supported yet)", ty_name(other)),
                    span,
                );
                return err;
            }
        };
        if !elem.is_numeric() {
            self.diags.error(format!("'dot' needs a numeric element type, got {}", ty_name(elem)), span);
            return err;
        }
        // No type hint for `b`: passing `a`'s full array type would make a length mismatch
        // produce a duplicate "array[m] vs array[n]" error on top of the clearer one below.
        // The element-type and length checks here cover correctness.
        let b = self.check_expr(b_arg, None);
        // MIR materializes both operands via `array_source_slot`, which only handles a literal
        // or a local (the M4 restriction). Reject an arbitrary array expression (an `if`, a
        // call, a block, …) here so it cannot reach lowering and panic — mirrors `check_pipeline`'s
        // restriction on the left operand.
        if !matches!(b.ty, Ty::Error) && !matches!(b.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "the right operand of 'dot' must be an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                b.span,
            );
            return err;
        }
        let (nb, b_elem) = match b.ty {
            Ty::Array(s, n) => (n, scalar_to_ty(s)),
            Ty::Error => return err,
            other => {
                self.diags.error(
                    format!("'dot' needs a fixed-length array on the right, got {}", ty_name(other)),
                    b.span,
                );
                return err;
            }
        };
        if b_elem != elem {
            self.diags.error(
                format!("'dot' operands must have the same element type, got {} and {}", ty_name(elem), ty_name(b_elem)),
                b.span,
            );
            return err;
        }
        if na != nb {
            self.diags.error(
                format!("'dot' operands must have the same length, got {na} and {nb}"),
                b.span,
            );
            return err;
        }
        self.constrain(elem, expected, span);
        Expr { kind: ExprKind::ArrayDot { a: Box::new(a_src), b: Box::new(b), elem }, ty: elem, span }
    }

    /// `r.map_err(f)` — convert a `Result<T, E>`'s error with `f: fn(E) -> E'`, yielding
    /// `Result<T, E'>` (`Ok` passes through). The explicit, visible way to change a result's error
    /// type — Align has no implicit `?` conversion (that would be a hidden coercion).
    fn check_map_err(&mut self, recv: Expr, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Ty::Result(ok, e) = self.resolve(recv.ty) else {
            return err; // guarded by the caller
        };
        if args.len() != 1 {
            self.diags.error(format!("'map_err' takes 1 argument, got {}", args.len()), span);
            return err;
        }
        let f = self.check_expr(&args[0], None);
        let Ty::Fn(fid) = self.resolve(f.ty) else {
            if f.ty != Ty::Error {
                self.diags.error(format!("'map_err' expects a function `fn(E) -> E'`, got {}", ty_name(f.ty)), args[0].span);
            }
            return err;
        };
        let (params, e2) = {
            let ft = &self.fn_types[fid as usize];
            (ft.params.clone(), ft.ret)
        };
        if params.as_slice() != [e] {
            self.diags.error(
                format!("'map_err' function must take the error type {} (got {})", scalar_name(e), ty_name(f.ty)),
                args[0].span,
            );
            return err;
        }
        let ty = Ty::Result(ok, e2);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::ResultMapErr { result: Box::new(recv), f: Box::new(f) }, ty, span }
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
            // `str.clone()` deep-copies into a free-standing heap-owned `string` (MMv2 slice 7).
            // Unlike `box.clone`, it needs no arena: the result owns its buffer and is `Drop`-freed,
            // so it can outlive any region — this is how a zero-copy view escapes. (Arena-bump
            // cloning, the in-arena optimization, is a later sub-slice.)
            Ty::Str | Ty::String => Expr { kind: ExprKind::StrClone(Box::new(recv)), ty: Ty::String, span },
            Ty::Error => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.clone()' is available on box<T>, str, and string, got {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
    }

    /// `builder()` — open an append-oriented string builder (MMv2 slice 7c, draft.md §12).
    fn check_builder_new(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags
                .error(format!("'builder' takes no arguments, got {}", args.len()), span);
        }
        Expr { kind: ExprKind::BuilderNew, ty: Ty::Builder, span }
    }

    /// `b.write(s)` / `b.write_int(n)` / `b.write_bool(v)` / `b.write_char(c)` /
    /// `b.write_float(x)` — append to a builder (MMv2 slice 7c/7d). The builder is borrowed
    /// (mutated through its handle, not consumed). Each writer takes the matching scalar; `write`
    /// takes a `str` (a `string` borrows as one — zero-cost, non-consuming, reuses the slice-7b
    /// borrow, so `b.write(owned_string)` keeps it usable). `write_int` widens to `i64` at codegen,
    /// like `print`; `write_float` accepts `f32`/`f64` (codegen picks the runtime fn by width).
    fn check_builder_write(&mut self, recv: &ast::Expr, args: &[ast::Expr], kind: BuilderWriteKind, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let mname = builder_write_method_name(kind);
        let recv_expr = self.check_expr(recv, None);
        if recv_expr.ty != Ty::Builder {
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.{mname}()' is a builder method, got {}", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        if args.len() != 1 {
            self.diags
                .error(format!("'.{mname}()' takes 1 argument, got {}", args.len()), span);
            return err;
        }
        let mut arg = self.check_expr(&args[0], None);
        if arg.ty == Ty::Error {
            return err;
        }
        // `write` accepts a `str`; a `string` borrows as one (zero-cost, non-consuming — reuses
        // the slice-7b borrow), so `b.write(owned_string)` keeps `owned_string` usable.
        if kind == BuilderWriteKind::Str && arg.ty == Ty::String {
            let s = arg.span;
            arg = Expr { kind: ExprKind::StrBorrow(Box::new(arg)), ty: Ty::Str, span: s };
        }
        let (ok, want) = match kind {
            BuilderWriteKind::Str => (arg.ty == Ty::Str, "a str"),
            BuilderWriteKind::Int => (matches!(arg.ty, Ty::Int(_) | Ty::IntVar(_)), "an integer"),
            BuilderWriteKind::Bool => (arg.ty == Ty::Bool, "a bool"),
            BuilderWriteKind::Char => (arg.ty == Ty::Char, "a char"),
            BuilderWriteKind::Float => (matches!(arg.ty, Ty::Float(_) | Ty::FloatVar(_)), "a float"),
        };
        if !ok {
            self.diags
                .error(format!("'.{mname}()' expects {want}, got {}", ty_name(arg.ty)), arg.span);
            return err;
        }
        Expr {
            kind: ExprKind::BuilderWrite { builder: Box::new(recv_expr), arg: Box::new(arg), kind },
            ty: Ty::Unit,
            span,
        }
    }

    /// `b.to_string()` — finish a builder into an **owned** `string`, consuming (moving) it.
    fn check_builder_to_string(&mut self, recv: &ast::Expr, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let recv_expr = self.check_expr(recv, None);
        if recv_expr.ty != Ty::Builder {
            if recv_expr.ty != Ty::Error {
                self.diags
                    .error(format!("'.to_string()' is a builder method, got {}", ty_name(recv_expr.ty)), span);
            }
            return err;
        }
        if !args.is_empty() {
            self.diags
                .error(format!("'.to_string()' takes no arguments, got {}", args.len()), span);
        }
        Expr { kind: ExprKind::BuilderToString(Box::new(recv_expr)), ty: Ty::String, span }
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
        // A box payload must be a true (owned) *primitive* scalar. Resolve the payload scalar
        // first, then reject the non-primitive ones at the scalar level so every shape is caught
        // consistently — including an *un-annotated* `heap.new(move_value)` (the `box<…>`
        // annotation path is guarded in `resolve_type`, but inference here must reject the same
        // set or codegen's `scalar_bytes` hits `unreachable!`): a Move scalar (`string`/`array`),
        // a `Struct` (codegen can't size a struct box), or a `str` view (not boxable).
        let scalar = self.payload_scalar(arg.ty, args[0].span);
        let reject = match scalar {
            _ if scalar.is_move() => Some(format!("an owned `{}` cannot be boxed", scalar_name(scalar))),
            Scalar::Struct(_) => Some("struct boxes are not supported".to_string()),
            Scalar::Enum(_) => Some("sum-type boxes are not supported".to_string()),
            Scalar::Str => Some("a `str` view is not boxable".to_string()),
            _ => None,
        };
        if let Some(why) = reject {
            self.diags
                .error(format!("a box payload must be a primitive scalar ({why})"), args[0].span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
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
    /// (a `Result<T, _>`, e.g. from `let u: T := json.decode(d)?` — the type flows back
    /// through `?`). There is deliberately no `<T>` call syntax: Align has no
    /// expression-position type-argument form (no turbofish — `open-questions.md` Settled,
    /// `impl/02-frontend.md` §8); the annotate-the-binding error below is the fallback when
    /// context gives no type. M5 cut: a flat struct of `i64`/`i32`/`bool`/`str` fields
    /// (float/nested later).
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
            // `array<T>` target (MMv2 slice 8c): parse a JSON array of scalars into an *owned*
            // `array<T>` (elements copied → `Static`/returnable, not region-tied to the input).
            Some(Ty::Result(Scalar::DynArray(prim), _)) => {
                let elem = scalar_to_ty(prim_to_scalar(prim));
                // The element must be runtime-parseable. A `str` element would be a zero-copy
                // view region-tied to the input (deferred — needs the array to carry that region).
                if !matches!(elem, Ty::Int(_) | Ty::Float(_) | Ty::Bool) {
                    self.diags.error(
                        format!("'json.decode' into array<{}> is not supported yet (int/float/bool elements only)", ty_name(elem)),
                        span,
                    );
                    return err;
                }
                // `check_str_init` accepts a `str` or auto-borrows an owned `string` (the result
                // is copied, so the input's region does not constrain it), and reports a mismatch.
                let input = self.check_str_init(&args[0]);
                return Expr {
                    kind: ExprKind::JsonDecodeArray { elem, input: Box::new(input) },
                    ty: Ty::Result(Scalar::DynArray(prim), Scalar::Enum(self.error_enum_id)),
                    span,
                };
            }
            // `array<Struct>` target (MMv2 slice 8d, the draft.md §19 headline): parse a JSON
            // array of objects into an owned, dynamic AoS. Each element decodes like the single
            // struct path; `str` fields are zero-copy views into the input, so the whole array is
            // region-tied to that input (see `region_of`) and cannot escape it.
            Some(Ty::Result(Scalar::DynStructArray(id), _)) => {
                if !self.decode_struct_fields_ok(id, span) {
                    return err;
                }
                // The input region bounds the result (its `str` fields borrow the input), so use
                // `check_str_init` — a borrowed owned `string`'s region then bounds the array.
                let input = self.check_str_init(&args[0]);
                return Expr {
                    kind: ExprKind::JsonDecodeStructArray { struct_id: id, input: Box::new(input) },
                    ty: Ty::Result(Scalar::DynStructArray(id), Scalar::Enum(self.error_enum_id)),
                    span,
                };
            }
            _ => {
                self.diags.error(
                    "cannot infer the decode target type; annotate the binding, e.g. `u: T := json.decode(d)?`".to_string(),
                    span,
                );
                return err;
            }
        };
        if !self.decode_struct_fields_ok(sid, span) {
            return err;
        }
        // The decoded struct's `str` fields are zero-copy views into the input, so the input's
        // region constrains the result (see `region_of`). `check_str_init` accepts a `str` or
        // auto-borrows an owned `string` (whose region then bounds the decoded value).
        let input = self.check_str_init(&args[0]);
        Expr {
            kind: ExprKind::JsonDecode { struct_id: sid, input: Box::new(input) },
            ty: Ty::Result(Scalar::Struct(sid), Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// Validate that struct `sid`'s fields are all `json.decode`-able (int / float / bool, or a
    /// `str` zero-copy view into the input). Reports the first offending field and returns false.
    /// Shared by the single-struct and `array<Struct>` decode paths (MMv2 slice 8d).
    fn decode_struct_fields_ok(&mut self, sid: u32, span: Span) -> bool {
        let fields = self.structs[sid as usize].fields.clone();
        for f in &fields {
            if !matches!(f.ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Str) {
                self.diags.error(
                    format!("'json.decode' field '{}' has type {} (only int/float/bool/str decode for now)", f.name, ty_name(f.ty)),
                    span,
                );
                return false;
            }
        }
        true
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
            Ty::Str | Ty::String | Ty::Slice(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) => Expr { kind: ExprKind::Len(Box::new(r)), ty: i64_ty, span },
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

    /// `recv[index]` — element access. M5/MMv2 cut: a scalar `array`/`slice`/owned `array<T>`
    /// (the element is a scalar, copied out); the bounds check + abort is emitted in MIR. Indexing
    /// a struct array (whole-element load) and `str` byte indexing are deferred.
    fn check_index(&mut self, recv: &ast::Expr, index: &ast::Expr, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let r = self.check_expr(recv, None);
        // The index is an `i64` (matching `.len()` and loop counters). A non-integer index must
        // bail with `Ty::Error` — returning a typed `Index` node with a bad index would feed a
        // non-int operand into the MIR bounds-check `icmp` and panic codegen.
        let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if i.ty == Ty::Error {
            return err;
        }
        if !i.ty.is_int_like() {
            self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
            return err;
        }
        let elem = match r.ty {
            Ty::Array(s, _) | Ty::Slice(s) | Ty::DynArray(s) => scalar_to_ty(s),
            // Indexing an `array<slice<T>>` (a `chunks` result) yields one chunk `slice<T>`.
            Ty::DynSliceArray(p) => Ty::Slice(prim_to_scalar(p)),
            // Indexing a struct array yields the whole struct by value (a copy). A struct is Copy
            // (primitive / `str` fields), so the copy transfers no ownership; if it holds `str`
            // views, the value is region-tied to the array (handled by `region_of`, which inherits
            // the receiver's region for an `Index`).
            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => Ty::Struct(id),
            Ty::Error => return err,
            other => {
                self.diags.error(format!("cannot index {} (only array / slice / owned array)", ty_name(other)), span);
                return err;
            }
        };
        // A Move-only element (e.g. `array<string>`, `array<array<T>>`) cannot be indexed yet:
        // the load copies the element's `{ptr,len}` without transferring ownership, so the array
        // and the copy would both free the same buffer (double-free). Such element reads need a
        // borrow / move-out design (a later slice) — reject cleanly until then.
        if matches!(elem, Ty::Box(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::String | Ty::Builder) || payload_is_move(elem) {
            self.diags.error(
                format!("indexing an array of the Move type {} is not supported yet (it would copy the element without transferring ownership)", ty_name(elem)),
                span,
            );
            return err;
        }
        // A slot-backed fixed array must be a literal or a variable (same restriction as a
        // pipeline source — MIR addresses it through a slot). A `{ptr,len}` view is fine as a value.
        if matches!(r.ty, Ty::Array(..) | Ty::StructArray(..)) && !matches!(r.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "indexing a fixed array requires an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return err;
        }
        Expr { kind: ExprKind::Index { recv: Box::new(r), index: Box::new(i) }, ty: elem, span }
    }

    /// `fs.read_file(path)` — read the whole file at `path` (a `str`) into a freshly heap-allocated
    /// owned `string`, yielding `Result<string, Error>`. The returned `string` owns its buffer
    /// (freed by the binding's `Drop`); an I/O error is `Err`. The first `std.fs` surface (the
    /// `std.io`/zero-copy work is later) — a builtin, dispatched like `json.decode`.
    fn check_fs_read_file(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'fs.read_file' expects 1 argument (the path), got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        // The path is a `str` (or an owned `string`, auto-borrowed).
        let path = self.check_str_init(&args[0]);
        Expr {
            kind: ExprKind::FsReadFile { path: Box::new(path) },
            ty: Ty::Result(Scalar::String, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }

    /// `io.stdout.write(s)` — write the bytes of a `str` (or owned `string`, auto-borrowed) to
    /// stdout with **no** trailing newline (unlike `print`), yielding `Result<(), Error>` (an I/O
    /// failure is `Err`). The first `std.io` surface; a builtin like `fs.read_file`.
    fn check_io_stdout_write(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        if args.len() != 1 {
            self.diags
                .error(format!("'io.stdout.write' expects 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let result_ty = Ty::Result(Scalar::Unit, Scalar::Enum(self.error_enum_id));
        // The argument is written as bytes: a `builder` (its accumulated bytes, written directly —
        // no `to_string()` materialization), or a `str` / owned `string` (auto-borrowed). The
        // builder is *borrowed* (not consumed), so it is still usable / dropped normally after.
        let arg0 = self.check_expr(&args[0], None);
        let kind = match arg0.ty {
            Ty::Builder => ExprKind::IoStdoutWriteBuilder { builder: Box::new(arg0) },
            // Replicates `check_str_init`: borrow an owned `string` as a `str`; constrain anything
            // else to `str`.
            Ty::String => {
                let span = arg0.span;
                ExprKind::IoStdoutWrite { arg: Box::new(Expr { kind: ExprKind::StrBorrow(Box::new(arg0)), ty: Ty::Str, span }) }
            }
            Ty::Str | Ty::Error => ExprKind::IoStdoutWrite { arg: Box::new(arg0) },
            other => {
                self.constrain(other, Some(Ty::Str), args[0].span);
                ExprKind::IoStdoutWrite { arg: Box::new(arg0) }
            }
        };
        Expr { kind, ty: result_ty, span }
    }

    /// `arr[index].field` — field access on a struct-array element (MMv2 slice 8f). Fused into one
    /// bounds-checked element-field load; only the field (a scalar or a `str` view) is read. The
    /// result inherits the array's region (a `str` field views the array's input), so it cannot
    /// escape that input.
    fn check_index_field(&mut self, arr: &ast::Expr, index: &ast::Expr, field: &ast::Ident, expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Local(u32::MAX), ty: Ty::Error, span };
        let r = self.check_expr(arr, None);
        let i = self.check_expr(index, Some(Ty::Int(IntTy { bits: 64, signed: true })));
        if i.ty == Ty::Error {
            return err;
        }
        if !i.ty.is_int_like() {
            self.diags.error(format!("an array index must be an integer, got {}", ty_name(i.ty)), index.span);
            return err;
        }
        let struct_id = match r.ty {
            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
            Ty::Error => return err,
            other => {
                self.diags.error(format!("'{}[i].{}' needs a struct array, got {}", "arr", field.name, ty_name(other)), span);
                return err;
            }
        };
        // A fixed `array<Struct>` slot must be a literal or a variable (same restriction as a
        // pipeline source — MIR addresses it through a slot). A `{ptr,len}` view is fine as a value.
        if matches!(r.ty, Ty::StructArray(..)) && !matches!(r.kind, ExprKind::ArrayLit { .. } | ExprKind::Local(_)) {
            self.diags.error(
                "indexing a fixed array requires an array literal or a variable (an arbitrary array expression is not supported yet)".to_string(),
                span,
            );
            return err;
        }
        let (field_index, field_ty) = match self.field_of(Ty::Struct(struct_id), &field.name, field.span) {
            Some(x) => x,
            None => return err,
        };
        // A field-read that is itself a Move type would copy without ownership transfer (the same
        // double-free concern as scalar indexing). Decoded structs only have scalar / `str`-view
        // fields, but guard generally.
        if matches!(field_ty, Ty::Box(_) | Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::String | Ty::Builder) || payload_is_move(field_ty) {
            self.diags.error(
                format!("reading a Move-type field {} out of an array element is not supported yet", ty_name(field_ty)),
                span,
            );
            return err;
        }
        self.constrain(field_ty, expected, span);
        Expr {
            kind: ExprKind::ElemField { recv: Box::new(r), index: Box::new(i), field: field_index, struct_id },
            ty: field_ty,
            span,
        }
    }

    /// `b.get()` — copy the value out of a `box<T>`.
    fn check_box_get(&mut self, recv: Expr, recv_ty: Ty, args: &[ast::Expr], span: Span) -> Expr {
        if !args.is_empty() {
            self.diags.error("'get' takes no arguments".to_string(), span);
        }
        match recv_ty {
            Ty::Box(s) => Expr { kind: ExprKind::BoxGet(Box::new(recv)), ty: scalar_to_ty(s), span },
            // `task.get()` — read a spawned task's result (`task_group`, slice ④). The result is
            // only computed after `wait()` joins, so `get()` before `wait()` reads an uncomputed
            // slot — rejected (the result is guaranteed ready only if a `wait()` dominates here).
            Ty::Task(s) => {
                if !self.wait_state.last().copied().unwrap_or(false) {
                    let msg = if self.task_group_fallible.last().copied().unwrap_or(false) {
                        // A fallible group: a bare `wait()` ignores the error; only `wait()?` makes
                        // the results safe to read.
                        "cannot call '.get()' before a successful 'wait()?' — this task_group is fallible, so use 'wait()?' to join (its error propagates) before reading results"
                    } else {
                        "cannot call '.get()' before 'wait()' — a task's result is ready only after the group is joined"
                    };
                    self.diags.error(msg.to_string(), span);
                }
                Expr { kind: ExprKind::TaskGet(Box::new(recv)), ty: scalar_to_ty(s), span }
            }
            Ty::Error => Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span },
            other => {
                self.diags
                    .error(format!("'.get()' is only available on box<T> or Task<R>, got {}", ty_name(other)), span);
                Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span }
            }
        }
    }

    /// Builtin `error(code)` — sugar for `Error.Code(code)` (the generic error category).
    fn check_error_ctor(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let ty = Ty::Enum(self.error_enum_id);
        if args.len() != 1 {
            self.diags.error(format!("'error' takes 1 argument, got {}", args.len()), span);
            return Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        }
        let arg = self.check_expr(&args[0], Some(Ty::Int(IntTy { bits: 32, signed: true })));
        Expr {
            kind: ExprKind::EnumValue { enum_id: self.error_enum_id, variant: ERROR_VARIANT_CODE, payload: vec![arg] },
            ty,
            span,
        }
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
        // `wait()?` on a fallible task_group: control only continues past the `?` if no task failed
        // (the `Err` was propagated), so every task succeeded → `get()` is now safe (slice ④c-2).
        // Recognised when `?` is applied directly to `wait()` (`wait()?`, also `w := wait()?`);
        // binding the raw `Result` first and unwrapping the local later (`w := wait(); w?`) is a
        // sound over-restriction — `get()` would still be rejected. (Indirect unwrap is a later,
        // local-tracking refinement.)
        if matches!(v.kind, ExprKind::Wait) {
            if let Some(w) = self.wait_state.last_mut() {
                *w = true;
            }
        }
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
        // The fallback runs only on `None`, so its `wait()`/`spawn()` must not leak into the
        // post-unwrap `wait`-state (slice ④c) — snapshot here and restore after the fallback.
        let w_snapshot = self.wait_state.last().copied();
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
        // Dominance merge: the `Some` path skips the fallback (state `w`), the `None` path runs it
        // (current state). After the unwrap, a `wait()` is guaranteed only if both held — `w &&
        // current` — so a conditional `spawn` in the fallback correctly clears the flag.
        if let (Some(w), Some(top)) = (w_snapshot, self.wait_state.last_mut()) {
            *top = w && *top;
        }
        self.constrain(payload, expected, span);
        Expr { kind: ExprKind::ElseUnwrap { opt: Box::new(o), fallback: Box::new(fb) }, ty: payload, span }
    }

    /// `Type.Variant(args)` — construct a sum-type value with a payload. Checks the argument count
    /// and each argument against the variant's payload scalar.
    fn check_variant_ctor(&mut self, enum_id: u32, field: &ast::Ident, args: &[ast::Expr], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let Some(idx) = self.enums[enum_id as usize].variants.iter().position(|v| v.name == field.name) else {
            self.diags.error(format!("'{}' is not a variant of '{}'", field.name, self.enums[enum_id as usize].name), span);
            return err;
        };
        let payload = self.enums[enum_id as usize].variants[idx].payload.clone();
        if args.len() != payload.len() {
            self.diags.error(
                format!("variant '{}' takes {} argument(s), got {}", field.name, payload.len(), args.len()),
                span,
            );
            return err;
        }
        let checked: Vec<Expr> = args
            .iter()
            .zip(&payload)
            .map(|(a, &s)| {
                let pt = scalar_to_ty(s);
                let e = self.check_expr(a, Some(pt));
                if e.ty != Ty::Error && self.resolve(e.ty) != pt {
                    self.diags.error(format!("payload type mismatch: expected {}, got {}", ty_name(pt), ty_name(e.ty)), e.span);
                }
                e
            })
            .collect();
        let ty = Ty::Enum(enum_id);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::EnumValue { enum_id, variant: idx as u32, payload: checked }, ty, span }
    }

    /// The variants of a matchable type: a user sum type, or the builtin `Option`/`Result`
    /// (so `match` works on them too). Each variant is `(name, positional payload scalars)`, in
    /// the order the lowering expects (Option: 0 = Some, 1 = None; Result: 0 = Ok, 1 = Err).
    fn match_variants(&self, ty: Ty) -> Option<(String, Vec<(String, Vec<Scalar>)>)> {
        match ty {
            Ty::Enum(id) => {
                let e = &self.enums[id as usize];
                Some((e.name.clone(), e.variants.iter().map(|v| (v.name.clone(), v.payload.clone())).collect()))
            }
            Ty::Option(s) => Some(("Option".into(), vec![("Some".into(), vec![s]), ("None".into(), Vec::new())])),
            Ty::Result(o, e) => Some(("Result".into(), vec![("Ok".into(), vec![o]), ("Err".into(), vec![e])])),
            _ => None,
        }
    }

    /// `match scrutinee { Variant => body, _ => body }` — exhaustive match over a sum type (a user
    /// `enum`, or builtin `Option`/`Result`). Each arm's body unifies to the match's type; every
    /// variant must be covered, or a `_` wildcard.
    fn check_match(&mut self, scrutinee: &ast::Expr, arms: &[ast::MatchArm], expected: Option<Ty>, span: Span) -> Expr {
        let err = Expr { kind: ExprKind::Bool(false), ty: Ty::Error, span };
        let s = self.check_expr(scrutinee, None);
        if s.ty == Ty::Error {
            return err;
        }
        let Some((type_name, variants)) = self.match_variants(self.resolve(s.ty)) else {
            self.diags.error(format!("`match` expects a sum type, got {}", ty_name(s.ty)), scrutinee.span);
            return err;
        };
        let mut covered = vec![false; variants.len()];
        let mut has_wildcard = false;
        let mut checked: Vec<hir::MatchArm> = Vec::with_capacity(arms.len());
        // The match's value type: unify all arm bodies (drives inference from `expected`).
        let mut result_ty: Option<Ty> = expected;
        for arm in arms {
            // Payload bindings are scoped to this arm only — snapshot the scope and restore after.
            let scope_mark = self.scope.len();
            // Mark a variant covered, diagnosing a duplicate; returns its tag index, or None if the
            // name is not a variant of the scrutinee type (a hard error).
            let mut cover = |me: &mut Self, name: &ast::Ident| -> Option<u32> {
                match variants.iter().position(|(vn, _)| vn == &name.name) {
                    Some(idx) => {
                        if covered[idx] {
                            me.diags.error(format!("duplicate arm for variant '{}'", name.name), name.span);
                        }
                        covered[idx] = true;
                        Some(idx as u32)
                    }
                    None => {
                        me.diags.error(format!("'{}' is not a variant of '{}'", name.name, type_name), name.span);
                        None
                    }
                }
            };
            let (variant_tags, bindings) = match &arm.pattern {
                ast::MatchPattern::Wildcard(_) => {
                    if has_wildcard {
                        self.diags.error("duplicate `_` arm".to_string(), arm.span);
                    }
                    has_wildcard = true;
                    (Vec::new(), Vec::new())
                }
                ast::MatchPattern::Or { variants: names, .. } => {
                    // Bare variant names, no bindings. A payload variant may appear (its payload is
                    // not bound). Each must be a real, not-yet-covered variant.
                    let tags = names
                        .iter()
                        .filter_map(|n| cover(self, n))
                        .collect::<Vec<_>>();
                    if tags.len() != names.len() {
                        return err;
                    }
                    (tags, Vec::new())
                }
                ast::MatchPattern::Variant { name, bindings } => {
                    let Some(idx) = cover(self, name) else { return err };
                    let payload = &variants[idx as usize].1;
                    if bindings.len() != payload.len() {
                        self.diags.error(
                            format!("variant '{}' binds {} value(s), got {}", name.name, payload.len(), bindings.len()),
                            arm.span,
                        );
                    }
                    // Declare each binding (typed by the matching payload scalar) so the arm
                    // body resolves even when the count is wrong. Binding names must be
                    // distinct — `Rect(w, w)` would otherwise silently shadow.
                    let mut seen_bindings = std::collections::HashSet::new();
                    let locals = bindings
                        .iter()
                        .enumerate()
                        .map(|(i, b)| {
                            if !seen_bindings.insert(&b.name) {
                                self.diags.error(format!("duplicate binding '{}' in pattern", b.name), b.span);
                            }
                            let ty = payload.get(i).map(|&s| scalar_to_ty(s)).unwrap_or(Ty::Error);
                            self.declare(&b.name, ty, false)
                        })
                        .collect();
                    (vec![idx], locals)
                }
            };
            // Each arm body is checked against the running result type, so the constraint (and any
            // mismatch error) comes from `check_expr`; the first non-error arm fixes the type.
            let body = self.check_expr(&arm.body, result_ty);
            if result_ty.is_none() && body.ty != Ty::Error {
                result_ty = Some(body.ty);
            }
            self.scope.truncate(scope_mark);
            checked.push(hir::MatchArm { variants: variant_tags, bindings, body });
        }
        // Exhaustiveness: every variant covered, or a `_` wildcard present.
        if !has_wildcard {
            let missing: Vec<&str> = variants
                .iter()
                .enumerate()
                .filter(|(i, _)| !covered[*i])
                .map(|(_, v)| v.0.as_str())
                .collect();
            if !missing.is_empty() {
                self.diags
                    .error(format!("non-exhaustive `match` on '{type_name}': missing {}", missing.join(", ")), span);
            }
        }
        let ty = result_ty.unwrap_or(Ty::Unit);
        self.constrain(ty, expected, span);
        Expr { kind: ExprKind::Match { scrutinee: Box::new(s), arms: checked }, ty, span }
    }

    fn check_if(&mut self, cond: &ast::Expr, then: &ast::Block, els: Option<&ast::Expr>, expected: Option<Ty>, span: Span) -> Expr {
        let c = self.check_expr(cond, Some(Ty::Bool));
        // `task_group` `wait`-state (slice ④c): each branch starts from the pre-`if` state; after
        // the `if`, a `wait()` is guaranteed only if it ran on *every* path — `then && else` (and
        // an absent `else` is a path that did not wait). Soundly tracks `get`-before-`wait`.
        let in_tg = !self.wait_state.is_empty();
        let w_before = self.wait_state.last().copied().unwrap_or(false);
        let then_b = self.check_block(then, expected);
        let w_then = self.wait_state.last().copied().unwrap_or(false);
        if in_tg {
            *self.wait_state.last_mut().unwrap() = w_before;
        }
        let els_b = match els {
            Some(ast::Expr { kind: ast::ExprKind::Block(b), .. }) => self.check_block(b, expected),
            Some(e) => {
                // `else if` chain: check as an expression and wrap as a block value.
                let v = self.check_expr(e, expected);
                Block { stmts: Vec::new(), value: Some(Box::new(v)) }
            }
            None => Block { stmts: Vec::new(), value: None },
        };
        if in_tg {
            let w_els = if els.is_some() { self.wait_state.last().copied().unwrap_or(false) } else { w_before };
            *self.wait_state.last_mut().unwrap() = w_then && w_els;
        }

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
                Stmt::AssignIndex { index, value, .. } => {
                    self.finalize_expr(index);
                    self.finalize_expr(value);
                }
                Stmt::Return(Some(e)) | Stmt::Expr(e) => self.finalize_expr(e),
                Stmt::Return(None) => {}
                Stmt::LetTuple { init, .. } => self.finalize_expr(init),
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
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::IntArith { lhs, rhs, .. } => {
                self.finalize_expr(lhs);
                self.finalize_expr(rhs);
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    self.finalize_expr(a);
                }
            }
            ExprKind::FnValue(_) => {}
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.finalize_expr(c);
                }
            }
            ExprKind::CallFnValue { callee, args } => {
                self.finalize_expr(callee);
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
            ExprKind::Block(b) | ExprKind::Arena(b) | ExprKind::TaskGroup(b) => self.finalize_block(b),
            ExprKind::Spawn { closure, .. } => self.finalize_expr(closure),
            ExprKind::EnumValue { payload, .. } => {
                for p in payload {
                    self.finalize_expr(p);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.finalize_expr(scrutinee);
                for a in arms {
                    self.finalize_expr(&mut a.body);
                }
            }
            ExprKind::ResultMapErr { result, f } => {
                self.finalize_expr(result);
                self.finalize_expr(f);
            }
            ExprKind::TaskGet(inner) => self.finalize_expr(inner),
            ExprKind::Wait => {}
            ExprKind::OptionSome(inner) | ExprKind::ResultOk(inner) | ExprKind::ResultErr(inner)
            | ExprKind::Try(inner) | ExprKind::HeapNew(inner) | ExprKind::BoxGet(inner)
            | ExprKind::BoxClone(inner) | ExprKind::StrClone(inner) | ExprKind::StrBorrow(inner) | ExprKind::BuilderToString(inner) | ExprKind::ArraySum { source: inner, .. } | ExprKind::ArrayCount { source: inner, .. } | ExprKind::ArrayAnyAll { source: inner, .. } | ExprKind::ArrayMinMax { source: inner, .. } | ExprKind::ArrayToArray { source: inner, .. } | ExprKind::ArrayPartition { source: inner, .. } | ExprKind::ArrayParMap { source: inner, .. } | ExprKind::ArraySort { source: inner, .. } | ExprKind::ArraySortBy { source: inner, .. } | ExprKind::ArrayToSlice(inner)
            | ExprKind::Len(inner) => {
                self.finalize_expr(inner)
            }
            ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
                self.finalize_expr(recv);
                self.finalize_expr(index);
            }
            ExprKind::BuilderWrite { builder, arg, .. } => {
                self.finalize_expr(builder);
                self.finalize_expr(arg);
            }
            ExprKind::ArrayReduce { source, init, .. } | ExprKind::ArrayScan { source, init, .. } => {
                self.finalize_expr(source);
                self.finalize_expr(init);
            }
            ExprKind::ArrayDot { a, b, .. } => {
                self.finalize_expr(a);
                self.finalize_expr(b);
            }
            ExprKind::ArrayChunks { source, n, .. } => {
                self.finalize_expr(source);
                self.finalize_expr(n);
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
            ExprKind::JsonDecode { input, .. } | ExprKind::JsonDecodeArray { input, .. } | ExprKind::JsonDecodeStructArray { input, .. } => self.finalize_expr(input),
            ExprKind::FsReadFile { path } => self.finalize_expr(path),
            ExprKind::IoStdoutWrite { arg } => self.finalize_expr(arg),
            ExprKind::IoStdoutWriteBuilder { builder } => self.finalize_expr(builder),
            ExprKind::Tuple { elems, .. } => {
                for el in elems {
                    self.finalize_expr(el);
                }
            }
            ExprKind::MathOp { operands, .. } => {
                for o in operands {
                    self.finalize_expr(o);
                }
            }
            ExprKind::TupleIndex { recv, .. } => self.finalize_expr(recv),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Char(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Local(_)
            | ExprKind::OptionNone
            | ExprKind::Field { .. }
            | ExprKind::BuilderNew
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
    ty.is_numeric() || matches!(ty, Ty::Str | Ty::String | Ty::Bool | Ty::Char | Ty::Error)
}

/// Map a method name to the builder writer it denotes (MMv2 slice 7c/7d), if any.
fn builder_write_kind(method: &str) -> Option<BuilderWriteKind> {
    Some(match method {
        "write" => BuilderWriteKind::Str,
        "write_int" => BuilderWriteKind::Int,
        "write_bool" => BuilderWriteKind::Bool,
        "write_char" => BuilderWriteKind::Char,
        "write_float" => BuilderWriteKind::Float,
        _ => return None,
    })
}

/// The surface method name of a builder writer (for diagnostics).
fn builder_write_method_name(kind: BuilderWriteKind) -> &'static str {
    match kind {
        BuilderWriteKind::Str => "write",
        BuilderWriteKind::Int => "write_int",
        BuilderWriteKind::Bool => "write_bool",
        BuilderWriteKind::Char => "write_char",
        BuilderWriteKind::Float => "write_float",
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
        Ty::DynStructArray(id, _) => format!("array<struct#{id}>"),
        Ty::DynSliceArray(p) => format!("array<slice<{}>>", scalar_name(prim_to_scalar(p))),
        Ty::Slice(s) => format!("slice<{}>", scalar_name(s)),
        Ty::DynArray(s) => format!("array<{}>", scalar_name(s)),
        Ty::Str => "str".to_string(),
        Ty::String => "string".to_string(),
        Ty::ArenaHandle => "arena".to_string(),
        Ty::Builder => "builder".to_string(),
        Ty::Struct(id) => format!("struct#{id}"),
        Ty::Tuple(id) => format!("tuple#{id}"),
        Ty::Fn(id) => format!("fn#{id}"),
        Ty::Enum(id) => format!("enum#{id}"),
        Ty::Task(s) => format!("Task<{}>", scalar_name(s)),
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

/// Intern a tuple type (dedup by element list) into `tuples`, returning its id. Tuples are
/// few, so a linear scan is fine.
fn intern_tuple(tuples: &mut Vec<hir::TupleDef>, elems: Vec<Scalar>) -> u32 {
    if let Some(i) = tuples.iter().position(|t| t.elems == elems) {
        return i as u32;
    }
    tuples.push(hir::TupleDef { elems });
    (tuples.len() - 1) as u32
}

fn intern_fn_type(fn_types: &mut Vec<hir::FnTy>, params: Vec<Scalar>, ret: Scalar) -> u32 {
    let ft = hir::FnTy { params, ret };
    if let Some(i) = fn_types.iter().position(|t| *t == ft) {
        return i as u32;
    }
    fn_types.push(ft);
    (fn_types.len() - 1) as u32
}

fn resolve_type(
    t: &ast::Type,
    struct_ids: &HashMap<String, u32>,
    enum_ids: &HashMap<String, u32>,
    tuples: &mut Vec<hir::TupleDef>,
    fn_types: &mut Vec<hir::FnTy>,
    diags: &mut Diagnostics,
) -> Ty {
    let (path, args, span) = match t {
        ast::Type::Named { path, args, span } => (path, args.as_slice(), *span),
        // `fn(T, U) -> R` — a function-value type. Scalar parameters/return (matching first-class
        // function values); interned into `fn_types` like a tuple type.
        ast::Type::Fn { params, ret, span: _ } => {
            let mut pscalars = Vec::with_capacity(params.len());
            for p in params {
                let pty = resolve_type(p, struct_ids, enum_ids, tuples, fn_types, diags);
                if pty == Ty::Error {
                    return Ty::Error;
                }
                match ty_to_scalar(pty) {
                    Some(s) => pscalars.push(s),
                    None => {
                        diags.error(format!("a function-type parameter must be a scalar for now, got {}", ty_name(pty)), p.span());
                        return Ty::Error;
                    }
                }
            }
            let rty = resolve_type(ret, struct_ids, enum_ids, tuples, fn_types, diags);
            if rty == Ty::Error {
                return Ty::Error;
            }
            let Some(rs) = ty_to_scalar(rty) else {
                diags.error(format!("a function-type return must be a scalar for now, got {}", ty_name(rty)), ret.span());
                return Ty::Error;
            };
            return Ty::Fn(intern_fn_type(fn_types, pscalars, rs));
        }
        ast::Type::Tuple { elems, span: _ } => {
            // PR1 cut: tuple elements are primitive scalars (int/float/bool/char) — Copy,
            // `Static`, so the tuple needs no drop/region machinery. `str`/owned elements later.
            let mut scalars = Vec::with_capacity(elems.len());
            for e in elems {
                let ety = resolve_type(e, struct_ids, enum_ids, tuples, fn_types, diags);
                if ety == Ty::Error {
                    return Ty::Error;
                }
                match ty_to_scalar(ety) {
                    Some(s @ (Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool | Scalar::Char
                    | Scalar::Str | Scalar::String | Scalar::DynArray(_) | Scalar::DynStructArray(_))) => scalars.push(s),
                    _ => {
                        diags.error(
                            format!("tuple elements must be a scalar, str, owned string, or owned array for now, got {}", ty_name(ety)),
                            e.span(),
                        );
                        return Ty::Error;
                    }
                }
            }
            return Ty::Tuple(intern_tuple(tuples, scalars));
        }
    };
    let name = path.segments.last().map(|s| s.name.as_str()).unwrap_or("");
    match name {
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "str" => Ty::Str,
        "string" => Ty::String,
        "f32" => Ty::Float(FloatTy { bits: 32 }),
        "f64" => Ty::Float(FloatTy { bits: 64 }),
        "()" => Ty::Unit,
        // `Error` is the builtin error sum type — resolved via `enum_ids` like any enum name.
        "box" => {
            let inner = match args {
                [a] => resolve_type(a, struct_ids, enum_ids, tuples, fn_types, diags),
                _ => {
                    diags.error("box takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            // `scalar_arg` accepts structs and owned `string` (valid Option/Result payloads), but
            // a box payload must be a true primitive scalar: codegen can't size a struct box, and
            // a Move payload (`string`) has no `box` drop story. Reject both with a clean
            // diagnostic (else `box<string>`/`box<Struct>` would type-check then panic in codegen).
            match scalar_arg(inner, "box payload", span, diags) {
                Some(Scalar::Struct(_)) => {
                    diags.error("a box payload must be a primitive scalar (struct boxes are not supported)".to_string(), span);
                    Ty::Error
                }
                Some(Scalar::Enum(_)) => {
                    diags.error("a box payload must be a primitive scalar (sum-type boxes are not supported)".to_string(), span);
                    Ty::Error
                }
                Some(s) if s.is_move() => {
                    diags.error(format!("a box payload must be a primitive scalar (an owned `{}` cannot be boxed)", scalar_name(s)), span);
                    Ty::Error
                }
                Some(Scalar::Str) => {
                    diags.error("a box payload must be a primitive scalar (a `str` view is not boxable)".to_string(), span);
                    Ty::Error
                }
                Some(s) => Ty::Box(s),
                None => Ty::Error,
            }
        }
        "Option" => {
            let inner = match args {
                [a] => resolve_type(a, struct_ids, enum_ids, tuples, fn_types, diags),
                _ => {
                    diags.error("Option takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "Option payload", span, diags) {
                Some(s) => Ty::Option(s),
                None => Ty::Error,
            }
        }
        "slice" => {
            let inner = match args {
                [a] => resolve_type(a, struct_ids, enum_ids, tuples, fn_types, diags),
                _ => {
                    diags.error("slice takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            match scalar_arg(inner, "slice element", span, diags) {
                Some(s) => Ty::Slice(s),
                None => Ty::Error,
            }
        }
        // `array<T>` — an owned, dynamic-length array (MMv2). Currently usable as a return
        // type so a function can hand back a free-standing owned array.
        "array" => {
            let inner = match args {
                [a] => resolve_type(a, struct_ids, enum_ids, tuples, fn_types, diags),
                _ => {
                    diags.error("array takes exactly one type argument".to_string(), span);
                    return Ty::Error;
                }
            };
            // An `array<Struct>` is a dynamic AoS (its own owned type); only a primitive
            // element resolves to the scalar `array<T>` (`DynArray`).
            match inner {
                Ty::Struct(id) => Ty::DynStructArray(id, Layout::Aos),
                _ => match scalar_arg(inner, "array element", span, diags) {
                    Some(s) => Ty::DynArray(s),
                    None => Ty::Error,
                },
            }
        }
        "Result" => {
            let (ok, err) = match args {
                [a, b] => (
                    resolve_type(a, struct_ids, enum_ids, tuples, fn_types, diags),
                    resolve_type(b, struct_ids, enum_ids, tuples, fn_types, diags),
                ),
                _ => {
                    diags.error("Result takes two type arguments".to_string(), span);
                    return Ty::Error;
                }
            };
            match (
                scalar_arg(ok, "Result ok payload", span, diags),
                scalar_arg(err, "Result err payload", span, diags),
            ) {
                (Some(o), Some(e)) => Ty::Result(o, e),
                _ => Ty::Error,
            }
        }
        _ => match parse_int_name(name) {
            Some(it) => Ty::Int(it),
            None => match struct_ids.get(name) {
                Some(&id) => Ty::Struct(id),
                None => match enum_ids.get(name) {
                    Some(&id) => Ty::Enum(id),
                    None => {
                        diags.error(format!("unknown type: '{name}'"), span);
                        Ty::Error
                    }
                },
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
        // A `str` field now decodes as a zero-copy view (MMv2 slice 6); decoding from a param
        // (region Static, the caller owns the buffer) and returning the struct is allowed.
        let (_r, strf) = check("U { name: str }\nfn parse(s: str) -> Result<U, Error> {\n  u: U := json.decode(s)?\n  return Ok(u)\n}\nfn main() -> i32 { return 0 }\n");
        assert!(!strf.has_errors(), "a str field decodes zero-copy and is returnable from a param");
    }

    #[test]
    fn json_decoded_str_view_cannot_escape_arena() {
        // A `str` field decoded from an arena-allocated input is a view into that input; the
        // decoded struct is region-tied to it, so the view cannot escape the arena.
        let (_p, d) = check("U { id: i64, name: str }\nfn bad(key: str) -> Result<i32, Error> {\n  mut outer := \"\"\n  arena {\n    d := key + key\n    u: U := json.decode(d)?\n    outer = u.name\n  }\n  return Ok(0)\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a decoded str view from arena input must not escape the arena");
    }

    #[test]
    fn json_decode_struct_array_checks_and_escape() {
        // MMv2 slice 8d: `json.decode` into `array<Struct>` infers the target through `?` and is
        // usable as a frame-local when decoded from a param (Static input, caller owns the buffer).
        let (_p, ok) = check("User { id: i64, name: str }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.len())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "json.decode into array<Struct> should check");
        // The decoded array's `str` fields are views into the input, so an array decoded from an
        // arena-allocated input must not escape the arena (use-after-free of the freed buffer).
        let (_q, esc) = check("User { id: i64, name: str }\nfn bad(key: str) -> Result<i64, Error> {\n  mut total := 0\n  arena {\n    d := key + key\n    users: array<User> := json.decode(d)?\n    total = users.len()\n  }\n  return Ok(total)\n}\nfn main() -> i32 = 0\n");
        assert!(!esc.has_errors(), "reading .len() inside the arena is fine (no escape)");
        // Returning the arena-decoded array (region-tied to the arena input) must be rejected.
        let (_r, ret) = check("User { id: i64, name: str }\nfn bad(key: str) -> Result<array<User>, Error> {\n  arena {\n    d := key + key\n    users: array<User> := json.decode(d)?\n    return Ok(users)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(ret.has_errors(), "an arena-tied decoded struct array must not escape via return");
    }

    #[test]
    fn array_index_checks_and_rejects() {
        // Indexing a scalar array / slice / owned array yields the element scalar.
        let (_p, ok) = check("fn main() -> i32 {\n  xs := [10, 20, 30]\n  return xs[1]\n}\n");
        assert!(!ok.has_errors(), "indexing a scalar array should check");
        let (_o, owned) = check("fn main() -> Result<(), Error> {\n  xs: array<i64> := json.decode(\"[1,2]\")?\n  print(xs[0])\n  return Ok(())\n}\n");
        assert!(!owned.has_errors(), "indexing an owned array<i64> should check");
        // A non-integer index is rejected.
        let (_q, badidx) = check("fn main() -> i32 {\n  xs := [10, 20]\n  return xs[true]\n}\n");
        assert!(badidx.has_errors(), "a non-integer index must be rejected");
        // Indexing a non-array is rejected.
        let (_r, nonarr) = check("fn main() -> i32 {\n  x := 5\n  return x[0]\n}\n");
        assert!(nonarr.has_errors(), "indexing a non-array must be rejected");
        // A whole-struct element `ps[0]` is a by-value (Copy) load — supported; the bound struct's
        // field reads fine. (Reading a field directly, `ps[0].x`, also works — see
        // `struct_array_element_field_checks`.)
        let (_s, structarr) = check("P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}, P{x: 2}]\n  q := ps[0]\n  return q.x\n}\n");
        assert!(!structarr.has_errors(), "a whole-struct element value should check (by-value copy)");
        // Indexing a Move-only element (here a nested owned array) is rejected — copying the
        // element's {ptr,len} without ownership transfer would double-free.
        let (_m, moveelem) = check("fn take(xs: array<array<i64>>) -> i64 {\n  ys := xs[0]\n  return ys.len()\n}\nfn main() -> i32 = 0\n");
        assert!(moveelem.has_errors(), "indexing an array of a Move type must be rejected (double-free)");
        // A `slice<Struct>` element index also yields a whole struct by value — supported (the
        // element resolves to a struct via the slice arm and loads through `SliceIndex`).
        let (_sl, slstruct) = check("P { x: i32 }\nfn first(s: slice<P>) -> i32 {\n  q := s[0]\n  return q.x\n}\nfn main() -> i32 = 0\n");
        assert!(!slstruct.has_errors(), "indexing a slice<Struct> for a whole struct should check");
    }

    #[test]
    fn str_in_composites_checks() {
        // PR-A: `str` is a composite payload (`Scalar::Str`). `Option<str>` / `Result<str,E>`
        // construct and unwrap; a literal-str payload is Static, so it is returnable.
        let (_p, ok) = check("fn mk() -> Option<str> = Some(\"lit\")\nfn r() -> Result<str, Error> = Ok(\"x\")\nfn main() -> i32 {\n  s := mk() else \"no\"\n  print(s)\n  return 0\n}\n");
        assert!(!ok.has_errors(), "Option<str> / Result<str,Error> with literal payloads should check");
        // Region: an arena-built `str` in an `Option<str>` must not escape the arena (the view
        // would dangle) — this falls out of the existing region model, no new logic.
        let (_q, esc) = check("fn bad(a: str, b: str) -> Option<str> {\n  arena {\n    return Some(a + b)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(esc.has_errors(), "an arena str inside Option<str> must not escape the arena");
        // `box<str>` is rejected (a view is not boxable) — both the annotation and `heap.new`.
        let (_r, bann) = check("fn f(b: box<str>) -> i32 = 0\nfn main() -> i32 = 0\n");
        assert!(bann.has_errors(), "box<str> annotation must be rejected");
        let (_s, bnew) = check("fn main() -> i32 {\n  arena {\n    p: box<str> := heap.new(\"x\")\n    return 0\n  }\n}\n");
        assert!(bnew.has_errors(), "heap.new of a str must be rejected");
        // Un-annotated `heap.new(move_value)` must reject at the scalar level too — else inference
        // forms `box<string>` and codegen's `scalar_bytes` panics (the `box<…>` annotation path is
        // guarded separately, so this exercises the inference path).
        let (_m, bmove) = check("fn mk() -> string = \"x\".clone()\nfn main() -> i32 {\n  arena {\n    p := heap.new(mk())\n    return 0\n  }\n}\n");
        assert!(bmove.has_errors(), "un-annotated heap.new of an owned string must be rejected");
    }

    #[test]
    fn str_array_and_slice_checks() {
        // PR-B: `array<str>` literal + index (→ str) + len.
        let (_p, ok) = check("fn main() -> i32 {\n  xs := [\"a\", \"b\", \"c\"]\n  print(xs[1])\n  print(xs.len())\n  return 0\n}\n");
        assert!(!ok.has_errors(), "array<str> literal + index + len should check");
        // `slice<str>` param: index + len.
        let (_q, sl) = check("fn snd(xs: slice<str>) -> str = xs[1]\nfn len(xs: slice<str>) -> i64 = xs.len()\nfn main() -> i32 = 0\n");
        assert!(!sl.has_errors(), "slice<str> index + len should check");
        // Region: a `slice<str>` viewing a local array must not escape.
        let (_r, esc) = check("fn bad() -> slice<str> {\n  s: slice<str> := [\"a\", \"b\"]\n  return s\n}\nfn main() -> i32 = 0\n");
        assert!(esc.has_errors(), "a slice<str> into a local array must not escape");
        // Region: an `array<str>` of arena strs must not let an element escape via index+return
        // (the fixed array is region-tracked because its `str` element is).
        let (_s, idxesc) = check("fn bad(a: str, b: str) -> str {\n  arena {\n    xs := [a + b, a]\n    return xs[0]\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(idxesc.has_errors(), "a str element of an arena str-array must not escape via index");
        // A literal-str array element is Static → returnable (no false reject); a scalar array
        // stays returnable too (no regression from the new array region-tracking).
        let (_t, lit) = check("fn ok() -> str {\n  xs := [\"lit\", \"lat\"]\n  return xs[0]\n}\nfn n() -> i64 {\n  ys := [1, 2, 3]\n  return ys[0]\n}\nfn main() -> i32 = 0\n");
        assert!(!lit.has_errors(), "literal-str and scalar array element reads stay returnable");
        // A `slice<str>` coerced from an arena str-array must not escape via return — the slice
        // inherits the array's region (`region_of(ArrayToSlice)`), and `slice<str>` is now
        // region-tracked.
        let (_u, slesc) = check("fn bad(a: str, b: str) -> slice<str> {\n  arena {\n    s: slice<str> := [a + b, a]\n    return s\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(slesc.has_errors(), "a slice<str> over an arena str-array must not escape");
        // A scalar `slice<i32>` parameter stays returnable (it borrows the caller) — no regression
        // from adding `Slice` to `tracks_region`.
        let (_v, slok) = check("fn id(xs: slice<i32>) -> slice<i32> = xs\nfn main() -> i32 = 0\n");
        assert!(!slok.has_errors(), "a slice<i32> parameter stays returnable");
    }

    #[test]
    fn fs_read_file_checks() {
        // std.fs: `fs.read_file(path)` yields `Result<string, Error>`; `?` unwraps an owned string.
        let (_p, ok) = check("fn main() -> Result<(), Error> {\n  data := fs.read_file(\"x.txt\")?\n  print(data.len())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "fs.read_file should check and yield an owned string");
        // The owned string owns a fresh buffer (not a view), so it is returnable.
        let (_q, ret) = check("fn load(p: str) -> Result<string, Error> {\n  return Ok(fs.read_file(p)?)\n}\nfn main() -> i32 = 0\n");
        assert!(!ret.has_errors(), "an fs.read_file string is owned and returnable");
        // Wrong arity errors cleanly.
        let (_r, ar) = check("fn main() -> Result<(), Error> {\n  data := fs.read_file()?\n  return Ok(())\n}\n");
        assert!(ar.has_errors(), "fs.read_file needs exactly one argument");
    }

    #[test]
    fn io_stdout_write_checks() {
        // std.io: `io.stdout.write(s)` (s: str / owned string) yields `Result<(), Error>`.
        let (_p, ok) = check("fn main() -> Result<(), Error> {\n  io.stdout.write(\"hi\")?\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "io.stdout.write of a str should check");
        // An owned string is accepted (auto-borrowed to str) and stays usable afterwards.
        let (_q, owned) = check("fn mk() -> string = \"x\".clone()\nfn main() -> Result<(), Error> {\n  s := mk()\n  io.stdout.write(s)?\n  print(s.len())\n  return Ok(())\n}\n");
        assert!(!owned.has_errors(), "io.stdout.write borrows an owned string (does not move it)");
        // A `builder` is accepted directly (written, not consumed — still usable / dropped after).
        let (_b, bld) = check("fn main() -> Result<(), Error> {\n  b := builder()\n  b.write(\"hi\")\n  io.stdout.write(b)?\n  print(b.to_string())\n  return Ok(())\n}\n");
        assert!(!bld.has_errors(), "io.stdout.write accepts a builder directly (borrows it)");
        // Wrong arity errors.
        let (_r, ar) = check("fn main() -> Result<(), Error> {\n  io.stdout.write()?\n  return Ok(())\n}\n");
        assert!(ar.has_errors(), "io.stdout.write needs exactly one argument");
    }

    #[test]
    fn struct_array_element_field_checks() {
        // MMv2 slice 8f: `arr[i].field` on a struct array reads one field (scalar or str view),
        // bounds-checked.
        let (_p, ok) = check("User { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users[0].score)\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "arr[i].field on a struct array should check");
        // A whole-struct `arr[i]` value (no field) is a by-value load — supported.
        let (_q, whole) = check("P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}]\n  q := ps[0]\n  return q.x\n}\n");
        assert!(!whole.has_errors(), "a whole-struct element value should check (by-value copy)");
        // An unknown field on the element is rejected.
        let (_r, badf) = check("P { x: i32 }\nfn main() -> i32 {\n  ps := [P{x: 1}]\n  return ps[0].nope\n}\n");
        assert!(badf.has_errors(), "an unknown element field must be rejected");
        // A `str` field read from an arena-decoded element must not escape the arena.
        let (_s, esc) = check("U { id: i64, name: str }\nfn bad(key: str) -> Result<str, Error> {\n  mut out := \"\"\n  arena {\n    d := key + key\n    users: array<U> := json.decode(d)?\n    out = users[0].name\n  }\n  return Ok(out)\n}\nfn main() -> i32 = 0\n");
        assert!(esc.has_errors(), "a str field of an arena-decoded element must not escape the arena");
    }

    #[test]
    fn pipeline_over_dynamic_struct_array_checks() {
        // MMv2 slice 8d-2: a fused pipeline over a decoded `array<Struct>` variable type-checks
        // (`where(.field)` + projection + reduction).
        let (_p, ok) = check("User { id: i64, active: bool, score: i32 }\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.where(.active).score.sum())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "a where(.field).field.sum() pipeline over array<Struct> should check");
        // `where` with a whole-struct predicate over a dynamic struct array now checks (it loads
        // the element by value and keeps it, so the following `.score` projection reads the source).
        let (_q, ok2) = check("User { id: i64, active: bool, score: i32 }\nfn keep(u: User) -> bool = u.active\nfn main() -> Result<(), Error> {\n  users: array<User> := json.decode(\"[]\")?\n  print(users.where(keep).score.sum())\n  return Ok(())\n}\n");
        assert!(!ok2.has_errors(), "'where' with a whole-struct predicate should check");
    }

    #[test]
    fn json_decoded_struct_cannot_escape_via_result_local() {
        // The decoded struct's region must survive while wrapped in a `Result`: binding the raw
        // `json.decode(...)` to a `Result`-typed local, unwrapping it with `?`, then returning
        // `Ok(u)` must still be rejected (otherwise the arena-tied str views escape → UAF).
        let (_p, d) = check("U { id: i64, name: str }\nfn bad(key: str) -> Result<U, Error> {\n  arena {\n    d := key + key\n    res: Result<U, Error> := json.decode(d)\n    u: U := res?\n    return Ok(u)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a region-tied decoded struct must not escape through a Result-typed local");
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
    fn call_result_view_cannot_escape_arena() {
        // A call may return a view borrowing one of its args; calling such a fn with an
        // arena-backed str and returning the result out of the arena must be rejected (the
        // borrowed buffer is freed at arena end → use-after-free). Conservative: the call
        // result lives no longer than its shortest-lived argument.
        let (_p, d) = check("fn dup(s: str) -> str = s\nfn leak() -> str {\n  arena {\n    x := \"a\" + \"b\"\n    return dup(x)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a view returned from a call on an arena arg must not escape the arena");
    }

    #[test]
    fn call_result_view_with_static_arg_returns_ok() {
        // The arg propagation only shortens the region by *tracked* args: a call whose str args
        // are literals (Static) yields a Static result, so it stays returnable — no false reject.
        let (_p, d) = check("fn dup(s: str) -> str = s\nfn ok() -> str = dup(\"hi\")\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "a call on a static-region arg should stay returnable");
    }

    #[test]
    fn reduce_str_accumulator_cannot_escape_arena() {
        // `reduce`'s accumulator is folded in the enclosing arena; when it is region-tracked (a
        // `str` built by concatenation), returning it out of the arena must be rejected (the
        // accumulator buffer is freed at arena end → use-after-free).
        let (_p, d) = check("fn build(a: str, e: i64) -> str = a + \"?\"\nfn leak() -> str {\n  arena {\n    ns := [1, 2, 3]\n    return ns.reduce(build, \"\")\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "a str reduce accumulator built in an arena must not escape it");
    }

    #[test]
    fn reduce_scalar_accumulator_returns_ok() {
        // A scalar reduce result carries no region (it is Copy), so folding inside an arena and
        // returning the scalar is fine — the arena region must not leak onto plain scalars.
        let (_p, d) = check("fn add(a: i64, e: i64) -> i64 = a + e\nfn total() -> i64 {\n  arena {\n    ns := [1, 2, 3]\n    return ns.reduce(0, add)\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "a scalar reduce accumulator carries no region and may be returned");
    }

    #[test]
    fn str_clone_produces_returnable_owned_string() {
        // `str.clone()` yields a heap-owned `string` (region `Static`), so it can be returned out
        // of the arena its source was built in — the explicit escape hatch (MMv2 slice 7).
        let (_p, d) = check("fn longer(a: str, b: str) -> string {\n  arena {\n    c := a + b\n    return c.clone()\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "a cloned (owned) string should be returnable from an arena");
    }

    #[test]
    fn arena_str_without_clone_still_cannot_escape() {
        // Without the `.clone()`, the arena-backed `str` view must not escape (regression guard
        // that adding `string` did not loosen the borrow's region check).
        let (_p, d) = check("fn longer(a: str, b: str) -> str {\n  arena {\n    c := a + b\n    return c\n  }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "an arena-backed str view must not escape without an explicit clone");
    }

    #[test]
    fn owned_string_is_move_use_after_move_rejected() {
        // A `string` is a Move type: binding it elsewhere moves it, so a later use is rejected
        // (whereas `print` borrows — covered by the e2e tests).
        let (_p, d) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  s := mk(\"x\")\n  t := s\n  return t.len()\n}\n");
        // `t := s` moves; but `t.len()` is fine. Now force a use-after-move:
        assert!(!d.has_errors(), "moving a string into a new binding and using the new one is fine");
        let (_q, d2) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  s := mk(\"x\")\n  t := s\n  return s.len()\n}\n");
        assert!(d2.has_errors(), "using a string after it was moved must be rejected");
    }

    #[test]
    fn string_borrows_as_str_arg_without_moving() {
        // MMv2 slice 7b: passing an owned `string` to a `str` parameter *borrows* it (zero-cost,
        // same `{ptr,len}` layout). The borrow does not consume the string, so a later use is
        // fine — unlike passing it to a `string` parameter (which moves).
        let (_p, d) = check("fn show(s: str) -> i64 = s.len()\nfn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  s := mk(\"x\")\n  a := show(s)\n  b := show(s)\n  return 0\n}\n");
        assert!(!d.has_errors(), "borrowing a string as a str arg must not move it");
    }

    #[test]
    fn string_borrows_into_str_let_and_assign() {
        // MMv2 slice 7e: a `str`-annotated let borrows an owned `string` (non-consuming), so the
        // source stays usable.
        let (_p, d) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  owned := mk(\"x\")\n  view: str := owned\n  print(view)\n  print(owned.len())\n  return 0\n}\n");
        assert!(!d.has_errors(), "borrowing a string into a str let must check and not move it");
        // A `str` place assignment borrows the same way.
        let (_q, d2) = check("fn mk(a: str) -> string = a.clone()\nfn main() -> i32 {\n  owned := mk(\"x\")\n  mut view: str := \"\"\n  view = owned\n  print(view)\n  print(owned.len())\n  return 0\n}\n");
        assert!(!d2.has_errors(), "borrowing a string into a str place assignment must check");
    }

    #[test]
    fn str_let_borrow_returned_escapes() {
        // The let-bound borrow is `Frame`-regioned: returning it (the buffer is freed at exit) is
        // rejected with the borrow-specific diagnostic — both via explicit `return` and as a
        // block's trailing (fall-through) value.
        let (_p, d) = check("fn mk(a: str) -> string = a.clone()\nfn leak() -> str {\n  owned := mk(\"x\")\n  view: str := owned\n  return view\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "returning a str that borrows a local string must be rejected");
        // Fall-through (trailing-value) return path — same rejection.
        let (_q, d2) = check("fn mk(a: str) -> string = a.clone()\nfn leak() -> str {\n  owned := mk(\"x\")\n  view: str := owned\n  view\n}\nfn main() -> i32 = 0\n");
        assert!(d2.has_errors(), "a trailing-value str borrow of a local string must also be rejected");
    }

    #[test]
    fn result_string_payload_checks_and_returns() {
        // MMv2 slice 8a: `Result<string, Error>` is representable; an owned `string` (Static
        // region) is returnable through it, and `?` unwraps to an owned string.
        let (_p, d) = check("fn mk(a: str) -> Result<string, Error> = Ok(a.clone())\nfn use(name: str) -> Result<i64, Error> {\n  s := mk(name)?\n  return Ok(s.len())\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "Result<string,Error> construct/return/unwrap should check");
    }

    #[test]
    fn option_string_payload_checks() {
        let (_p, d) = check("fn first() -> Option<string> = Some(\"x\".clone())\nfn main() -> i32 {\n  s := first() else { return 9 }\n  print(s)\n  return 0\n}\n");
        assert!(!d.has_errors(), "Option<string> construct + else-unwrap should check");
    }

    #[test]
    fn json_decode_scalar_array_checks() {
        // MMv2 slice 8c: `json.decode` into an owned `array<scalar>` checks (target inferred from
        // the `array<T>` annotation threaded through `?`).
        let (_p, d) = check("fn parse(s: str) -> Result<array<i64>, Error> {\n  xs: array<i64> := json.decode(s)?\n  return Ok(xs)\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "json.decode into array<i64> should check");
        // `array<char>` is a representable owned-array type, but the runtime parser only handles
        // int/float/bool elements — `json.decode` rejects it cleanly (exercises the element check).
        let (_q, d2) = check("fn parse(s: str) -> Result<array<char>, Error> {\n  xs: array<char> := json.decode(s)?\n  return Ok(xs)\n}\nfn main() -> i32 = 0\n");
        assert!(d2.has_errors(), "json.decode into array<char> must be rejected for now");
    }

    #[test]
    fn result_and_option_array_payload_checks() {
        // MMv2 slice 8b: `Result<array<i64>, Error>` / `Option<array<i64>>` are representable; an
        // owned array is returnable through them and `?`/`else` unwrap to the owned array.
        let (_p, d) = check("fn mk() -> Result<array<i64>, Error> = Ok([1, 2, 3].to_array())\nfn use() -> Result<i64, Error> {\n  xs := mk()?\n  return Ok(xs.sum())\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "Result<array<i64>,Error> construct/return/unwrap should check");
        let (_q, d2) = check("fn first() -> Option<array<i64>> = Some([1, 2].to_array())\nfn main() -> i32 {\n  xs := first() else { return 9 }\n  print(xs.sum())\n  return 0\n}\n");
        assert!(!d2.has_errors(), "Option<array<i64>> construct + else-unwrap should check");
    }

    #[test]
    fn box_array_payload_rejected_cleanly() {
        // Like `box<string>`, an owned `array` is a Move scalar and cannot be boxed — rejected in
        // sema (not a codegen panic).
        let (_p, d) = check("fn main() -> i32 {\n  arena {\n    p: box<array<i64>> := heap.new([1].to_array())\n    return 0\n  }\n}\n");
        assert!(d.has_errors(), "box<array<T>> must be rejected (an owned array cannot be boxed)");
    }

    #[test]
    fn box_string_payload_rejected_cleanly() {
        // `string` is now a scalar (slice 8a), so `box<string>` must be rejected in sema with a
        // clean diagnostic — not type-check and then panic in codegen (the box payload guard must
        // cover Move scalars, like it already covers structs).
        let (_p, d) = check("fn main() -> i32 {\n  arena {\n    p: box<string> := heap.new(\"x\".clone())\n    return 0\n  }\n}\n");
        assert!(d.has_errors(), "box<string> must be rejected (an owned string cannot be boxed)");
    }

    #[test]
    fn result_string_use_after_try_rejected() {
        // `?` consumes the Result (moves its owned payload out); using the source again is a
        // use-after-move.
        let (_p, d) = check("fn mk() -> Result<string, Error> = Ok(\"x\".clone())\nfn use() -> Result<i64, Error> {\n  r := mk()\n  a := r?\n  b := r?\n  return Ok(a.len())\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "using a Result<string> after `?` consumed it must be rejected");
    }

    #[test]
    fn builder_constructs_string_checks() {
        // MMv2 slice 7c: `builder()` + `write`/`write_int` + `to_string()` yields an owned
        // `string` returnable from the function.
        let (_p, d) = check("fn make(name: str, n: i64) -> string {\n  b := builder()\n  b.write(\"x=\")\n  b.write(name)\n  b.write_int(n)\n  return b.to_string()\n}\nfn main() -> i32 = 0\n");
        assert!(!d.has_errors(), "builder construction should check");
    }

    #[test]
    fn builder_to_string_consumes_use_after_move_rejected() {
        // `to_string()` consumes (moves) the builder; using it afterwards is a use-after-move.
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write(\"a\")\n  s := b.to_string()\n  t := b.to_string()\n  return 0\n}\n");
        assert!(d.has_errors(), "using a builder after to_string() must be rejected");
    }

    #[test]
    fn builder_write_wrong_arg_type_errors() {
        // `.write()` takes a str; an int is rejected (use `.write_int()`).
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write(42)\n  return 0\n}\n");
        assert!(d.has_errors(), "builder.write of a non-str must error");
    }

    #[test]
    fn builder_scalar_writers_check() {
        // MMv2 slice 7d: bool/char/float writers accept their matching scalar.
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write_int(1)\n  b.write_bool(true)\n  b.write_char('z')\n  b.write_float(2.5)\n  s := b.to_string()\n  return 0\n}\n");
        assert!(!d.has_errors(), "builder scalar writers should check");
    }

    #[test]
    fn builder_write_bool_rejects_non_bool() {
        // Each typed writer rejects a mismatched scalar (here `write_bool` of an int).
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write_bool(1)\n  return 0\n}\n");
        assert!(d.has_errors(), "write_bool of a non-bool must error");
    }

    #[test]
    fn builder_write_float_rejects_int() {
        let (_p, d) = check("fn main() -> i32 {\n  b := builder()\n  b.write_float(3)\n  return 0\n}\n");
        assert!(d.has_errors(), "write_float of an int must error (no implicit int->float)");
    }

    #[test]
    fn write_on_non_builder_errors() {
        // The builder methods are builder-only; calling `.write()` on a str is an error.
        let (_p, d) = check("fn main() -> i32 {\n  s := \"x\"\n  s.write(\"y\")\n  return 0\n}\n");
        assert!(d.has_errors(), "'.write()' on a non-builder must error");
    }

    #[test]
    fn string_borrow_returned_as_str_view_escapes() {
        // The borrow is `Frame`-regioned: a function that returns a borrow of its `str` arg, when
        // fed a borrowed `string`, would dangle (the string is freed at frame exit). The
        // call-result region tie (slice 6b) must catch this through the borrow.
        let (_p, d) = check("fn id(s: str) -> str = s\nfn mk(a: str) -> string = a.clone()\nfn leak() -> str {\n  owned := mk(\"x\")\n  return id(owned)\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "returning a str borrow of a frame-owned string must be rejected");
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
            "fn add(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  return [1, 2, 3].reduce(0, add)\n}\n",
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
    fn move_owned_local_through_if_arm_rejected() {
        // MMv2 slice 4.5: moving a *bound* owned array out through an `if`/`else` arm is a
        // deferred-feature error (codegen only nulls slots at direct move sites). A fresh
        // temporary through an `if` is fine — there is no bound slot to double-free.
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn pick(c: bool) -> array<i32> {\n  ys := [1, 2, 3].map(double).to_array()\n  zs := [4, 5, 6].map(double).to_array()\n  return if c { ys } else { zs }\n}\nfn main() -> i32 = 0\n");
        assert!(d.has_errors(), "moving a bound owned local out through an if/else arm must error");
    }

    #[test]
    fn conditional_move_then_no_later_use_checks() {
        // Moving an owned local on only one path (with no later use of the source) is allowed:
        // MIR nulls the slot at the move site so the not-moved path is still freed at exit.
        let (_p, d) = check("fn double(x: i32) -> i32 = x * 2\nfn run(c: bool) -> i32 {\n  ys := [1, 2, 3].map(double).to_array()\n  mut total := 0\n  if c {\n    zs := ys\n    total = zs.sum()\n  }\n  return total\n}\nfn main() -> i32 = run(true)\n");
        assert!(!d.has_errors(), "a one-path move with no later use of the source should check");
    }

    #[test]
    fn min_over_non_numeric_errors() {
        // `min`/`max` need a numeric element, like `sum`. A bool-producing map is rejected.
        let (_p, d) = check("fn isbig(x: i32) -> bool = x > 1\nfn main() -> i32 {\n  if [1, 2, 3].map(isbig).min() { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "min over a non-numeric element must error");
    }

    #[test]
    fn min_max_inline_checks() {
        let (_p, d) = check("fn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [3, 1, 2].map(id).min() + [3, 1, 2].map(id).max()\n}\n");
        assert!(!d.has_errors(), "min + max over an i32 pipeline should check");
    }

    #[test]
    fn scan_inline_checks() {
        // scan(init, f) with f: (i32, i32) -> i32 yields array<i32>; summing it checks.
        let (_p, d) = check("fn add(acc: i32, x: i32) -> i32 = acc + x\nfn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [1, 2, 3].map(id).scan(0, add).sum()\n}\n");
        assert!(!d.has_errors(), "scan(0, add) over an i32 pipeline should check");
    }

    #[test]
    fn scan_fn_arity_mismatch_errors() {
        // scan needs a 2-arg fold; a 1-arg function must error.
        let (_p, d) = check("fn bad(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [1, 2, 3].scan(0, bad).sum()\n}\n");
        assert!(d.has_errors(), "scan with a non-binary function must error");
    }

    #[test]
    fn sort_inline_checks() {
        let (_p, d) = check("fn id(x: i32) -> i32 = x\nfn h(acc: i32, x: i32) -> i32 = acc + x\nfn main() -> i32 {\n  return [3, 1, 2].map(id).sort().reduce(0, h)\n}\n");
        assert!(!d.has_errors(), "sort of a numeric pipeline should check");
    }

    #[test]
    fn sort_over_struct_element_rejected() {
        let (_p, d) = check("Point { x: i32, y: i32 }\nfn main() -> i32 {\n  s := [Point { x: 1, y: 2 }].sort()\n  return 0\n}\n");
        assert!(d.has_errors(), "sort over struct elements must error (project a field first)");
    }

    #[test]
    fn dot_length_mismatch_errors() {
        let (_p, d) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [4, 5]\n  return xs.dot(ys)\n}\n");
        assert!(d.has_errors(), "dot of unequal-length arrays must error");
    }

    #[test]
    fn dot_element_type_mismatch_errors() {
        // An int array dotted with a float array must error (no implicit numeric coercion).
        let (_p, d) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [1.0, 2.0, 3.0]\n  if xs.dot(ys) == 0 { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "dot of mismatched element types must error");
    }

    #[test]
    fn dot_arbitrary_right_operand_rejected_not_panicked() {
        // An `if` expression as the right operand is an arbitrary array expr; it must be
        // rejected in sema, not reach `array_source_slot` and panic in MIR.
        let (_p, d) = check("fn main() -> i32 {\n  xs := [1, 2, 3]\n  ys := [4, 5, 6]\n  zs := [7, 8, 9]\n  c := true\n  if xs.dot(if c { ys } else { zs }) == 32 { return 1 }\n  return 0\n}\n");
        assert!(d.has_errors(), "an arbitrary array expr as dot's right operand must error");
    }

    #[test]
    fn dot_inline_checks() {
        let (_p, d) = check("fn main() -> i32 {\n  xs := [2, 3, 4]\n  ys := [5, 6, 7]\n  if xs.dot(ys) == 56 { return 1 }\n  return 0\n}\n");
        assert!(!d.has_errors(), "dot of two equal-length i64 arrays should check");
    }

    #[test]
    fn scan_over_struct_element_rejected_not_panicked() {
        // A struct element (no field projection) must be rejected in sema, not panic in MIR.
        let (_p, d) = check("Point { x: i32, y: i32 }\nfn addx(acc: i32, p: Point) -> i32 = acc + p.x\nfn main() -> i32 {\n  return [Point { x: 1, y: 2 }].scan(0, addx).sum()\n}\n");
        assert!(d.has_errors(), "scan over struct elements must error (project a field first)");
    }

    #[test]
    fn scan_struct_accumulator_rejected() {
        // A struct accumulator (ty_to_scalar succeeds for structs) must be rejected explicitly.
        let (_p, d) = check("Acc { s: i32 }\nfn step(a: Acc, x: i32) -> Acc = Acc { s: a.s + x }\nfn id(x: i32) -> i32 = x\nfn main() -> i32 {\n  return [1, 2, 3].map(id).scan(Acc { s: 0 }, step).len()\n}\n");
        assert!(d.has_errors(), "scan with a struct accumulator must error");
    }

    #[test]
    fn reduce_fold_type_mismatch_errors() {
        // fold that takes the wrong element type.
        let (_p, d) = check(
            "fn add(acc: i32, x: bool) -> i32 = acc\nfn main() -> i32 {\n  return [1, 2, 3].reduce(0, add)\n}\n",
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
    fn main_arguments_only_array_str() {
        // `main(args: array<str>)` with a `Result<(), Error>` return is accepted (PR-C, argv).
        let (_p, ok) = check("pub fn main(args: array<str>) -> Result<(), Error> {\n  print(args.len())\n  return Ok(())\n}\n");
        assert!(!ok.has_errors(), "main(args: array<str>) -> Result should check");
        // Any other main parameter is rejected.
        let (_q, bad) = check("fn main(n: i32) -> i32 {\n  return n\n}\n");
        assert!(bad.has_errors(), "main with a non-`array<str>` argument must error");
        // `main(args)` must return Result (the only form the wrapper marshals argv into).
        let (_r, noresult) = check("fn main(args: array<str>) -> i32 = 0\n");
        assert!(noresult.has_errors(), "main(args) with a non-Result return must error");
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
