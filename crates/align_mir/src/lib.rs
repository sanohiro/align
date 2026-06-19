//! MIR: backend-agnostic intermediate representation (`docs/impl/04-mir.md`).
//!
//! Align's semantics (desugaring, fusion, SIMD-ization, arena) are settled here, and
//! `MIR -> LLVM` is restricted to pure lowering. Allocation / error paths / parallel
//! units remain explicit nodes ("nothing hidden").
//!
//! M1 model: each function is a CFG of basic blocks. Named locals (params + `let`) are
//! addressable **slots** (lowered to allocas), read via `Load` and written via `Store`;
//! expression temporaries are SSA-like [`ValueId`]s. `if` becomes branches + blocks,
//! using a result slot when it produces a value. fusion/SIMD/arena arrive with their
//! features.

use align_ast::{BinOp, UnOp};
use align_sema::{hir, FloatTy, IntTy, Ty};

pub mod print;

/// SSA-like temporary value (defined once).
pub type ValueId = u32;
/// Memory slot (a local variable; lowered to an alloca).
pub type Slot = u32;
pub type BlockId = u32;

#[derive(Clone, Debug)]
pub struct Program {
    pub fns: Vec<Function>,
    /// Struct layouts, indexed by the id in [`Ty::Struct`]; codegen builds LLVM struct
    /// types from these.
    pub structs: Vec<hir::StructDef>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    /// Slots holding the incoming parameters, in order.
    pub params: Vec<Slot>,
    pub ret: Ty,
    /// Type of every slot, indexed by [`Slot`].
    pub slots: Vec<Ty>,
    /// Type of every temporary, indexed by [`ValueId`].
    pub value_tys: Vec<Ty>,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
}

impl Function {
    /// The type produced by an operand.
    pub fn operand_ty(&self, op: &Operand) -> Ty {
        match op {
            Operand::Const(Const::Int(_, ty)) => *ty,
            Operand::Const(Const::Float(_, ty)) => *ty,
            Operand::Const(Const::Char(_)) => Ty::Char,
            Operand::Const(Const::Bool(_)) => Ty::Bool,
            Operand::Const(Const::Unit) => Ty::Unit,
            Operand::Value(v) => self.value_tys[*v as usize],
            Operand::Arg(i) => self.slots[self.params[*i as usize] as usize],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    /// `v = rvalue` (a temporary). A `Unit`-typed rvalue (e.g. a void call) has no value.
    Let(ValueId, Rvalue),
    /// `slot <- operand`.
    Store(Slot, Operand),
    /// `slot.field <- operand` (struct field store; `slot` holds a struct).
    StoreField(Slot, u32, Operand),
    /// `slot[index] <- value` (array element store).
    StoreIndex(Slot, Operand, Operand),
    /// `ptr[index] <- value` — store into a raw element pointer (the buffer of an owned
    /// `array<T>` being filled). The element type comes from `value`.
    PtrStore(Operand, Operand, Operand),
    /// `slot[index].field <- value` (struct-array element field store).
    StoreElemField(Slot, Operand, u32, Operand),
    /// End an arena, freeing all its allocations (the operand is the arena handle).
    ArenaEnd(Operand),
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    Use(Operand),
    Load(Slot),
    Un(UnOp, Operand),
    Bin(BinOp, Operand, Operand),
    Call(String, Vec<Operand>),
    /// Load field `index` from the struct in `slot`.
    Field(Slot, u32),
    /// `Some(operand)` — build an `Option` aggregate (tag = Some).
    OptionSome(Operand),
    /// `None` — build an `Option` aggregate (tag = None); the type is the value's.
    OptionNone,
    /// Whether an `Option` operand is `Some` (its tag).
    OptionIsSome(Operand),
    /// The payload of an `Option` operand (valid only when it is `Some`).
    OptionUnwrap(Operand),
    /// `Ok(operand)` — build a `Result` aggregate (tag = Ok); the type is the value's.
    ResultOk(Operand),
    /// `Err(operand)` — build a `Result` aggregate (tag = Err); the type is the value's.
    ResultErr(Operand),
    /// Whether a `Result` operand is `Ok` (its tag).
    ResultIsOk(Operand),
    /// The ok payload of a `Result` operand (valid only when `Ok`).
    ResultUnwrapOk(Operand),
    /// The err payload of a `Result` operand (valid only when `Err`).
    ResultUnwrapErr(Operand),
    /// Open a new arena; the value is its handle.
    ArenaBegin,
    /// `heap.new(init)` in an arena: bump-allocate, store `init`, yield the `box` pointer.
    /// First operand is the arena handle, second is the initial value.
    HeapAlloc(Operand, Operand),
    /// Read (copy) the value out of a `box` operand.
    BoxGet(Operand),
    /// Deep-copy a `box` into a fresh allocation. First operand is the arena handle,
    /// second is the source box.
    BoxClone(Operand, Operand),
    /// `slot[index]` — load an array element.
    Index(Slot, Operand),
    /// `slot[index].field` — load a field of a struct-array element.
    IndexField(Slot, Operand, u32),
    /// Borrow array `slot` (length `n`) as a slice value `{ &slot[0], n }`.
    MakeSlice(Slot, i128),
    /// Bump-allocate `count` elements of type `elem` in the arena `handle`; yields the
    /// element pointer (used to build an owned `array<T>` via [`Rvalue::MakeDynArray`]).
    ArenaAlloc { handle: Operand, count: Operand, elem: Ty },
    /// Build an owned `array<T>` value `{ ptr, len }` from a buffer pointer and a length.
    MakeDynArray { ptr: Operand, len: Operand },
    /// The `len` of a slice operand.
    SliceLen(Operand),
    /// `slice[index]` — load a slice element (scalar).
    SliceIndex(Operand, Operand),
    /// A string literal — a `str` view `{ &bytes, len }` over a constant.
    StrLit(String),
    /// `template "..."` / `str + str` — build a `str` from pieces. The optional operand
    /// is the enclosing arena handle (the result lives there; `None` = leaked).
    Template(Vec<TemplatePiece>, Option<Operand>),
    /// `json.decode` into struct `struct_id`: parse the `str` `input` and fill the `out`
    /// struct slot. Yields an `i32` status (0 = ok). codegen builds the field table (names,
    /// type tags, byte offsets) and calls the runtime parser.
    JsonDecode { struct_id: u32, input: Operand, out: Slot },
}

/// One piece of a lowered `template`: a static run, or an interpolated value.
#[derive(Clone, Debug)]
pub enum TemplatePiece {
    Static(String),
    IntHole(Operand),
    StrHole(Operand),
    BoolHole(Operand),
    CharHole(Operand),
    /// A float hole; codegen picks f32/f64 from the operand's type.
    FloatHole(Operand),
    /// A `str` operand emitted as a JSON string literal (quoted + escaped). From `json.encode`.
    JsonStrHole(Operand),
}

#[derive(Clone, Debug)]
pub enum Operand {
    Const(Const),
    Value(ValueId),
    /// The i-th incoming function argument.
    Arg(u32),
}

#[derive(Clone, Copy, Debug)]
pub enum Const {
    Int(i128, Ty),
    Float(f64, Ty),
    Char(u32),
    Bool(bool),
    /// The unit value `()`.
    Unit,
}

#[derive(Clone, Debug)]
pub enum Term {
    Goto(BlockId),
    Branch(Operand, BlockId, BlockId),
    Return(Option<Operand>),
    Unreachable,
}

/// typed HIR -> MIR.
pub fn lower_program(program: &hir::Program) -> Program {
    Program {
        fns: program.fns.iter().map(lower_fn).collect(),
        structs: program.structs.clone(),
    }
}

struct BBuild {
    stmts: Vec<Stmt>,
    term: Option<Term>,
}

struct Builder {
    slots: Vec<Ty>,
    value_tys: Vec<Ty>,
    blocks: Vec<BBuild>,
    cur: BlockId,
    /// The enclosing function's return type (so `?` can build the propagated Result).
    ret: Ty,
    /// Handles of the arenas currently open (innermost last); any exit out of them
    /// (`return`, `?`) must free them first.
    arenas: Vec<ValueId>,
}

impl Builder {
    /// Free every open arena (innermost first) — emitted before any exit that leaves
    /// the arena scopes.
    fn emit_arena_cleanup(&mut self) {
        let handles = self.arenas.clone();
        for h in handles.into_iter().rev() {
            self.push(Stmt::ArenaEnd(Operand::Value(h)));
        }
    }
}

impl Builder {
    fn new_block(&mut self) -> BlockId {
        let id = self.blocks.len() as BlockId;
        self.blocks.push(BBuild {
            stmts: Vec::new(),
            term: None,
        });
        id
    }

    fn fresh_value(&mut self, ty: Ty) -> ValueId {
        let v = self.value_tys.len() as ValueId;
        self.value_tys.push(ty);
        v
    }

    fn new_slot(&mut self, ty: Ty) -> Slot {
        let s = self.slots.len() as Slot;
        self.slots.push(ty);
        s
    }

    fn push(&mut self, s: Stmt) {
        self.blocks[self.cur as usize].stmts.push(s);
    }

    fn terminate(&mut self, t: Term) {
        let b = &mut self.blocks[self.cur as usize];
        if b.term.is_none() {
            b.term = Some(t);
        }
    }

    fn is_terminated(&self) -> bool {
        self.blocks[self.cur as usize].term.is_some()
    }
}

fn lower_fn(f: &hir::Fn) -> Function {
    let mut b = Builder {
        slots: f.locals.iter().map(|l| l.ty).collect(),
        value_tys: Vec::new(),
        blocks: Vec::new(),
        cur: 0,
        ret: f.ret,
        arenas: Vec::new(),
    };
    let entry = b.new_block();
    b.cur = entry;

    // Slot index == HIR LocalId (locals are created in id order).
    let params: Vec<Slot> = f.params.clone();
    for (i, &slot) in params.iter().enumerate() {
        b.push(Stmt::Store(slot, Operand::Arg(i as u32)));
    }

    let tail = lower_block(&mut b, &f.body);
    if !b.is_terminated() {
        match tail {
            Some(op) if f.ret != Ty::Unit => b.terminate(Term::Return(Some(op))),
            _ => b.terminate(Term::Return(None)),
        }
    }

    let blocks = b
        .blocks
        .into_iter()
        .enumerate()
        .map(|(id, bb)| Block {
            id: id as BlockId,
            stmts: bb.stmts,
            term: bb.term.unwrap_or(Term::Unreachable),
        })
        .collect();

    Function {
        name: f.name.clone(),
        params,
        ret: f.ret,
        slots: b.slots,
        value_tys: b.value_tys,
        blocks,
        entry,
    }
}

/// Lower a block; returns its trailing value operand if any. If a statement diverges
/// (e.g. `return`), the current block becomes terminated and the rest of the block —
/// including its trailing value — is dead code and is not lowered.
fn lower_block(b: &mut Builder, block: &hir::Block) -> Option<Operand> {
    for s in &block.stmts {
        lower_stmt(b, s);
        if b.is_terminated() {
            return None;
        }
    }
    block.value.as_ref().map(|e| lower_expr(b, e))
}

fn lower_stmt(b: &mut Builder, s: &hir::Stmt) {
    match s {
        hir::Stmt::Let { local, init } => match &init.kind {
            // A struct literal initializes its slot field by field; there is no scalar
            // value to bind.
            hir::ExprKind::StructLit { fields, .. } => {
                for (i, fe) in fields.iter().enumerate() {
                    let op = lower_expr(b, fe);
                    b.push(Stmt::StoreField(*local, i as u32, op));
                }
            }
            // An array literal stores its elements into the slot.
            hir::ExprKind::ArrayLit { elems, elem } => store_array_elems(b, *local, elems, *elem),
            _ => {
                let op = lower_expr(b, init);
                b.push(Stmt::Store(*local, op));
            }
        },
        hir::Stmt::Assign { local, value } => {
            let op = lower_expr(b, value);
            b.push(Stmt::Store(*local, op));
        }
        hir::Stmt::AssignField { base, index, value } => {
            let op = lower_expr(b, value);
            b.push(Stmt::StoreField(*base, *index, op));
        }
        hir::Stmt::Return(value) => {
            let op = value.as_ref().map(|e| lower_expr(b, e));
            // Free any arenas this return exits before leaving the function.
            b.emit_arena_cleanup();
            b.terminate(Term::Return(op));
            // The current block is now terminated; `lower_block` stops here, so no dead
            // block is created and callers can see the divergence via `is_terminated`.
        }
        hir::Stmt::Expr(e) => {
            let _ = lower_expr(b, e);
        }
    }
}

fn lower_expr(b: &mut Builder, e: &hir::Expr) -> Operand {
    match &e.kind {
        hir::ExprKind::Unit => Operand::Const(Const::Unit),
        hir::ExprKind::Int(v) => Operand::Const(Const::Int(*v, e.ty)),
        hir::ExprKind::Float(v) => Operand::Const(Const::Float(*v, e.ty)),
        hir::ExprKind::Char(v) => Operand::Const(Const::Char(*v)),
        hir::ExprKind::Str(s) => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::StrLit(s.clone())));
            Operand::Value(v)
        }
        hir::ExprKind::Template(parts) => {
            let mut pieces = Vec::new();
            for p in parts {
                match p {
                    hir::TemplatePart::Text(s) => pieces.push(TemplatePiece::Static(s.clone())),
                    hir::TemplatePart::Hole(h) => {
                        let ty = h.ty;
                        let op = lower_expr(b, h);
                        pieces.push(match ty {
                            Ty::Str => TemplatePiece::StrHole(op),
                            Ty::Bool => TemplatePiece::BoolHole(op),
                            Ty::Char => TemplatePiece::CharHole(op),
                            Ty::Float(_) => TemplatePiece::FloatHole(op),
                            _ => TemplatePiece::IntHole(op),
                        });
                    }
                    hir::TemplatePart::JsonStr(h) => {
                        let op = lower_expr(b, h);
                        pieces.push(TemplatePiece::JsonStrHole(op));
                    }
                }
            }
            let arena = b.arenas.last().map(|h| Operand::Value(*h));
            let r = b.fresh_value(e.ty);
            b.push(Stmt::Let(r, Rvalue::Template(pieces, arena)));
            Operand::Value(r)
        }
        hir::ExprKind::JsonDecode { struct_id, input } => lower_json_decode(b, *struct_id, input, e.ty),
        hir::ExprKind::Bool(v) => Operand::Const(Const::Bool(*v)),
        hir::ExprKind::Local(id) => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Load(*id)));
            Operand::Value(v)
        }
        hir::ExprKind::Unary { op, expr } => {
            let a = lower_expr(b, expr);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Un(*op, a)));
            Operand::Value(v)
        }
        hir::ExprKind::Binary { op, lhs, rhs } => {
            let l = lower_expr(b, lhs);
            let r = lower_expr(b, rhs);
            // `str + str` is concatenation, built like a two-piece template.
            if *op == BinOp::Add && lhs.ty == Ty::Str {
                let arena = b.arenas.last().map(|h| Operand::Value(*h));
                let v = b.fresh_value(e.ty);
                b.push(Stmt::Let(
                    v,
                    Rvalue::Template(vec![TemplatePiece::StrHole(l), TemplatePiece::StrHole(r)], arena),
                ));
                return Operand::Value(v);
            }
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Bin(*op, l, r)));
            Operand::Value(v)
        }
        hir::ExprKind::Call { func, args } => {
            let ops = args.iter().map(|a| lower_expr(b, a)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Call(func.clone(), ops)));
            Operand::Value(v)
        }
        hir::ExprKind::If { cond, then, els } => lower_if(b, cond, then, els, e.ty),
        hir::ExprKind::Field { base, index } => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Field(*base, *index)));
            Operand::Value(v)
        }
        hir::ExprKind::IndexField { base, index, field } => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::IndexField(*base, index_const(*index as usize), *field)));
            Operand::Value(v)
        }
        hir::ExprKind::Block(blk) => {
            lower_block(b, blk).unwrap_or(Operand::Const(Const::Bool(false)))
        }
        hir::ExprKind::OptionSome(inner) => {
            let op = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::OptionSome(op)));
            Operand::Value(v)
        }
        hir::ExprKind::OptionNone => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::OptionNone));
            Operand::Value(v)
        }
        hir::ExprKind::ElseUnwrap { opt, fallback } => lower_else_unwrap(b, opt, fallback, e.ty),
        hir::ExprKind::ResultOk(inner) => {
            let op = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ResultOk(op)));
            Operand::Value(v)
        }
        hir::ExprKind::ResultErr(inner) => {
            let op = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::ResultErr(op)));
            Operand::Value(v)
        }
        hir::ExprKind::Try(inner) => lower_try(b, inner, e.ty),
        hir::ExprKind::Arena(blk) => {
            let handle = b.fresh_value(Ty::ArenaHandle);
            b.push(Stmt::Let(handle, Rvalue::ArenaBegin));
            b.arenas.push(handle);
            let tail = lower_block(b, blk);
            b.arenas.pop();
            if b.is_terminated() {
                // The body diverged (return/?): cleanup already ran on that path.
                Operand::Const(Const::Unit)
            } else {
                b.push(Stmt::ArenaEnd(Operand::Value(handle)));
                tail.unwrap_or(Operand::Const(Const::Unit))
            }
        }
        hir::ExprKind::HeapNew(inner) => {
            let init = lower_expr(b, inner);
            let handle = *b.arenas.last().expect("heap.new outside an arena (sema-checked)");
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::HeapAlloc(Operand::Value(handle), init)));
            Operand::Value(v)
        }
        hir::ExprKind::BoxGet(inner) => {
            let bx = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BoxGet(bx)));
            Operand::Value(v)
        }
        hir::ExprKind::BoxClone(inner) => {
            let src = lower_expr(b, inner);
            let handle = *b.arenas.last().expect("clone outside an arena (sema-checked)");
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BoxClone(Operand::Value(handle), src)));
            Operand::Value(v)
        }
        hir::ExprKind::ArraySum { source, stages } => {
            let init = zero_of(e.ty);
            lower_array_reduce(b, source, stages, e.ty, init, Reducer::Sum)
        }
        hir::ExprKind::ArrayCount { source, stages } => {
            // i64 accumulator seeded at 0; each surviving element adds 1.
            let init = Operand::Const(Const::Int(0, i64_ty()));
            lower_array_reduce(b, source, stages, i64_ty(), init, Reducer::Count)
        }
        hir::ExprKind::ArrayReduce { source, stages, func, init } => {
            let init_op = lower_expr(b, init);
            lower_array_reduce(b, source, stages, e.ty, init_op, Reducer::Fold(func.clone()))
        }
        hir::ExprKind::ArrayAnyAll { source, stages, func, all } => {
            // bool accumulator: `all` seeds true (&&-fold), `any` seeds false (||-fold).
            let init = Operand::Const(Const::Bool(*all));
            lower_array_reduce(b, source, stages, Ty::Bool, init, Reducer::AnyAll { func: func.clone(), all: *all })
        }
        hir::ExprKind::ArrayToArray { source, stages, elem } => {
            lower_array_collect(b, source, stages, *elem)
        }
        hir::ExprKind::ArrayToSlice(inner) => {
            let (slot, n) = array_source_slot(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeSlice(slot, n)));
            Operand::Value(v)
        }
        hir::ExprKind::Len(inner) => {
            // `str`/`slice` carry the length in their `{ ptr, len }` view.
            let sv = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::SliceLen(sv)));
            Operand::Value(v)
        }
        hir::ExprKind::ArrayLit { .. } => {
            unreachable!("array literal only appears as a let initializer or pipeline source")
        }
        // A struct literal in value position (return/arg/assign): materialize it into a
        // temp slot field by field, then load the whole struct. (A `let` initializer stores
        // straight into its own slot — see `lower_stmt` — avoiding this copy.)
        hir::ExprKind::StructLit { fields, .. } => {
            let slot = b.new_slot(e.ty);
            for (i, fe) in fields.iter().enumerate() {
                let op = lower_expr(b, fe);
                b.push(Stmt::StoreField(slot, i as u32, op));
            }
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
    }
}

/// The i64 type used for array indices / loop counters.
fn i64_ty() -> Ty {
    Ty::Int(IntTy { bits: 64, signed: true })
}

fn index_const(i: usize) -> Operand {
    Operand::Const(Const::Int(i as i128, i64_ty()))
}

/// Zero of a numeric scalar type (the identity for `sum`).
fn zero_of(ty: Ty) -> Operand {
    match ty {
        Ty::Float(_) => Operand::Const(Const::Float(0.0, ty)),
        _ => Operand::Const(Const::Int(0, ty)),
    }
}

/// Resolve an array-typed source expression to a slot holding it (materializing a
/// literal), returning `(slot, length)`.
fn array_source_slot(b: &mut Builder, source: &hir::Expr) -> (Slot, i128) {
    match &source.kind {
        hir::ExprKind::ArrayLit { elems, elem } => {
            let slot = b.new_slot(source.ty);
            store_array_elems(b, slot, elems, *elem);
            (slot, elems.len() as i128)
        }
        hir::ExprKind::Local(id) => {
            let n = match source.ty {
                Ty::Array(_, n) | Ty::StructArray(_, n) => n as i128,
                _ => 0,
            };
            (*id, n)
        }
        _ => unreachable!("array source must be a literal or a local in M4"),
    }
}

/// Store an array literal's elements into `slot`: scalar arrays write each element by
/// index; struct arrays write each element's fields (the elements are struct literals).
fn store_array_elems(b: &mut Builder, slot: Slot, elems: &[hir::Expr], elem: Ty) {
    if matches!(elem, Ty::Struct(_)) {
        for (i, e) in elems.iter().enumerate() {
            if let hir::ExprKind::StructLit { fields, .. } = &e.kind {
                for (j, fe) in fields.iter().enumerate() {
                    let v = lower_expr(b, fe);
                    b.push(Stmt::StoreElemField(slot, index_const(i), j as u32, v));
                }
            }
        }
    } else {
        for (i, e) in elems.iter().enumerate() {
            let v = lower_expr(b, e);
            b.push(Stmt::StoreIndex(slot, index_const(i), v));
        }
    }
}

/// `src.map(f).where(p)….{sum,reduce}` → one loop folding the post-stage elements into
/// an accumulator. `fold` is the binary reducer (`None` = `+`), `init` seeds the
/// accumulator (type `acc_ty`). Stages run inline (fusion); a failing `where` skips to
/// the increment, so no intermediate array is built.
/// How a fused pipeline's surviving elements combine into the result.
enum Reducer {
    /// `sum`: `acc + element`.
    Sum,
    /// `count`: `acc + 1` (element value ignored).
    Count,
    /// `reduce(f, init)`: `f(acc, element)`.
    Fold(String),
    /// `any(p)` / `all(p)`: `acc || p(element)` / `acc && p(element)`.
    AnyAll { func: String, all: bool },
}

/// The set-up of a pipeline source: a stack array (slot + const length), a struct array
/// (slot), or a `{ptr,len}`-shaped value — a `slice` or an owned `array` (operand + runtime
/// length). Shared by the reducing and collecting loops.
struct SrcSetup {
    slot: Slot,
    slice_val: Option<Operand>,
    bound: Operand,
    scalar_slot: bool,
}

fn setup_source(b: &mut Builder, source: &hir::Expr) -> SrcSetup {
    match source.ty {
        // `slice<T>` and owned `array<T>` share the `{ptr,len}` layout and runtime length.
        Ty::Slice(_) | Ty::DynArray(_) => {
            let sv = lower_expr(b, source);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            SrcSetup { slot: 0, slice_val: Some(sv), bound: Operand::Value(len), scalar_slot: false }
        }
        _ => {
            let (slot, n) = array_source_slot(b, source);
            SrcSetup {
                slot,
                slice_val: None,
                bound: Operand::Const(Const::Int(n, i64_ty())),
                scalar_slot: matches!(source.ty, Ty::Array(..)),
            }
        }
    }
}

fn lower_array_reduce(
    b: &mut Builder,
    source: &hir::Expr,
    stages: &[hir::Stage],
    acc_ty: Ty,
    init: Operand,
    reducer: Reducer,
) -> Operand {
    let elem_ty = acc_ty;
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src } = setup_source(b, source);

    let acc = b.new_slot(acc_ty);
    b.push(Stmt::Store(acc, init));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < len
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound)));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: address element i, run the stages, accumulate.
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    // A scalar array or a slice loads the element up front; a struct array stays
    // addressed by index until a `.field` projection loads a scalar.
    let mut cur: Option<Operand> = if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            _ => elem_ty,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::SliceIndex(sv.clone(), index.clone())));
        Some(Operand::Value(x))
    } else if scalar_slot_src {
        let src_elem = match source.ty {
            Ty::Array(s, _) => align_sema::scalar_to_ty(s),
            _ => elem_ty,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::Index(slot, index.clone())));
        Some(Operand::Value(x))
    } else {
        None
    };

    for stage in stages {
        match &stage.kind {
            hir::StageKind::Project { field } => {
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::IndexField(slot, index.clone(), *field)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func } => {
                let arg = cur.take().expect("map before projection");
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), vec![arg])));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func } => {
                let arg = cur.clone().expect("where before projection");
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), vec![arg])));
                let keep = b.new_block();
                // false → skip this element (go to the increment).
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                // Predicate on a struct element's (bool) field; the element is unchanged.
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::IndexField(slot, index.clone(), *field)));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }
    let a = b.fresh_value(acc_ty);
    b.push(Stmt::Let(a, Rvalue::Load(acc)));
    let next = b.fresh_value(acc_ty);
    match &reducer {
        // `count`: acc = acc + 1 (the element value is irrelevant).
        Reducer::Count => b.push(Stmt::Let(next, Rvalue::Bin(BinOp::Add, Operand::Value(a), index_const(1)))),
        // `sum`: acc = acc + cur.
        Reducer::Sum => {
            let cur = cur.expect("sum needs a scalar element");
            b.push(Stmt::Let(next, Rvalue::Bin(BinOp::Add, Operand::Value(a), cur)));
        }
        // `reduce`: acc = f(acc, cur).
        Reducer::Fold(func) => {
            let cur = cur.expect("reduce needs a scalar element");
            b.push(Stmt::Let(next, Rvalue::Call(func.clone(), vec![Operand::Value(a), cur])));
        }
        // `any`/`all`: t = p(cur); acc = acc || t  /  acc && t.
        Reducer::AnyAll { func, all } => {
            let cur = cur.expect("any/all needs a scalar element");
            let t = b.fresh_value(Ty::Bool);
            b.push(Stmt::Let(t, Rvalue::Call(func.clone(), vec![cur])));
            let op = if *all { BinOp::And } else { BinOp::Or };
            b.push(Stmt::Let(next, Rvalue::Bin(op, Operand::Value(a), Operand::Value(t))));
        }
    }
    b.push(Stmt::Store(acc, Operand::Value(next)));
    b.terminate(Term::Goto(cont));

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    b.cur = exit;
    let r = b.fresh_value(elem_ty);
    b.push(Stmt::Let(r, Rvalue::Load(acc)));
    Operand::Value(r)
}

/// `source.….to_array()` — the fused loop, but each surviving element is appended to a
/// freshly bump-allocated arena buffer instead of folded. Yields an owned `array<T>` value
/// `{ ptr, len }` where `len` is the survivor count. (MMv2 slice 3; arena-only.)
fn lower_array_collect(b: &mut Builder, source: &hir::Expr, stages: &[hir::Stage], elem: Ty) -> Operand {
    let handle = Operand::Value(*b.arenas.last().expect("to_array outside an arena (sema-checked)"));
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src } = setup_source(b, source);

    // Output buffer: `bound` (upper-bound = source length) elements in the arena. map/where
    // never grow the count, so the buffer never needs to be resized.
    let out_ptr = b.fresh_value(Ty::Box(scalar_of(elem)));
    b.push(Stmt::Let(out_ptr, Rvalue::ArenaAlloc { handle, count: bound.clone(), elem }));

    // `acc` is the running output index (= final length); `iv` is the source index.
    let acc = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc, Operand::Const(Const::Int(0, i64_ty()))));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < len
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound)));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: address element i, run the stages, append survivors.
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    let mut cur: Option<Operand> = if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::SliceIndex(sv.clone(), index.clone())));
        Some(Operand::Value(x))
    } else if scalar_slot_src {
        let src_elem = match source.ty {
            Ty::Array(s, _) => align_sema::scalar_to_ty(s),
            _ => elem,
        };
        let x = b.fresh_value(src_elem);
        b.push(Stmt::Let(x, Rvalue::Index(slot, index.clone())));
        Some(Operand::Value(x))
    } else {
        None
    };

    for stage in stages {
        match &stage.kind {
            hir::StageKind::Project { field } => {
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::IndexField(slot, index.clone(), *field)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func } => {
                let arg = cur.take().expect("map before projection");
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), vec![arg])));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func } => {
                let arg = cur.clone().expect("where before projection");
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), vec![arg])));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::IndexField(slot, index.clone(), *field)));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }

    // append: out_ptr[out_idx] = cur; out_idx += 1.
    let cur = cur.expect("to_array needs a scalar element");
    let out_idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(out_idx, Rvalue::Load(acc)));
    b.push(Stmt::PtrStore(Operand::Value(out_ptr), Operand::Value(out_idx), cur));
    let next = b.fresh_value(i64_ty());
    b.push(Stmt::Let(next, Rvalue::Bin(BinOp::Add, Operand::Value(out_idx), index_const(1))));
    b.push(Stmt::Store(acc, Operand::Value(next)));
    b.terminate(Term::Goto(cont));

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    // exit: build the owned array { out_ptr, out_idx }.
    b.cur = exit;
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::Load(acc)));
    let arr = b.fresh_value(Ty::DynArray(scalar_of(elem)));
    b.push(Stmt::Let(arr, Rvalue::MakeDynArray { ptr: Operand::Value(out_ptr), len: Operand::Value(len) }));
    Operand::Value(arr)
}

/// The scalar of a known-scalar element `Ty` (panics on a non-scalar — `to_array` is
/// sema-restricted to scalar elements).
fn scalar_of(ty: Ty) -> align_sema::Scalar {
    align_sema::ty_to_scalar(ty).expect("to_array element must be a scalar (sema-checked)")
}

/// `json.decode(input)` → fill an out struct via the runtime parser (status `i32`), then
/// branch into `Ok(<struct>)` on status 0 or `Err(<code>)` otherwise, yielding the Result.
fn lower_json_decode(b: &mut Builder, struct_id: u32, input: &hir::Expr, result_ty: Ty) -> Operand {
    let sty = Ty::Struct(struct_id);
    let out = b.new_slot(sty);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(Ty::ErrCode);
    b.push(Stmt::Let(code, Rvalue::JsonDecode { struct_id, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, Ty::ErrCode)))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the filled struct and wrap it.
    b.cur = ok_bb;
    let s = b.fresh_value(sty);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code as the Error.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(Operand::Value(code))));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `expr?` → branch on the Result tag. `Err` propagates (early-return an `Err` of the
/// function's own return type — the cold edge); `Ok` continues with the unwrapped value.
fn lower_try(b: &mut Builder, inner: &hir::Expr, ok_ty: Ty) -> Operand {
    let ret_err_ty = match b.ret {
        Ty::Result(_, e) => align_sema::scalar_to_ty(e),
        _ => Ty::Error,
    };
    let r = lower_expr(b, inner);

    let is_ok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_ok, Rvalue::ResultIsOk(r.clone())));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    // NOTE: the Err edge is the designed "cold" path, but this is a plain branch — LLVM
    // branch-weight / cold metadata is not emitted yet (a later codegen optimization).
    b.terminate(Term::Branch(Operand::Value(is_ok), ok_bb, err_bb));

    // Err: extract the error and early-return Err(err) of the function's return type.
    b.cur = err_bb;
    let err = b.fresh_value(ret_err_ty);
    b.push(Stmt::Let(err, Rvalue::ResultUnwrapErr(r.clone())));
    let propagated = b.fresh_value(b.ret);
    b.push(Stmt::Let(propagated, Rvalue::ResultErr(Operand::Value(err))));
    // `?` exits the function: free any open arenas first.
    b.emit_arena_cleanup();
    b.terminate(Term::Return(Some(Operand::Value(propagated))));

    // Ok: continue with the unwrapped value.
    b.cur = ok_bb;
    let v = b.fresh_value(ok_ty);
    b.push(Stmt::Let(v, Rvalue::ResultUnwrapOk(r)));
    Operand::Value(v)
}

/// `opt else fallback` → branch on the Option tag; `Some` unwraps the payload into the
/// result slot, `None` evaluates the fallback (which writes the slot or diverges).
fn lower_else_unwrap(b: &mut Builder, opt: &hir::Expr, fallback: &hir::Expr, ty: Ty) -> Operand {
    let result_slot = b.new_slot(ty);
    let opt_op = lower_expr(b, opt);

    let is_some = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_some, Rvalue::OptionIsSome(opt_op.clone())));
    let some_bb = b.new_block();
    let none_bb = b.new_block();
    let join_bb = b.new_block();
    b.terminate(Term::Branch(Operand::Value(is_some), some_bb, none_bb));

    // Some: unwrap the payload into the result slot.
    b.cur = some_bb;
    let val = b.fresh_value(ty);
    b.push(Stmt::Let(val, Rvalue::OptionUnwrap(opt_op)));
    b.push(Stmt::Store(result_slot, Operand::Value(val)));
    b.terminate(Term::Goto(join_bb));

    // None: the fallback yields the value, or diverges (then the block is already
    // terminated and the store/goto are skipped).
    b.cur = none_bb;
    let fb = lower_expr(b, fallback);
    if !b.is_terminated() {
        b.push(Stmt::Store(result_slot, fb));
        b.terminate(Term::Goto(join_bb));
    }

    b.cur = join_bb;
    let r = b.fresh_value(ty);
    b.push(Stmt::Let(r, Rvalue::Load(result_slot)));
    Operand::Value(r)
}

fn lower_if(
    b: &mut Builder,
    cond: &hir::Expr,
    then: &hir::Block,
    els: &hir::Block,
    ty: Ty,
) -> Operand {
    let result_slot = (ty != Ty::Unit).then(|| b.new_slot(ty));

    let c = lower_expr(b, cond);
    let then_bb = b.new_block();
    let else_bb = b.new_block();
    let join_bb = b.new_block();
    b.terminate(Term::Branch(c, then_bb, else_bb));

    b.cur = then_bb;
    let tv = lower_block(b, then);
    if let (Some(slot), Some(op)) = (result_slot, tv) {
        b.push(Stmt::Store(slot, op));
    }
    b.terminate(Term::Goto(join_bb));

    b.cur = else_bb;
    let ev = lower_block(b, els);
    if let (Some(slot), Some(op)) = (result_slot, ev) {
        b.push(Stmt::Store(slot, op));
    }
    b.terminate(Term::Goto(join_bb));

    b.cur = join_bb;
    match result_slot {
        Some(slot) => {
            let v = b.fresh_value(ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
        // Unit if: value is unused by the caller (statement position).
        None => Operand::Const(Const::Bool(false)),
    }
}

/// A short type name used in MIR text / diagnostics.
pub fn ty_name(ty: Ty) -> String {
    match ty {
        Ty::Int(IntTy { bits, signed }) => format!("{}{}", if signed { 'i' } else { 'u' }, bits),
        Ty::IntVar(_) => "int?".to_string(),
        Ty::Float(FloatTy { bits }) => format!("f{bits}"),
        Ty::FloatVar(_) => "float?".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Option(_) => "Option".to_string(),
        Ty::Result(..) => "Result".to_string(),
        Ty::Box(_) => "box".to_string(),
        Ty::Array(_, n) | Ty::StructArray(_, n) => format!("array[{n}]"),
        Ty::Slice(_) => "slice".to_string(),
        Ty::DynArray(_) => "array".to_string(),
        Ty::Str => "str".to_string(),
        Ty::ArenaHandle => "arena".to_string(),
        Ty::ErrCode => "Error".to_string(),
        Ty::Struct(id) => format!("struct#{id}"),
        Ty::Unit => "()".to_string(),
        Ty::Error => "<error>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_diag::Diagnostics;
    use align_lexer::tokenize;
    use align_parser::parse_file;
    use align_sema::check_file;

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
        // entry stores the literal into x's slot; a later block returns the loaded value.
        assert!(f.blocks.iter().any(|b| matches!(b.term, Term::Return(Some(_)))));
    }

    #[test]
    fn if_creates_branch() {
        let p = lower("fn f(n: i64) -> i64 {\n  if n < 2 { return n }\n  return n\n}\n");
        let f = &p.fns[0];
        assert!(f.blocks.iter().any(|b| matches!(b.term, Term::Branch(..))));
    }

    #[test]
    fn struct_lowers_to_field_stores_and_loads() {
        let src = "Point { x: i32, y: i32 }\nfn main() -> i32 {\n  p := Point { x: 3, y: 4 }\n  return p.x + p.y\n}\n";
        let p = lower(src);
        assert_eq!(p.structs.len(), 1);
        let f = &p.fns[0];
        let stmts: Vec<&Stmt> = f.blocks.iter().flat_map(|b| &b.stmts).collect();
        // Two field stores for the literal, two field loads for the reads.
        assert_eq!(stmts.iter().filter(|s| matches!(s, Stmt::StoreField(..))).count(), 2);
        assert_eq!(
            stmts
                .iter()
                .filter(|s| matches!(s, Stmt::Let(_, Rvalue::Field(..))))
                .count(),
            2
        );
    }
}
