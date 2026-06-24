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
use align_sema::{hir, payload_is_move, FloatTy, IntTy, Layout, Ty};

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
    /// Sum-type layouts, indexed by the id in [`Ty::Enum`]; codegen builds the tagged struct
    /// `{ i32 tag, … }` from each (variant payloads + `field_base`).
    pub enums: Vec<hir::EnumDef>,
    /// Tuple layouts, indexed by the id in [`Ty::Tuple`]; codegen builds an anonymous LLVM
    /// struct type from each element list.
    pub tuples: Vec<hir::TupleDef>,
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
    /// Run all deferred tasks of a `task_group` and clear the list (`wait()`). Operand = the
    /// task-group handle. ④b-1 runs them sequentially; ④b-2 joins threads.
    TgWait(Operand),
    /// End a `task_group`, freeing its region (the operand is the task-group handle).
    TgEnd(Operand),
    /// Null-initialise an owned-array slot (`{null, 0}`) so a later [`Stmt::Drop`] on a path
    /// that never allocated is a no-op (MMv2 slice 4 drop-flag-via-null-slot).
    DropFlagInit(Slot),
    /// Null one owned field (`{null, 0}`) of a tuple slot, after a partial field move (`a := t.0`)
    /// took its buffer — so the tuple's exit `Drop` frees null there, not the buffer now owned by
    /// the new binding. The other fields are untouched.
    NullTupleField(Slot, u32),
    /// Drop a free-standing owned `array<T>` slot: free its buffer (null-safe).
    Drop(Slot),
    /// Free the buffer of a free-standing owned `array<T>` *value* (a `{ptr,len}` operand that
    /// is not backed by a slot — an unbound `.to_array()` temporary consumed in place). Used to
    /// free the materialized buffer right after the loop that consumes it (null-safe).
    DropValue(Operand),
}

#[derive(Clone, Debug)]
pub enum Rvalue {
    Use(Operand),
    Load(Slot),
    Un(UnOp, Operand),
    Bin(BinOp, Operand, Operand),
    /// Explicit-overflow integer arithmetic (`core.math`): `op` is `Add`/`Sub`/`Mul` on the
    /// integer type `int_ty`. `Saturating` → the clamped result (`int_ty`); `Checked` → an
    /// `Option<int_ty>` (`None` on overflow). Lowers to the LLVM `{s,u}OP.sat` / `{s,u}OP.with.overflow`
    /// intrinsics (signedness from `int_ty`).
    IntArith { op: BinOp, mode: align_sema::ArithMode, int_ty: Ty, a: Operand, b: Operand },
    /// A scalar math builtin (`core.math`): `abs` (1 operand) / `min` / `max` (2). `ty` is the
    /// numeric operand/result type; lowers to the matching LLVM intrinsic (signedness/float from `ty`).
    MathOp { fn_: align_sema::MathFn, ty: Ty, operands: Vec<Operand> },
    Call(String, Vec<Operand>),
    /// The address of a top-level function as a value (`Ty::Fn`) — a function pointer.
    FnAddr(String),
    /// A capturing closure value: the lifted function `lifted` (which takes the captures as
    /// trailing parameters) plus the captured values. Codegen copies the captures into a
    /// frame-local environment and builds `{ thunk_ptr, env_ptr }`, where the thunk unpacks the
    /// env and forwards to `lifted`. `capture_tys` give the env layout.
    Closure { lifted: String, captures: Vec<Operand>, capture_tys: Vec<Ty> },
    /// An indirect call through a function-value `callee` (a `Ty::Fn` pointer). `param_tys`/`ret_ty`
    /// give codegen the LLVM function type for the indirect `call` (taken from the checked args /
    /// result type — no signature table needed).
    CallIndirect { callee: Operand, args: Vec<Operand>, param_tys: Vec<Ty>, ret_ty: Ty },
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
    /// `Type.Variant(payload…)` — build a sum-type aggregate `{ i32 tag, … }`: store the variant
    /// tag in field 0 and each payload operand in this variant's fields.
    MakeEnum { enum_id: u32, variant: u32, payload: Vec<Operand> },
    /// Whether a sum-type operand's tag equals `variant` (the `match`-arm test).
    EnumTagEq { enum_id: u32, scrutinee: Operand, variant: u32 },
    /// The `slot`-th payload field of a sum-type operand for `variant` (valid only on that variant).
    EnumPayload { enum_id: u32, variant: u32, slot: u32, operand: Operand },
    /// Open a new arena; the value is its handle.
    ArenaBegin,
    /// Open a `task_group`; the value is its handle (a `*TaskGroup`).
    TgBegin,
    /// Register a deferred task (`spawn`): snapshot the closure's captures into a fresh env in the
    /// task-group region, allocate the result slot there, and register the task. Yields the slot
    /// pointer (the `Task<R>` handle). `tg` = the task-group handle, `closure` = the `{fn, env}`
    /// value, `capture_tys` give the env layout (empty = non-capturing), `r` = the result scalar.
    SpawnTask { tg: Operand, closure: Operand, capture_tys: Vec<Ty>, r: Ty, fallible: bool },
    /// `wait()` as a value: join the task_group and yield its outcome. `fallible` → build
    /// `Result<(), Error>` from the runtime's first error code (`Ok(())` if `0`, else `Err(code)`);
    /// otherwise yields `()`.
    TgWaitResult { tg: Operand, fallible: bool },
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
    /// `base[index].field` for a `{ptr,len}` view of struct `struct_id` (an owned, dynamic
    /// `array<Struct>`, MMv2 slice 8d-2). Like [`IndexField`] but addressed through the loaded
    /// buffer pointer (`getelementptr %Struct, ptr, index, field`) rather than a stack slot, so a
    /// fused pipeline (`users.where(.active).score.sum()`) can run over a runtime-length AoS.
    IndexFieldPtr { base: Operand, index: Operand, field: u32, struct_id: u32 },
    /// `base[index]` — load a **whole** struct element of `struct_id` from a `{ptr,len}` view of
    /// an owned, dynamic `array<Struct>` (GEP `%Struct, ptr, index`, then load the aggregate). The
    /// field-less analogue of [`Rvalue::IndexFieldPtr`]; emitted by `map(f)` whose `f` consumes a
    /// struct element by value (a fixed stack `array<Struct>` uses [`Rvalue::Index`] instead).
    IndexPtr { base: Operand, index: Operand, struct_id: u32 },
    /// `(e0, e1, ...)` — build a tuple aggregate value of `tuple_id` from its element operands
    /// (an anonymous LLVM struct, by value). The whole-value analogue of a struct literal.
    MakeTuple { tuple_id: u32, elems: Vec<Operand> },
    /// `recv.N` — extract element `index` from a tuple value (by value).
    TupleIndex { tuple: Operand, index: u32 },
    /// Borrow array `slot` (length `n`) as a slice value `{ &slot[0], n }`.
    MakeSlice(Slot, i128),
    /// Bump-allocate `count` elements of type `elem` in the arena `handle`; yields the
    /// element pointer (used to build an owned `array<T>` via [`Rvalue::MakeDynArray`]).
    ArenaAlloc { handle: Operand, count: Operand, elem: Ty },
    /// Heap-allocate `count` elements of type `elem` (free-standing owned array, outside any
    /// arena). Yields the element pointer; freed by a later [`Stmt::Drop`].
    HeapAllocBuf { count: Operand, elem: Ty },
    /// Build an owned `array<T>` value `{ ptr, len }` from a buffer pointer and a length.
    MakeDynArray { ptr: Operand, len: Operand },
    /// `chunks(n)`: split the `{ptr,len}` slice `src` (element size `elem`) into length-`n`
    /// sub-slices, yielding an owned `array<slice<T>>` value `{ chunk_buf, count }` (via the
    /// runtime `align_rt_chunks`). The element slices borrow `src`.
    Chunks { src: Operand, n: Operand, elem: Ty },
    /// `par_map(f)` over a `{ptr,len}` source `src` with no prior stages — apply the Pure `func`
    /// to each element in parallel (runtime `align_rt_par_map` + a per-`func` thunk), materializing
    /// an owned `array<elem_out>` `{ out_buf, count }`. `elem_in` is the source element type (the
    /// `func` parameter — a scalar, or a `slice<T>` chunk); `elem_out` is `func`'s return.
    ParMapParallel { src: Operand, func: String, elem_in: Ty, elem_out: Ty },
    /// The `len` of a slice operand.
    SliceLen(Operand),
    /// The buffer `ptr` (field 0) of a slice / owned-array `{ptr,len}` operand — the raw element
    /// pointer, used to store back into the buffer (e.g. an in-place `sort`).
    SlicePtr(Operand),
    /// `slice[index]` — load a slice element (scalar).
    SliceIndex(Operand, Operand),
    /// A string literal — a `str` view `{ &bytes, len }` over a constant.
    StrLit(String),
    /// `str.clone()` — deep-copy a `str` operand's bytes into a fresh heap buffer, yielding an
    /// owned `string` `{ptr,len}`. The buffer is freed by a later [`Stmt::Drop`] of its slot.
    StrClone(Operand),
    /// `builder()` — open a builder, yielding an opaque handle (MMv2 slice 7c).
    BuilderNew,
    /// `b.write(s)` — append a `str` operand's bytes to the builder. Side-effecting; yields unit.
    BuilderWriteStr(Operand, Operand),
    /// `b.write_int(n)` — append a decimal integer (widened to `i64`) to the builder. Yields unit.
    BuilderWriteInt(Operand, Operand),
    /// `b.write_bool(v)` — append `true`/`false`. Yields unit.
    BuilderWriteBool(Operand, Operand),
    /// `b.write_char(c)` — append a `char`'s UTF-8. Yields unit.
    BuilderWriteChar(Operand, Operand),
    /// `b.write_float(x)` — append an `f32`/`f64` (codegen picks the width). Yields unit.
    BuilderWriteFloat(Operand, Operand),
    /// `b.to_string()` — finish the builder into an owned `string` `{ptr,len}` (a fresh heap
    /// buffer freed by a later [`Stmt::Drop`]), consuming the builder handle.
    BuilderToString(Operand),
    /// `template "..."` / `str + str` — build a `str` from pieces. The optional operand
    /// is the enclosing arena handle (the result lives there; `None` = leaked).
    Template(Vec<TemplatePiece>, Option<Operand>),
    /// `json.decode` into struct `struct_id`: parse the `str` `input` and fill the `out`
    /// struct slot. Yields an `i32` status (0 = ok). codegen builds the field table (names,
    /// type tags, byte offsets) and calls the runtime parser.
    JsonDecode { struct_id: u32, input: Operand, out: Slot },
    /// `json.decode` into an owned `array<elem>` (MMv2 slice 8c): parse a JSON array of scalars
    /// and write the materialized `{ptr,len}` into the `out` slot. Yields an `i32` status
    /// (0 = ok). `elem` is the element scalar (its kind/width gives the runtime element tag).
    JsonDecodeArray { elem: Ty, input: Operand, out: Slot },
    /// `json.decode` into an owned `array<Struct>` (MMv2 slice 8d): parse a JSON array of objects
    /// into a freshly heap-allocated AoS and write the materialized `{ptr,len}` (len = element
    /// count) into the `out` slot. Yields an `i32` status (0 = ok). codegen builds the same field
    /// table as [`JsonDecode`] plus the element stride, and calls the runtime parser.
    JsonDecodeStructArray { struct_id: u32, input: Operand, out: Slot },
    /// `fs.read_file(path)`: read the file named by the `str` `path` into a freshly heap-allocated
    /// owned `string`, writing its `{ptr,len}` into the `out` slot. Yields an `i32` status
    /// (0 = ok). The first `std.fs` surface.
    FsReadFile { path: Operand, out: Slot },
    /// `io.stdout.write(arg)`: write the bytes of the `str` `arg` to stdout (no newline). Yields
    /// an `i32` status (0 = ok). The first `std.io` surface.
    IoStdoutWrite { arg: Operand },
    /// `io.stdout.write(b)` for a `builder` `b`: write the builder's bytes to stdout (no newline),
    /// borrowing it. Yields an `i32` status (0 = ok).
    IoStdoutWriteBuilder { builder: Operand },
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
        fns: program.fns.iter().map(|f| lower_fn(f, &program.tuples)).collect(),
        structs: program.structs.clone(),
        enums: program.enums.clone(),
        tuples: program.tuples.clone(),
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
    /// Handles of the `task_group`s currently open (innermost last); `spawn`/`wait` use the top.
    task_groups: Vec<ValueId>,
    /// Free-standing owned locals (heap `array<T>`) that must be freed at every function
    /// exit (MMv2 slice 4; `hir::Fn::drop_locals`). Their slots are null-initialised at
    /// entry, so a drop on a path that never allocated frees null (a no-op).
    drop_locals: Vec<Slot>,
    /// Tuple defs — to tell whether a `Ty::Tuple` slot is a Move tuple (holds an owned element),
    /// which `null_moved_source` must null on move so its exit `Drop` doesn't double-free.
    tuples: Vec<hir::TupleDef>,
}

impl Builder {
    /// Free every open arena (innermost first), join + free every open `task_group`, and drop
    /// every owned free-standing local — emitted before any exit that leaves these scopes.
    fn emit_exit_cleanup(&mut self) {
        for s in self.drop_locals.clone() {
            self.push(Stmt::Drop(s));
        }
        // An early exit out of a `task_group` still joins its tasks (structured concurrency) and
        // frees the region.
        let tgs = self.task_groups.clone();
        for h in tgs.into_iter().rev() {
            self.push(Stmt::TgWait(Operand::Value(h)));
            self.push(Stmt::TgEnd(Operand::Value(h)));
        }
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

fn lower_fn(f: &hir::Fn, tuples: &[hir::TupleDef]) -> Function {
    let mut b = Builder {
        slots: f.locals.iter().map(|l| l.ty).collect(),
        value_tys: Vec::new(),
        blocks: Vec::new(),
        cur: 0,
        ret: f.ret,
        arenas: Vec::new(),
        task_groups: Vec::new(),
        drop_locals: f.drop_locals.clone(),
        tuples: tuples.to_vec(),
    };
    let entry = b.new_block();
    b.cur = entry;

    // Slot index == HIR LocalId (locals are created in id order).
    let params: Vec<Slot> = f.params.clone();
    for (i, &slot) in params.iter().enumerate() {
        b.push(Stmt::Store(slot, Operand::Arg(i as u32)));
    }
    // Null-initialise each owned-drop slot so a drop on a path that never allocated frees
    // null (a no-op) instead of an uninitialised pointer. Parameters are excluded: they arrive
    // already initialised (owning a valid buffer), so zeroing them would clobber the argument
    // and leak the caller-transferred buffer.
    for s in b.drop_locals.clone() {
        if !params.contains(&s) {
            b.push(Stmt::DropFlagInit(s));
        }
    }

    let tail = lower_block(&mut b, &f.body);
    if !b.is_terminated() {
        // Fall-through end of the body: if the trailing value moves an owned local out (the
        // function returns it), null that local's slot so the exit cleanup frees null — the
        // caller now owns the buffer — then drop the remaining owned locals.
        if f.ret != Ty::Unit {
            if let Some(v) = &f.body.value {
                null_moved_source(&mut b, v);
            }
        }
        let tail = tail.filter(|_| f.ret != Ty::Unit);
        b.emit_exit_cleanup();
        match tail {
            Some(op) => b.terminate(Term::Return(Some(op))),
            None => b.terminate(Term::Return(None)),
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

/// Null the slot of an owned `array<T>` local moved out at a (just-lowered) consuming site,
/// so its exit [`Stmt::Drop`] becomes a no-op `free(null)` and the buffer is freed once — by
/// the new owner. The moved expression is a bare `Local` (null its slot) or a block/arena whose
/// trailing value is the move (recurse into the tail). Other shapes (fresh temporaries like
/// `make()` / `.to_array()`) own no slot, and sema rejects moving a bound owned local out
/// through an `if`/`else` arm, so no other case reaches here. Restricted to free-standing owned
/// slots (`DynArray`, owned `string`) — `box<T>` is arena-regioned and never free-standing-dropped.
fn null_moved_source(b: &mut Builder, e: &hir::Expr) {
    match &e.kind {
        hir::ExprKind::Local(id) => {
            let moved = match b.slots.get(*id as usize) {
                Some(&ty) => {
                    matches!(ty, Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::String | Ty::Builder)
                        || payload_is_move(ty)
                        // A Move tuple (holds an owned element) moved away must be nulled so its
                        // exit `Drop` frees nulls, not the buffers the new owner took.
                        || matches!(ty, Ty::Tuple(tid) if b.tuples[tid as usize].elems.iter().any(|s| s.is_move()))
                }
                None => false,
            };
            if moved {
                b.push(Stmt::DropFlagInit(*id));
            }
        }
        hir::ExprKind::Block(blk) | hir::ExprKind::Arena(blk) => {
            if let Some(v) = &blk.value {
                null_moved_source(b, v);
            }
        }
        // `t.get()` moves an owned result out of the task; null the task slot so its exit `Drop`
        // doesn't double-free the buffer the gotten value now owns.
        hir::ExprKind::TaskGet(inner) => null_moved_source(b, inner),
        // A bound owned local moved into a wrapper (`return Ok(xs)` / `Some(xs)` / `Err(xs)`) is
        // consumed by the construction — see through the wrapper to null the source slot, else the
        // local's exit `Drop` double-frees the buffer now owned by the aggregate.
        hir::ExprKind::ResultOk(inner) | hir::ExprKind::ResultErr(inner) | hir::ExprKind::OptionSome(inner) => {
            null_moved_source(b, inner);
        }
        // A tuple literal moves each owned-local element into the tuple (its consumer — a
        // destructure target, or the returned tuple's caller — now owns the buffer), so null those
        // source slots, else both the source local and the new owner would free the same buffer.
        hir::ExprKind::Tuple { elems, .. } => {
            for el in elems {
                null_moved_source(b, el);
            }
        }
        // A partial field move (`a := t.0`) took the owned element's buffer; null that one field of
        // the tuple slot so the tuple's exit `Drop` frees null there, not the now-aliased buffer.
        hir::ExprKind::TupleIndex { recv, index } => {
            if let hir::ExprKind::Local(t) = &recv.kind {
                let owned = matches!(b.slots.get(*t as usize), Some(&Ty::Tuple(tid))
                    if b.tuples[tid as usize].elems.get(*index as usize).is_some_and(|s| s.is_move()));
                if owned {
                    b.push(Stmt::NullTupleField(*t, *index));
                }
            }
        }
        _ => {}
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
                // If the initializer moved an owned local, null its slot (drop-flag).
                null_moved_source(b, init);
            }
        },
        hir::Stmt::Assign { local, value } => {
            let op = lower_expr(b, value);
            b.push(Stmt::Store(*local, op));
            null_moved_source(b, value);
        }
        hir::Stmt::AssignField { base, index, value } => {
            let op = lower_expr(b, value);
            b.push(Stmt::StoreField(*base, *index, op));
        }
        hir::Stmt::AssignIndex { base, index, value } => {
            // `base[index] = value` — bounds-checked element store (abort on out-of-range, like a
            // read). A `{ptr,len}` slice/owned-array writes through its buffer pointer; a fixed
            // stack array writes its slot directly.
            let idx = lower_expr(b, index);
            let val = lower_expr(b, value);
            let base_ty = b.slots[*base as usize];
            match base_ty {
                Ty::Slice(s) | Ty::DynArray(s) => {
                    let sv = b.fresh_value(base_ty);
                    b.push(Stmt::Let(sv, Rvalue::Load(*base)));
                    let len = b.fresh_value(i64_ty());
                    b.push(Stmt::Let(len, Rvalue::SliceLen(Operand::Value(sv))));
                    emit_bounds_check(b, &idx, Operand::Value(len));
                    let ptr = b.fresh_value(Ty::Box(s));
                    b.push(Stmt::Let(ptr, Rvalue::SlicePtr(Operand::Value(sv))));
                    b.push(Stmt::PtrStore(Operand::Value(ptr), idx, val));
                }
                Ty::Array(_, n) => {
                    emit_bounds_check(b, &idx, Operand::Const(Const::Int(n as i128, i64_ty())));
                    b.push(Stmt::StoreIndex(*base, idx, val));
                }
                other => unreachable!("element assignment into non-array/slice {other:?}"),
            }
        }
        hir::Stmt::Return(value) => {
            let op = value.as_ref().map(|e| lower_expr(b, e));
            // A returned owned array is moved out: null its slot so the exit cleanup below frees
            // null (the caller now owns the buffer), then free open arenas / drop owned locals.
            if let Some(e) = value {
                null_moved_source(b, e);
            }
            b.emit_exit_cleanup();
            b.terminate(Term::Return(op));
            // The current block is now terminated; `lower_block` stops here, so no dead
            // block is created and callers can see the divergence via `is_terminated`.
        }
        hir::Stmt::LetTuple { locals, init, .. } => {
            // Evaluate the tuple once, then extract each bound element into its slot (`_` skipped).
            let tup = lower_expr(b, init);
            // If the tuple was built from owned source locals (`(x, y) := (a, b)`), null them: the
            // destructure targets now own the buffers, so the source slots must not also free them.
            null_moved_source(b, init);
            for (i, lid) in locals.iter().enumerate() {
                if let Some(lid) = lid {
                    let ety = b.slots[*lid as usize];
                    let v = b.fresh_value(ety);
                    b.push(Stmt::Let(v, Rvalue::TupleIndex { tuple: tup.clone(), index: i as u32 }));
                    b.push(Stmt::Store(*lid, Operand::Value(v)));
                }
            }
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
        hir::ExprKind::JsonDecodeArray { elem, input } => lower_json_decode_array(b, *elem, input, e.ty),
        hir::ExprKind::JsonDecodeStructArray { struct_id, input } => lower_json_decode_struct_array(b, *struct_id, input, e.ty),
        hir::ExprKind::FsReadFile { path } => lower_fs_read_file(b, path, e.ty),
        hir::ExprKind::IoStdoutWrite { arg } => {
            lower_io_stdout_write(b, arg, e.ty, |a| Rvalue::IoStdoutWrite { arg: a })
        }
        hir::ExprKind::IoStdoutWriteBuilder { builder } => {
            lower_io_stdout_write(b, builder, e.ty, |a| Rvalue::IoStdoutWriteBuilder { builder: a })
        }
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
        hir::ExprKind::IntArith { op, mode, lhs, rhs } => {
            let int_ty = lhs.ty;
            let a = lower_expr(b, lhs);
            let bb = lower_expr(b, rhs);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::IntArith { op: *op, mode: *mode, int_ty, a, b: bb }));
            Operand::Value(v)
        }
        hir::ExprKind::MathOp { fn_, operands } => {
            let ty = operands[0].ty;
            let ops: Vec<Operand> = operands.iter().map(|o| lower_expr(b, o)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MathOp { fn_: *fn_, ty, operands: ops }));
            Operand::Value(v)
        }
        hir::ExprKind::FnValue(name) => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::FnAddr(name.clone())));
            Operand::Value(v)
        }
        hir::ExprKind::Closure { lifted, captures } => {
            let capture_tys: Vec<Ty> = captures.iter().map(|c| c.ty).collect();
            let ops: Vec<Operand> = captures.iter().map(|c| lower_expr(b, c)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Closure { lifted: lifted.clone(), captures: ops, capture_tys }));
            Operand::Value(v)
        }
        hir::ExprKind::CallFnValue { callee, args } => {
            let c = lower_expr(b, callee);
            // The function type for the indirect call comes from the (sema-checked) arg types and
            // the call's result type — no signature table is threaded into MIR.
            let (param_tys, ops): (Vec<Ty>, Vec<Operand>) =
                args.iter().map(|a| (a.ty, lower_expr(b, a))).unzip();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::CallIndirect { callee: c, args: ops, param_tys, ret_ty: e.ty }));
            Operand::Value(v)
        }
        hir::ExprKind::Call { func, args } => {
            let ops = args.iter().map(|a| lower_expr(b, a)).collect();
            // A by-value owned-array argument is moved into the callee: null the caller's slot.
            // `print` only reads its argument (it borrows), so it must not null the source — it
            // keeps living (matching the borrow in sema's MoveCheck).
            if func != "print" {
                for a in args {
                    null_moved_source(b, a);
                }
            }
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Call(func.clone(), ops)));
            Operand::Value(v)
        }
        hir::ExprKind::If { cond, then, els } => lower_if(b, cond, then, els, e.ty),
        // `Type.Variant(payload…)` — build the sum-type aggregate `{ i32 tag, … }`.
        hir::ExprKind::EnumValue { enum_id, variant, payload } => {
            let ops: Vec<Operand> = payload.iter().map(|p| lower_expr(b, p)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeEnum { enum_id: *enum_id, variant: *variant, payload: ops }));
            Operand::Value(v)
        }
        hir::ExprKind::Match { scrutinee, arms } => lower_match(b, scrutinee, arms, e.ty),
        hir::ExprKind::ResultMapErr { result, f } => lower_map_err(b, result, f, e.ty),
        hir::ExprKind::Field { base, index } => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Field(*base, *index)));
            Operand::Value(v)
        }
        hir::ExprKind::Tuple { tuple_id, elems } => {
            let ops: Vec<Operand> = elems.iter().map(|el| lower_expr(b, el)).collect();
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::MakeTuple { tuple_id: *tuple_id, elems: ops }));
            Operand::Value(v)
        }
        hir::ExprKind::TupleIndex { recv, index } => {
            let t = lower_expr(b, recv);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::TupleIndex { tuple: t, index: *index }));
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
        // ④b: `task_group` opens a region owning each task's env + result slot, plus a deferred
        // task list. `spawn`/`wait` use the handle; the region is freed at scope end.
        hir::ExprKind::TaskGroup(blk) => {
            let handle = b.fresh_value(Ty::ArenaHandle);
            b.push(Stmt::Let(handle, Rvalue::TgBegin));
            b.task_groups.push(handle);
            let tail = lower_block(b, blk);
            b.task_groups.pop();
            if b.is_terminated() {
                Operand::Const(Const::Unit)
            } else {
                b.push(Stmt::TgEnd(Operand::Value(handle)));
                tail.unwrap_or(Operand::Const(Const::Unit))
            }
        }
        // ④b-1b (deferred): `spawn(closure)` snapshots the closure's captures into a fresh env in
        // the task-group region and registers the task; it runs at `wait`. The `Task<R>` handle is
        // the task's result slot. The closure's captures give the env layout.
        hir::ExprKind::Spawn { closure, fallible } => {
            let Ty::Task(s) = e.ty else { unreachable!("spawn result is a Task") };
            let r_ty = align_sema::scalar_to_ty(s);
            let capture_tys: Vec<Ty> = match &closure.kind {
                hir::ExprKind::Closure { captures, .. } => captures.iter().map(|c| c.ty).collect(),
                _ => Vec::new(),
            };
            let clos = lower_expr(b, closure);
            let tg = Operand::Value(*b.task_groups.last().expect("spawn outside a task_group"));
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::SpawnTask { tg, closure: clos, capture_tys, r: r_ty, fallible: *fallible }));
            Operand::Value(v)
        }
        // `t.get()` reads the result out of the task's slot.
        hir::ExprKind::TaskGet(inner) => {
            let bx = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BoxGet(bx)));
            Operand::Value(v)
        }
        // `wait()` — run all deferred tasks of the enclosing task_group.
        hir::ExprKind::Wait => {
            let tg = Operand::Value(*b.task_groups.last().expect("wait outside a task_group"));
            // A fallible group's `wait()` yields `Result<(), Error>` (built from the runtime's
            // error code); an infallible group's yields `()`.
            let fallible = matches!(e.ty, Ty::Result(..));
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::TgWaitResult { tg, fallible }));
            Operand::Value(v)
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
        hir::ExprKind::StrClone(inner) => {
            // Deep-copy the `str` bytes into a fresh heap buffer, yielding an owned `string`
            // `{ptr,len}`. The slot it lands in is `Drop`-freed at scope exit (sema marks the
            // String local for drop), so no arena is needed.
            let src = lower_expr(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::StrClone(src)));
            Operand::Value(v)
        }
        // Borrowing an owned `string` as a `str` (slice 7b) is a no-op at runtime: the two share
        // the `{ptr,len}` layout, so the loaded value is the view. The `string` is not moved (no
        // `null_moved_source`), so its owner still `Drop`-frees it.
        hir::ExprKind::StrBorrow(inner) => lower_expr(b, inner),
        hir::ExprKind::BuilderNew => {
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BuilderNew));
            Operand::Value(v)
        }
        hir::ExprKind::BuilderWrite { builder, arg, kind } => {
            let bop = lower_expr(b, builder);
            let aop = lower_expr(b, arg);
            let v = b.fresh_value(Ty::Unit);
            let rv = match kind {
                hir::BuilderWriteKind::Str => Rvalue::BuilderWriteStr(bop, aop),
                hir::BuilderWriteKind::Int => Rvalue::BuilderWriteInt(bop, aop),
                hir::BuilderWriteKind::Bool => Rvalue::BuilderWriteBool(bop, aop),
                hir::BuilderWriteKind::Char => Rvalue::BuilderWriteChar(bop, aop),
                hir::BuilderWriteKind::Float => Rvalue::BuilderWriteFloat(bop, aop),
            };
            b.push(Stmt::Let(v, rv));
            Operand::Const(Const::Unit)
        }
        hir::ExprKind::BuilderToString(inner) => {
            let bop = lower_expr(b, inner);
            // The builder is consumed: null its slot so the exit `Drop` of an unfinished builder
            // is a no-op (`builder_free(null)`), and the finished `string` owns its own buffer.
            null_moved_source(b, inner);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::BuilderToString(bop)));
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
        hir::ExprKind::ArrayReduce { source, stages, func, captures, init } => {
            let init_op = lower_expr(b, init);
            lower_array_reduce(b, source, stages, e.ty, init_op, Reducer::Fold { func: func.clone(), captures: captures.clone() })
        }
        hir::ExprKind::ArrayAnyAll { source, stages, func, captures, all } => {
            // bool accumulator: `all` seeds true (&&-fold), `any` seeds false (||-fold).
            let init = Operand::Const(Const::Bool(*all));
            lower_array_reduce(b, source, stages, Ty::Bool, init, Reducer::AnyAll { func: func.clone(), captures: captures.clone(), all: *all })
        }
        hir::ExprKind::ArrayMinMax { source, stages, is_max } => {
            // Seed with the element type's extreme so the running `min`/`max` is replaced by the
            // first element and an empty pipeline yields that extreme (the fold identity).
            let init = extreme_of(e.ty, *is_max);
            lower_array_reduce(b, source, stages, e.ty, init, Reducer::MinMax { is_max: *is_max })
        }
        hir::ExprKind::ArrayToArray { source, stages, elem } => {
            lower_array_collect(b, source, stages, *elem, CollectKind::Collect)
        }
        hir::ExprKind::ArrayScan { source, stages, func, captures, init, elem } => {
            let init_op = lower_expr(b, init);
            lower_array_collect(b, source, stages, *elem, CollectKind::Scan { func: func.clone(), init: init_op, captures: captures.clone() })
        }
        hir::ExprKind::ArrayDot { a, b: bex, elem } => lower_array_dot(b, a, bex, *elem),
        hir::ExprKind::ArraySort { source, stages, elem } => lower_array_sort(b, source, stages, *elem, None),
        hir::ExprKind::ArraySortBy { source, stages, key_func, captures, key_ty, elem } => {
            lower_array_sort(b, source, stages, *elem, Some(SortKey { func: key_func.clone(), captures: captures.clone(), key_ty: *key_ty }))
        }
        hir::ExprKind::ArrayPartition { source, stages, func, captures, elem } => {
            let tuple_id = match e.ty {
                Ty::Tuple(id) => id,
                _ => unreachable!("partition result is a tuple"),
            };
            lower_array_partition(b, source, stages, *elem, func, captures, tuple_id)
        }
        hir::ExprKind::ArrayParMap { source, stages, func, captures, elem } => {
            // With no prior stages, a `{ptr,len}` (or fixed scalar-array) source, and no captures,
            // run in parallel via the runtime; otherwise (prior stages, struct-array source, or a
            // capturing lambda — the parallel thunk takes no capture context) fall back to the
            // sequential collect loop.
            let elem_in = match source.ty {
                Ty::Slice(s) | Ty::DynArray(s) | Ty::Array(s, _) => Some(align_sema::scalar_to_ty(s)),
                Ty::DynSliceArray(p) => Some(Ty::Slice(align_sema::prim_to_scalar(p))),
                _ => None,
            };
            if stages.is_empty() && captures.is_empty() {
                if let Some(elem_in) = elem_in {
                    let src = match source.ty {
                        Ty::Slice(_) | Ty::DynArray(_) | Ty::DynSliceArray(_) => lower_expr(b, source),
                        _ => {
                            let (slot, n) = array_source_slot(b, source);
                            let sv = b.fresh_value(Ty::Slice(scalar_of(elem_in)));
                            b.push(Stmt::Let(sv, Rvalue::MakeSlice(slot, n)));
                            Operand::Value(sv)
                        }
                    };
                    // Free the source buffer if it is an owned temporary the runtime just consumed
                    // (same rule as `setup_source`: `chunks`/call results are always heap; the
                    // materializing terminals arena-allocate inside an arena and are bulk-freed).
                    let free_src = matches!(source.kind, hir::ExprKind::ArrayChunks { .. } | hir::ExprKind::Call { .. })
                        || (matches!(
                            source.kind,
                            hir::ExprKind::ArrayToArray { .. } | hir::ExprKind::ArrayScan { .. }
                                | hir::ExprKind::ArrayParMap { .. } | hir::ExprKind::ArraySort { .. } | hir::ExprKind::ArraySortBy { .. }
                        ) && b.arenas.is_empty());
                    let v = b.fresh_value(e.ty);
                    b.push(Stmt::Let(v, Rvalue::ParMapParallel { src: src.clone(), func: func.clone(), elem_in, elem_out: *elem }));
                    if free_src {
                        b.push(Stmt::DropValue(src));
                    }
                    return Operand::Value(v);
                }
            }
            // Sequential fallback: append a `map(f)` stage (carrying any captures) and materialize
            // via the collect loop.
            let mut stages2 = stages.clone();
            stages2.push(hir::Stage { kind: hir::StageKind::Map { func: func.clone(), captures: captures.clone() }, out_ty: *elem });
            lower_array_collect(b, source, &stages2, *elem, CollectKind::Collect)
        }
        hir::ExprKind::ArrayChunks { source, n, elem } => {
            // Materialize the source as a `{ptr,len}` slice, then call the runtime chunker.
            let src = match source.ty {
                Ty::Slice(_) | Ty::DynArray(_) => lower_expr(b, source),
                _ => {
                    let (slot, len) = array_source_slot(b, source);
                    let sv = b.fresh_value(Ty::Slice(scalar_of(*elem)));
                    b.push(Stmt::Let(sv, Rvalue::MakeSlice(slot, len)));
                    Operand::Value(sv)
                }
            };
            let n_op = lower_expr(b, n);
            let v = b.fresh_value(e.ty);
            b.push(Stmt::Let(v, Rvalue::Chunks { src, n: n_op, elem: *elem }));
            Operand::Value(v)
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
        hir::ExprKind::Index { recv, index } => lower_index(b, recv, index, e.ty),
        hir::ExprKind::ElemField { recv, index, field, struct_id } => {
            lower_index_field(b, recv, index, *field, *struct_id, e.ty)
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

/// The i32 status code a runtime builtin (`fs`/`json`/`io`) returns, before it is wrapped into
/// `Error.Code`.
fn status_ty() -> Ty {
    Ty::Int(IntTy { bits: 32, signed: true })
}

/// Emit the explicit bounds check for `recv[index]` (semantics live in MIR):
/// `if index < 0 || index >= len { bounds_fail(index, len); unreachable }`. Leaves `b.cur` at the
/// in-bounds block so the caller emits the element load. Out-of-bounds is a hard error (the
/// settled panic model — never a silent OOB read).
fn emit_bounds_check(b: &mut Builder, idx: &Operand, len: Operand) {
    let lo = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(lo, Rvalue::Bin(BinOp::Lt, idx.clone(), Operand::Const(Const::Int(0, i64_ty())))));
    let hi = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(hi, Rvalue::Bin(BinOp::Ge, idx.clone(), len.clone())));
    let oob = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(oob, Rvalue::Bin(BinOp::Or, Operand::Value(lo), Operand::Value(hi))));

    let fail = b.new_block();
    let ok = b.new_block();
    b.terminate(Term::Branch(Operand::Value(oob), fail, ok));

    // fail: report (index, len) and abort. `bounds_fail` is `-> !`, so the block is `Unreachable`.
    b.cur = fail;
    let t = b.fresh_value(Ty::Unit);
    b.push(Stmt::Let(t, Rvalue::Call("bounds_fail".to_string(), vec![idx.clone(), len])));
    b.terminate(Term::Unreachable);

    b.cur = ok;
}

/// `recv[index]` → a bounds-checked scalar element load. A scalar `array<T>` / `slice` loads
/// through its `{ptr,len}` value (`SliceIndex`); a fixed stack `array` loads through its slot
/// (`Index`).
fn lower_index(b: &mut Builder, recv: &hir::Expr, index: &hir::Expr, elem_ty: Ty) -> Operand {
    let idx = lower_expr(b, index);
    // The length, and whether the element loads from a `{ptr,len}` value or a stack slot.
    enum Src {
        Slice(Operand),
        Slot(Slot),
    }
    let (src, len): (Src, Operand) = match recv.ty {
        // A `{ptr,len}` value: scalar `slice`/owned `array` loads a scalar element; an
        // `array<slice<T>>` (`chunks` result) loads a whole `slice<T>` element; an owned dynamic
        // `array<Struct>` loads a whole struct element (all by `elem_ty` via `SliceIndex`).
        Ty::Slice(_) | Ty::DynArray(_) | Ty::DynSliceArray(_) | Ty::DynStructArray(..) => {
            let sv = lower_expr(b, recv);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            (Src::Slice(sv), Operand::Value(len))
        }
        _ => {
            // A fixed `array<T>` (sema restricted `recv` to a literal / local).
            let (slot, n) = array_source_slot(b, recv);
            (Src::Slot(slot), Operand::Const(Const::Int(n, i64_ty())))
        }
    };
    emit_bounds_check(b, &idx, len);
    let v = b.fresh_value(elem_ty);
    match src {
        Src::Slice(sv) => b.push(Stmt::Let(v, Rvalue::SliceIndex(sv, idx))),
        Src::Slot(slot) => b.push(Stmt::Let(v, Rvalue::Index(slot, idx))),
    }
    Operand::Value(v)
}

/// `recv[index].field` for a struct array (MMv2 slice 8f) → a bounds-checked element-field load.
/// A fixed stack `array<Struct>` uses the slot-based `IndexField`; an owned dynamic
/// `array<Struct>` uses the pointer-based `IndexFieldPtr` (same addressing as a fused pipeline
/// projection). Only the one field (a scalar or a `str` view) is loaded — no whole-struct copy.
fn lower_index_field(b: &mut Builder, recv: &hir::Expr, index: &hir::Expr, field: u32, struct_id: u32, field_ty: Ty) -> Operand {
    let idx = lower_expr(b, index);
    // Set the element-field address up the same way the fused pipeline does (one shared seam,
    // `lower_field_access`): a fixed `array<Struct>` is slot-addressed, an owned dynamic
    // `array<Struct>` is a `{ptr,len}` value addressed by pointer. Differs from the pipeline only
    // in needing an explicit bounds check (the loop's counter is in-bounds by construction).
    let (struct_view, slice_val, slot, len) = match recv.ty {
        Ty::DynStructArray(_, layout) => {
            let sv = lower_expr(b, recv);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            (Some((struct_id, layout)), Some(sv), 0, Operand::Value(len))
        }
        _ => {
            // A fixed `array<Struct>` slot (sema restricted `recv` to a literal / local).
            let (slot, n) = array_source_slot(b, recv);
            (None, None, slot, Operand::Const(Const::Int(n, i64_ty())))
        }
    };
    emit_bounds_check(b, &idx, len);
    let v = lower_field_access(b, struct_view, &slice_val, slot, &idx, field, field_ty);
    Operand::Value(v)
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

/// The seed for a `min` (`is_max = false`) / `max` (`is_max = true`) fold: the element type's
/// largest / smallest value, so the first element always replaces it. Floats use ±infinity.
fn extreme_of(ty: Ty, is_max: bool) -> Operand {
    match ty {
        Ty::Float(_) => {
            let v = if is_max { f64::NEG_INFINITY } else { f64::INFINITY };
            Operand::Const(Const::Float(v, ty))
        }
        Ty::Int(IntTy { bits, signed }) => {
            // `min` seeds with the type max; `max` seeds with the type min.
            let v: i128 = if is_max {
                // type minimum
                if signed { -(1i128 << (bits - 1)) } else { 0 }
            } else {
                // type maximum
                if signed { (1i128 << (bits - 1)) - 1 } else { (1i128 << bits) - 1 }
            };
            Operand::Const(Const::Int(v, ty))
        }
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
    /// `reduce(init, f)`: `f(acc, element)`. `captures` are a lifted lambda's captured values,
    /// passed after the `(acc, element)` arguments.
    Fold { func: String, captures: Vec<hir::Expr> },
    /// `any(p)` / `all(p)`: `acc || p(element)` / `acc && p(element)`. `captures` as `Fold`.
    AnyAll { func: String, captures: Vec<hir::Expr>, all: bool },
    /// `min` / `max`: keep `element` when it is smaller / larger than `acc`.
    MinMax { is_max: bool },
}

/// The set-up of a pipeline source: a stack array (slot + const length), a struct array
/// (slot), or a `{ptr,len}`-shaped value — a `slice` or an owned `array` (operand + runtime
/// length). Shared by the reducing and collecting loops.
struct SrcSetup {
    slot: Slot,
    slice_val: Option<Operand>,
    bound: Operand,
    scalar_slot: bool,
    /// `Some((struct_id, layout))` when the source is an owned, dynamic `array<Struct>` — a
    /// `{ptr,len}` view (`slice_val`) addressed by pointer + index for field projection (MMv2
    /// slice 8d-2). The loop keeps it index-addressed (no up-front element load) and projects
    /// fields via the layout seam `lower_field_access`. The layout is carried (not discarded) so
    /// it reaches that seam — adding `Layout::Soa` then forces a match there.
    struct_view: Option<(u32, Layout)>,
    /// An unbound free-standing owned-array temporary that this source materialized in place
    /// (`[..].to_array().sum()` with no arena): its `{ptr,len}` value, to be freed by the
    /// consuming loop once done. `None` for slots, slices, bound locals, and arena temporaries
    /// (the latter are bulk-freed by the arena).
    temp_free: Option<Operand>,
}

/// The arguments for a stage function call: the element, then any captured values (a lifted
/// lambda's captured enclosing locals, passed by value). Captures are lowered each iteration —
/// they reference loop-invariant enclosing locals, so LLVM hoists the loads out of the loop.
fn stage_call_args(b: &mut Builder, arg: Operand, captures: &[hir::Expr]) -> Vec<Operand> {
    let mut args = Vec::with_capacity(1 + captures.len());
    args.push(arg);
    for c in captures {
        args.push(lower_expr(b, c));
    }
    args
}

fn setup_source(b: &mut Builder, source: &hir::Expr) -> SrcSetup {
    match source.ty {
        // `slice<T>`, owned `array<T>`, and `array<slice<T>>` (a `chunks` result, element =
        // `slice<T>`) all share the `{ptr,len}` layout and runtime length.
        Ty::Slice(_) | Ty::DynArray(_) | Ty::DynSliceArray(_) => {
            let sv = lower_expr(b, source);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            // A source that *owns* a fresh free-standing buffer nothing else holds must be freed
            // by the consuming loop: a `.to_array()` / `.scan()` materialization, or a call
            // returning an owned `array<T>` (`make().sum()` — ownership transferred to us). A
            // bound `Local`
            // and a struct `Field` are borrows (freed by the owner's exit `Drop`), and arena
            // temporaries are bulk-freed, so none of those are freed here. `Block`/`If` sources
            // may *borrow* a bound local in a branch (e.g. `(if c { ys } else { zs }).sum()`), so
            // blanket-freeing them would double-free — they are left as a sound, bounded leak.
            // `chunks` (runtime `align_rt_chunks`) and a function's owned-array return are *always*
            // heap-allocated, so they must be freed even inside an `arena {}` (the arena's bulk-free
            // doesn't cover them). The materializing terminals instead arena-allocate when inside an
            // arena (bulk-freed there), so the loop frees them only outside one.
            let always_heap = matches!(
                source.kind,
                hir::ExprKind::ArrayChunks { .. } | hir::ExprKind::Call { .. }
            );
            let arena_if_in_arena = matches!(
                source.kind,
                hir::ExprKind::ArrayToArray { .. } | hir::ExprKind::ArrayScan { .. }
                    | hir::ExprKind::ArrayParMap { .. } | hir::ExprKind::ArraySort { .. } | hir::ExprKind::ArraySortBy { .. }
            );
            let temp_free =
                (always_heap || (arena_if_in_arena && b.arenas.is_empty())).then(|| sv.clone());
            SrcSetup { slot: 0, slice_val: Some(sv), bound: Operand::Value(len), scalar_slot: false, struct_view: None, temp_free }
        }
        // An owned, dynamic `array<Struct>`: a `{ptr,len}` view addressed by pointer for field
        // projection (slice 8d-2). It is a bound local borrow (sema requires a variable source),
        // so nothing is freed by the loop — the owner's exit `Drop` frees the buffer.
        Ty::DynStructArray(id, layout) => {
            let sv = lower_expr(b, source);
            let len = b.fresh_value(i64_ty());
            b.push(Stmt::Let(len, Rvalue::SliceLen(sv.clone())));
            SrcSetup { slot: 0, slice_val: Some(sv), bound: Operand::Value(len), scalar_slot: false, struct_view: Some((id, layout)), temp_free: None }
        }
        _ => {
            let (slot, n) = array_source_slot(b, source);
            SrcSetup {
                slot,
                slice_val: None,
                bound: Operand::Const(Const::Int(n, i64_ty())),
                scalar_slot: matches!(source.ty, Ty::Array(..)),
                struct_view: None,
                temp_free: None,
            }
        }
    }
}

/// The **single layout seam** for struct-array element-field addressing — the one place that
/// turns `arr[i].field` into a load, shared by the fused pipeline (8d-2) and surface indexing
/// (8f). A stack-slot (fixed) `array<Struct>` is always AoS and uses the slot-based
/// [`Rvalue::IndexField`]; an owned dynamic `array<Struct>` view (`struct_view = Some((id,
/// layout))`) carries its [`Layout`] here. The `match layout` below is the SoA hook: today only
/// `Layout::Aos` (the pointer-based [`Rvalue::IndexFieldPtr`], `element, field` GEP); when
/// `Layout::Soa` (`soa array<T>`) lands at M6, this match goes non-exhaustive — a compile error
/// that points exactly here, the one site SoA's column-array indexing must branch in.
fn lower_field_access(
    b: &mut Builder,
    struct_view: Option<(u32, Layout)>,
    slice_val: &Option<Operand>,
    slot: Slot,
    index: &Operand,
    field: u32,
    out_ty: Ty,
) -> ValueId {
    let v = b.fresh_value(out_ty);
    match struct_view {
        Some((struct_id, layout)) => match layout {
            Layout::Aos => b.push(Stmt::Let(
                v,
                Rvalue::IndexFieldPtr {
                    base: slice_val.clone().expect("a struct-view source has a {ptr,len} value"),
                    index: index.clone(),
                    field,
                    struct_id,
                },
            )),
        },
        None => b.push(Stmt::Let(v, Rvalue::IndexField(slot, index.clone(), field))),
    }
    v
}

/// Load a **whole** struct element `src[index]` for a `map(f)` whose `f` consumes the struct by
/// value (the whole-element companion of [`lower_field_access`]). A fixed stack `array<Struct>`
/// (`struct_view == None`) loads the aggregate straight from its slot ([`Rvalue::Index`]); an
/// owned dynamic `array<Struct>` view loads through the buffer pointer ([`Rvalue::IndexPtr`]). The
/// `match layout` mirrors the field seam: `Layout::Soa` (M6) makes it non-exhaustive here too.
fn lower_struct_elem(
    b: &mut Builder,
    struct_view: Option<(u32, Layout)>,
    slice_val: &Option<Operand>,
    slot: Slot,
    index: &Operand,
    struct_id: u32,
) -> ValueId {
    let v = b.fresh_value(Ty::Struct(struct_id));
    match struct_view {
        Some((sid, layout)) => match layout {
            Layout::Aos => b.push(Stmt::Let(
                v,
                Rvalue::IndexPtr {
                    base: slice_val.clone().expect("a struct-view source has a {ptr,len} value"),
                    index: index.clone(),
                    struct_id: sid,
                },
            )),
        },
        None => b.push(Stmt::Let(v, Rvalue::Index(slot, index.clone()))),
    }
    v
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
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src, struct_view, temp_free } = setup_source(b, source);

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

    // A scalar array or a slice loads the element up front; a struct array (stack slot or a
    // `{ptr,len}` `array<Struct>` view) stays addressed by index until a `.field` projection.
    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
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
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                // A scalar element is already loaded; a struct element consumed whole (a
                // `map(f)` with no prior `.field`) is loaded here by index.
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func, captures } => {
                // A scalar element is already loaded; a whole struct element (a struct-consuming
                // predicate, no prior projection) is loaded here by index. `where` keeps the
                // element, so `cur` is left unchanged either way.
                let arg = match &cur {
                    Some(a) => a.clone(),
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("where with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), call_args)));
                let keep = b.new_block();
                // false → skip this element (go to the increment).
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                // Predicate on a struct element's (bool) field; the element is unchanged.
                let pred = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, Ty::Bool);
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }
    let a = b.fresh_value(acc_ty);
    b.push(Stmt::Let(a, Rvalue::Load(acc)));
    // `min`/`max` update the accumulator conditionally (keep the element only when it beats the
    // current best), branching straight to `cont`; the other reducers compute a `next` value
    // unconditionally and fall through to the shared store-and-loop below.
    if let Reducer::MinMax { is_max } = &reducer {
        let cur = cur.expect("min/max needs a scalar element");
        let op = if *is_max { BinOp::Gt } else { BinOp::Lt };
        let cmp = b.fresh_value(Ty::Bool);
        b.push(Stmt::Let(cmp, Rvalue::Bin(op, cur.clone(), Operand::Value(a))));
        let upd = b.new_block();
        b.terminate(Term::Branch(Operand::Value(cmp), upd, cont));
        b.cur = upd;
        b.push(Stmt::Store(acc, cur));
        b.terminate(Term::Goto(cont));
    } else {
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
            Reducer::Fold { func, captures } => {
                let cur = cur.expect("reduce needs a scalar element");
                let mut args = vec![Operand::Value(a), cur];
                for c in captures {
                    args.push(lower_expr(b, c));
                }
                b.push(Stmt::Let(next, Rvalue::Call(func.clone(), args)));
            }
            // `any`/`all`: t = p(cur); acc = acc || t  /  acc && t.
            Reducer::AnyAll { func, captures, all } => {
                let cur = cur.expect("any/all needs a scalar element");
                let t = b.fresh_value(Ty::Bool);
                let args = stage_call_args(b, cur, captures);
                b.push(Stmt::Let(t, Rvalue::Call(func.clone(), args)));
                let op = if *all { BinOp::And } else { BinOp::Or };
                b.push(Stmt::Let(next, Rvalue::Bin(op, Operand::Value(a), Operand::Value(t))));
            }
            Reducer::MinMax { .. } => unreachable!("min/max handled above"),
        }
        b.push(Stmt::Store(acc, Operand::Value(next)));
        b.terminate(Term::Goto(cont));
    }

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
    // Free a free-standing `.to_array()` temporary now that the fold has consumed it. The
    // result `r` is a scalar accumulator independent of the buffer, so this is safe.
    if let Some(tmp) = temp_free {
        b.push(Stmt::DropValue(tmp));
    }
    Operand::Value(r)
}

/// What a materializing collect loop appends per surviving element.
enum CollectKind {
    /// `to_array`: append the element itself.
    Collect,
    /// `scan(init, f)`: thread an accumulator (`acc = f(acc, element)`, seeded with `init`) and
    /// append the running accumulator. `captures` are a lifted lambda's captured values, passed
    /// after the `(acc, element)` arguments.
    Scan { func: String, init: Operand, captures: Vec<hir::Expr> },
}

/// `source.….to_array()` / `.scan(init, f)` — the fused loop, but each surviving element is
/// appended to a freshly allocated buffer (arena-bump inside an arena, else heap) instead of
/// folded into a scalar. Yields an owned `array<T>` value `{ ptr, len }` where `len` is the
/// survivor count. (MMv2 slice 3 `to_array`; slice 5 adds `scan`.)
fn lower_array_collect(b: &mut Builder, source: &hir::Expr, stages: &[hir::Stage], elem: Ty, kind: CollectKind) -> Operand {
    // Inside an arena → bump-allocate (bulk-freed); otherwise → free-standing heap (dropped).
    let arena = b.arenas.last().copied();
    // A collect source can itself be a fresh unbound owned temporary (`make().map(f).to_array()`
    // — `make()` returns an owned array nothing else holds). The copy loop consumes it into the
    // new output buffer, so free that source temporary at the exit (the result is a separate
    // buffer). `temp_free` is None for slots / bound locals / arena temporaries.
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src, struct_view, temp_free } = setup_source(b, source);

    // Output buffer: `bound` (upper-bound = source length) elements. map/where never grow
    // the count, so the buffer never needs to be resized.
    let out_ptr = b.fresh_value(Ty::Box(scalar_of(elem)));
    let alloc = match arena {
        Some(h) => Rvalue::ArenaAlloc { handle: Operand::Value(h), count: bound.clone(), elem },
        // KNOWN LIMITATION (deferred): a free-standing `.to_array()` that is consumed as an
        // unbound temporary (`[..].to_array().sum()`) is never bound to a `drop_local`, so its
        // buffer is leaked. Sound (no UAF) and bounded; the "complete drop coverage" slice will
        // either bind such temporaries to synthetic drop slots or fuse the terminal so no
        // materialization happens. Arena mode is unaffected (bulk-freed).
        None => Rvalue::HeapAllocBuf { count: bound.clone(), elem },
    };
    b.push(Stmt::Let(out_ptr, alloc));

    // `acc` is the running output index (= final length); `iv` is the source index.
    let acc = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc, Operand::Const(Const::Int(0, i64_ty()))));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));
    // `scan` threads an accumulator (output element type) seeded with `init`.
    let scan_acc = match &kind {
        CollectKind::Scan { init, .. } => {
            let s = b.new_slot(elem);
            b.push(Stmt::Store(s, init.clone()));
            Some(s)
        }
        CollectKind::Collect => None,
    };

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

    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
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
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                // A scalar element is already loaded; a struct element consumed whole (a
                // `map(f)` with no prior `.field`) is loaded here by index.
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func, captures } => {
                // A scalar element is already loaded; a whole struct element (a struct-consuming
                // predicate, no prior projection) is loaded here by index. `where` keeps the
                // element, so `cur` is left unchanged either way.
                let arg = match &cur {
                    Some(a) => a.clone(),
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("where with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), call_args)));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                let pred = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, Ty::Bool);
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }

    // append: out_ptr[out_idx] = <value>; out_idx += 1. For `to_array` the value is the
    // element; for `scan` it is the updated accumulator `acc = f(acc, element)`.
    let cur = cur.expect("to_array/scan needs a scalar element");
    let value = match (&kind, scan_acc) {
        (CollectKind::Scan { func, captures, .. }, Some(acc_slot)) => {
            let prev = b.fresh_value(elem);
            b.push(Stmt::Let(prev, Rvalue::Load(acc_slot)));
            let folded = b.fresh_value(elem);
            let mut args = vec![Operand::Value(prev), cur];
            for c in captures {
                args.push(lower_expr(b, c));
            }
            b.push(Stmt::Let(folded, Rvalue::Call(func.clone(), args)));
            b.push(Stmt::Store(acc_slot, Operand::Value(folded)));
            Operand::Value(folded)
        }
        _ => cur,
    };
    let out_idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(out_idx, Rvalue::Load(acc)));
    b.push(Stmt::PtrStore(Operand::Value(out_ptr), Operand::Value(out_idx), value));
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
    // Free the source temporary now its elements have been copied into the new buffer.
    if let Some(tmp) = temp_free {
        b.push(Stmt::DropValue(tmp));
    }
    Operand::Value(arr)
}

/// `source.….partition(p)` — one fused loop that splits the surviving scalar elements into two
/// owned arrays (predicate true, then false) and returns them as a tuple `(array<T>, array<T>)`.
/// Mirrors [`lower_array_collect`] but with two buffers + a per-element predicate branch at the
/// append point. Each buffer is sized at the source length (an upper bound).
fn lower_array_partition(
    b: &mut Builder,
    source: &hir::Expr,
    stages: &[hir::Stage],
    elem: Ty,
    pred_func: &str,
    pred_captures: &[hir::Expr],
    tuple_id: u32,
) -> Operand {
    let arena = b.arenas.last().copied();
    let SrcSetup { slot, slice_val, bound, scalar_slot: scalar_slot_src, struct_view, temp_free } = setup_source(b, source);

    // Two output buffers, each an upper-bound `bound` elements (a split never grows the count).
    let alloc_buf = |b: &mut Builder| {
        let p = b.fresh_value(Ty::Box(scalar_of(elem)));
        let alloc = match arena {
            Some(h) => Rvalue::ArenaAlloc { handle: Operand::Value(h), count: bound.clone(), elem },
            // Unbound free-standing buffers leak if the result tuple is never destructured (same
            // bounded caveat as `to_array`); destructured into owned locals, they are freed once.
            None => Rvalue::HeapAllocBuf { count: bound.clone(), elem },
        };
        b.push(Stmt::Let(p, alloc));
        p
    };
    let out_a = alloc_buf(b);
    let out_b = alloc_buf(b);
    let acc_a = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc_a, Operand::Const(Const::Int(0, i64_ty()))));
    let acc_b = b.new_slot(i64_ty());
    b.push(Stmt::Store(acc_b, Operand::Const(Const::Int(0, i64_ty()))));
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
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound.clone())));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: address element i, run the stages.
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);

    let mut cur: Option<Operand> = if struct_view.is_some() {
        None
    } else if let Some(sv) = &slice_val {
        let src_elem = match source.ty {
            Ty::Slice(s) | Ty::DynArray(s) => align_sema::scalar_to_ty(s),
            Ty::DynSliceArray(p) => Ty::Slice(align_sema::prim_to_scalar(p)),
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
                let v = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, stage.out_ty);
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Map { func, captures } => {
                let arg = match cur.take() {
                    Some(a) => a,
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("map with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let v = b.fresh_value(stage.out_ty);
                b.push(Stmt::Let(v, Rvalue::Call(func.clone(), call_args)));
                cur = Some(Operand::Value(v));
            }
            hir::StageKind::Where { func, captures } => {
                let arg = match &cur {
                    Some(a) => a.clone(),
                    None => {
                        let sid = match source.ty {
                            Ty::StructArray(id, _) | Ty::DynStructArray(id, _) => id,
                            _ => unreachable!("where with no loaded element must be over a struct array"),
                        };
                        Operand::Value(lower_struct_elem(b, struct_view, &slice_val, slot, &index, sid))
                    }
                };
                let call_args = stage_call_args(b, arg, captures);
                let pred = b.fresh_value(Ty::Bool);
                b.push(Stmt::Let(pred, Rvalue::Call(func.clone(), call_args)));
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
            hir::StageKind::WhereField { field } => {
                let pred = lower_field_access(b, struct_view, &slice_val, slot, &index, *field, Ty::Bool);
                let keep = b.new_block();
                b.terminate(Term::Branch(Operand::Value(pred), keep, cont));
                b.cur = keep;
            }
        }
    }

    // Split: pred = p(element); true → out_a[acc_a++] = element, false → out_b[acc_b++] = element.
    let cur = cur.expect("partition needs a scalar element");
    let pred = b.fresh_value(Ty::Bool);
    let pred_args = stage_call_args(b, cur.clone(), pred_captures);
    b.push(Stmt::Let(pred, Rvalue::Call(pred_func.to_string(), pred_args)));
    let to_a = b.new_block();
    let to_b = b.new_block();
    b.terminate(Term::Branch(Operand::Value(pred), to_a, to_b));

    let append = |b: &mut Builder, buf: ValueId, acc: Slot| {
        let oi = b.fresh_value(i64_ty());
        b.push(Stmt::Let(oi, Rvalue::Load(acc)));
        b.push(Stmt::PtrStore(Operand::Value(buf), Operand::Value(oi), cur.clone()));
        let n = b.fresh_value(i64_ty());
        b.push(Stmt::Let(n, Rvalue::Bin(BinOp::Add, Operand::Value(oi), index_const(1))));
        b.push(Stmt::Store(acc, Operand::Value(n)));
        b.terminate(Term::Goto(cont));
    };
    b.cur = to_a;
    append(b, out_a, acc_a);
    b.cur = to_b;
    append(b, out_b, acc_b);

    // cont: i += 1; loop.
    b.cur = cont;
    let i2 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i2, Rvalue::Load(iv)));
    let inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(inc, Rvalue::Bin(BinOp::Add, Operand::Value(i2), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(inc)));
    b.terminate(Term::Goto(header));

    // exit: build the two owned arrays and the result tuple `(array<T>, array<T>)`.
    b.cur = exit;
    let la = b.fresh_value(i64_ty());
    b.push(Stmt::Let(la, Rvalue::Load(acc_a)));
    let arr_a = b.fresh_value(Ty::DynArray(scalar_of(elem)));
    b.push(Stmt::Let(arr_a, Rvalue::MakeDynArray { ptr: Operand::Value(out_a), len: Operand::Value(la) }));
    let lb = b.fresh_value(i64_ty());
    b.push(Stmt::Let(lb, Rvalue::Load(acc_b)));
    let arr_b = b.fresh_value(Ty::DynArray(scalar_of(elem)));
    b.push(Stmt::Let(arr_b, Rvalue::MakeDynArray { ptr: Operand::Value(out_b), len: Operand::Value(lb) }));
    if let Some(tmp) = temp_free {
        b.push(Stmt::DropValue(tmp));
    }
    let tup = b.fresh_value(Ty::Tuple(tuple_id));
    b.push(Stmt::Let(tup, Rvalue::MakeTuple { tuple_id, elems: vec![Operand::Value(arr_a), Operand::Value(arr_b)] }));
    Operand::Value(tup)
}

/// `source.….sort()` — materialize the surviving elements into an owned `array<T>` (the
/// `to_array` collect loop), then sort that buffer ascending in place with insertion sort.
/// Reads use `SliceIndex` over the `{ptr,len}` value; writes use `PtrStore` through its buffer
/// pointer (`SlicePtr`). Returns the same owned array. O(n²) — fine for the small arrays this
/// first cut targets; a faster sort is a follow-up.
/// A `sort_by_key` key: the per-element key function, its captures, and the key type. The
/// insertion sort compares `key(a) > key(b)` instead of `a > b`.
struct SortKey {
    func: String,
    captures: Vec<hir::Expr>,
    key_ty: Ty,
}

fn lower_array_sort(b: &mut Builder, source: &hir::Expr, stages: &[hir::Stage], elem: Ty, sort_key: Option<SortKey>) -> Operand {
    let arr = lower_array_collect(b, source, stages, elem, CollectKind::Collect);
    // Lower the key function's captures ONCE before the loop — they are loop-invariant, so
    // re-lowering them inside the per-comparison block would emit redundant loads on the hot path
    // (and LICM is not run). `key_of` reuses these pre-lowered operands.
    let lowered_captures: Vec<Operand> = match &sort_key {
        Some(sk) => sk.captures.iter().map(|c| lower_expr(b, c)).collect(),
        None => Vec::new(),
    };
    // Compute the sort key of an element value (`key(elem)` for `sort_by_key`, else the element).
    let key_of = |b: &mut Builder, v: Operand| -> Operand {
        match &sort_key {
            Some(sk) => {
                let kc = b.fresh_value(sk.key_ty);
                let mut args = Vec::with_capacity(1 + lowered_captures.len());
                args.push(v);
                args.extend(lowered_captures.iter().cloned());
                b.push(Stmt::Let(kc, Rvalue::Call(sk.func.clone(), args)));
                Operand::Value(kc)
            }
            None => v,
        }
    };
    let ptr = b.fresh_value(Ty::Box(scalar_of(elem)));
    b.push(Stmt::Let(ptr, Rvalue::SlicePtr(arr.clone())));
    let len = b.fresh_value(i64_ty());
    b.push(Stmt::Let(len, Rvalue::SliceLen(arr.clone())));

    // i = 1; while i < len { key = arr[i]; j = i-1; while j >= 0 && arr[j] > key { arr[j+1] =
    // arr[j]; j-- }; arr[j+1] = key; i++ }.
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(1, i64_ty()))));
    let jv = b.new_slot(i64_ty());

    let outer = b.new_block();
    let outer_body = b.new_block();
    let inner = b.new_block();
    let cmp_bb = b.new_block();
    let shift = b.new_block();
    let place = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(outer));

    // outer: while i < len
    b.cur = outer;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let ocond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(ocond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), Operand::Value(len))));
    b.terminate(Term::Branch(Operand::Value(ocond), outer_body, exit));

    // outer_body: key = arr[i]; j = i - 1.
    b.cur = outer_body;
    let i_cur = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_cur, Rvalue::Load(iv)));
    let key = b.fresh_value(elem);
    b.push(Stmt::Let(key, Rvalue::SliceIndex(arr.clone(), Operand::Value(i_cur))));
    // The sort key of the element being inserted (invariant across the inner loop).
    let key_cmp = key_of(b, Operand::Value(key));
    let j0 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(j0, Rvalue::Bin(BinOp::Sub, Operand::Value(i_cur), index_const(1))));
    b.push(Stmt::Store(jv, Operand::Value(j0)));
    b.terminate(Term::Goto(inner));

    // inner: while j >= 0 (then test arr[j] > key in cmp_bb).
    b.cur = inner;
    let j_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(j_val, Rvalue::Load(jv)));
    let jge0 = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(jge0, Rvalue::Bin(BinOp::Ge, Operand::Value(j_val), index_const(0))));
    b.terminate(Term::Branch(Operand::Value(jge0), cmp_bb, place));

    // cmp_bb: if arr[j] > key, shift; else place.
    b.cur = cmp_bb;
    let aj = b.fresh_value(elem);
    b.push(Stmt::Let(aj, Rvalue::SliceIndex(arr.clone(), Operand::Value(j_val))));
    // Compare keys: `key(arr[j]) > key(element)` (for a plain sort, the keys are the elements).
    let aj_cmp = key_of(b, Operand::Value(aj));
    let gt = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(gt, Rvalue::Bin(BinOp::Gt, aj_cmp, key_cmp.clone())));
    b.terminate(Term::Branch(Operand::Value(gt), shift, place));

    // shift: arr[j+1] = arr[j]; j -= 1.
    b.cur = shift;
    let jp1 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(jp1, Rvalue::Bin(BinOp::Add, Operand::Value(j_val), index_const(1))));
    b.push(Stmt::PtrStore(Operand::Value(ptr), Operand::Value(jp1), Operand::Value(aj)));
    let jdec = b.fresh_value(i64_ty());
    b.push(Stmt::Let(jdec, Rvalue::Bin(BinOp::Sub, Operand::Value(j_val), index_const(1))));
    b.push(Stmt::Store(jv, Operand::Value(jdec)));
    b.terminate(Term::Goto(inner));

    // place: arr[j+1] = key; i += 1. `jv` is unchanged between `inner` (which dominates `place`)
    // and here — only `shift` writes it, and `shift` loops back to `inner` — so `j_val` from
    // `inner` is still current; reuse it instead of re-loading.
    b.cur = place;
    let jf1 = b.fresh_value(i64_ty());
    b.push(Stmt::Let(jf1, Rvalue::Bin(BinOp::Add, Operand::Value(j_val), index_const(1))));
    b.push(Stmt::PtrStore(Operand::Value(ptr), Operand::Value(jf1), Operand::Value(key)));
    let i_inc = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_inc, Rvalue::Bin(BinOp::Add, Operand::Value(i_cur), index_const(1))));
    b.push(Stmt::Store(iv, Operand::Value(i_inc)));
    b.terminate(Term::Goto(outer));

    b.cur = exit;
    arr
}

/// `a.dot(b)` — the inner product `Σ a[i]*b[i]` of two fixed-length scalar arrays of equal
/// (sema-checked) length, folded in one counted loop. Both sources materialize to a slot
/// (`array_source_slot`); `mul`/`add` lower per element type (int or float).
fn lower_array_dot(b: &mut Builder, a: &hir::Expr, bex: &hir::Expr, elem: Ty) -> Operand {
    let (a_slot, n) = array_source_slot(b, a);
    let (b_slot, _nb) = array_source_slot(b, bex);

    let acc = b.new_slot(elem);
    b.push(Stmt::Store(acc, zero_of(elem)));
    let iv = b.new_slot(i64_ty());
    b.push(Stmt::Store(iv, Operand::Const(Const::Int(0, i64_ty()))));
    let bound = Operand::Const(Const::Int(n, i64_ty()));

    let header = b.new_block();
    let body = b.new_block();
    let cont = b.new_block();
    let exit = b.new_block();
    b.terminate(Term::Goto(header));

    // header: while i < n
    b.cur = header;
    let i_val = b.fresh_value(i64_ty());
    b.push(Stmt::Let(i_val, Rvalue::Load(iv)));
    let cond = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(cond, Rvalue::Bin(BinOp::Lt, Operand::Value(i_val), bound)));
    b.terminate(Term::Branch(Operand::Value(cond), body, exit));

    // body: acc += a[i] * b[i].
    b.cur = body;
    let idx = b.fresh_value(i64_ty());
    b.push(Stmt::Let(idx, Rvalue::Load(iv)));
    let index = Operand::Value(idx);
    let xa = b.fresh_value(elem);
    b.push(Stmt::Let(xa, Rvalue::Index(a_slot, index.clone())));
    let xb = b.fresh_value(elem);
    b.push(Stmt::Let(xb, Rvalue::Index(b_slot, index)));
    let prod = b.fresh_value(elem);
    b.push(Stmt::Let(prod, Rvalue::Bin(BinOp::Mul, Operand::Value(xa), Operand::Value(xb))));
    let a_acc = b.fresh_value(elem);
    b.push(Stmt::Let(a_acc, Rvalue::Load(acc)));
    let next = b.fresh_value(elem);
    b.push(Stmt::Let(next, Rvalue::Bin(BinOp::Add, Operand::Value(a_acc), Operand::Value(prod))));
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
    let r = b.fresh_value(elem);
    b.push(Stmt::Let(r, Rvalue::Load(acc)));
    Operand::Value(r)
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
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecode { struct_id, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
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
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `json.decode(input)` into an owned `array<elem>` → materialize the array into an out slot via
/// the runtime parser (status `i32`), then branch into `Ok(<array>)` / `Err(<code>)`. Mirrors
/// [`lower_json_decode`]; the array is heap-owned (the unwrapped local `Drop`-frees it).
fn lower_json_decode_array(b: &mut Builder, elem: Ty, input: &hir::Expr, result_ty: Ty) -> Operand {
    let arr_ty = Ty::DynArray(scalar_of(elem));
    let out = b.new_slot(arr_ty);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecodeArray { elem, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized array {ptr,len} and wrap it (it owns its buffer now).
    b.cur = ok_bb;
    let a = b.fresh_value(arr_ty);
    b.push(Stmt::Let(a, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(a))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code (the out slot was zeroed → no buffer allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `fs.read_file(path)` → read the file into an owned `string` materialized in an out slot via
/// the runtime (status `i32`), then branch `Ok(<string>)` / `Err(<code>)`. Mirrors
/// [`lower_json_decode_array`]; the `string` is heap-owned (the unwrapped local `Drop`-frees it).
/// Wrap a runtime builtin's i32 status `code` into `Error.Code(code)` — the `err` payload of
/// `result_ty`'s `Result<_, Error>` (4b-2). The Error enum id comes from `result_ty`'s err scalar.
fn make_error_code(b: &mut Builder, code: ValueId, result_ty: Ty) -> Operand {
    let error_id = match result_ty {
        Ty::Result(_, align_sema::Scalar::Enum(eid)) => eid,
        _ => 0, // sema guarantees `Result<_, Error>` for these builtins
    };
    let ev = b.fresh_value(Ty::Enum(error_id));
    b.push(Stmt::Let(
        ev,
        Rvalue::MakeEnum { enum_id: error_id, variant: align_sema::ERROR_VARIANT_CODE, payload: vec![Operand::Value(code)] },
    ));
    Operand::Value(ev)
}

fn lower_fs_read_file(b: &mut Builder, path: &hir::Expr, result_ty: Ty) -> Operand {
    let out = b.new_slot(Ty::String);
    let p = lower_expr(b, path);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::FsReadFile { path: p, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized string {ptr,len} and wrap it (it owns its buffer now).
    b.cur = ok_bb;
    let s = b.fresh_value(Ty::String);
    b.push(Stmt::Let(s, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(s))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code (the out slot was zeroed → no buffer allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `io.stdout.write(arg)` → call the runtime writer (status `i32`), then branch `Ok(())` /
/// `Err(<code>)`. No out slot — the result payload is unit. `write_rv` builds the status-producing
/// rvalue from the lowered argument (a `str` value or a `builder` handle).
fn lower_io_stdout_write(
    b: &mut Builder,
    arg: &hir::Expr,
    result_ty: Ty,
    write_rv: impl FnOnce(Operand) -> Rvalue,
) -> Operand {
    let a = lower_expr(b, arg);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, write_rv(a)));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: wrap unit.
    b.cur = ok_bb;
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Const(Const::Unit))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code.
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
    b.push(Stmt::Store(rslot, Operand::Value(errv)));
    b.terminate(Term::Goto(join));

    b.cur = join;
    let r = b.fresh_value(result_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
    Operand::Value(r)
}

/// `json.decode(input)` into an owned `array<Struct>` (MMv2 slice 8d) → materialize the AoS into
/// an out slot via the runtime parser (status `i32`), then branch `Ok(<array>)` / `Err(<code>)`.
/// Mirrors [`lower_json_decode_array`]; the AoS buffer is heap-owned (the unwrapped local
/// `Drop`-frees it), while its elements' `str` fields remain views into the input.
fn lower_json_decode_struct_array(b: &mut Builder, struct_id: u32, input: &hir::Expr, result_ty: Ty) -> Operand {
    let arr_ty = Ty::DynStructArray(struct_id, Layout::Aos);
    let out = b.new_slot(arr_ty);
    let inp = lower_expr(b, input);
    let code = b.fresh_value(status_ty());
    b.push(Stmt::Let(code, Rvalue::JsonDecodeStructArray { struct_id, input: inp, out }));

    let isok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(isok, Rvalue::Bin(BinOp::Eq, Operand::Value(code), Operand::Const(Const::Int(0, status_ty())))));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    let rslot = b.new_slot(result_ty);
    b.terminate(Term::Branch(Operand::Value(isok), ok_bb, err_bb));

    // Ok: load the materialized array {ptr,len} and wrap it (it owns its buffer now).
    b.cur = ok_bb;
    let a = b.fresh_value(arr_ty);
    b.push(Stmt::Let(a, Rvalue::Load(out)));
    let okv = b.fresh_value(result_ty);
    b.push(Stmt::Let(okv, Rvalue::ResultOk(Operand::Value(a))));
    b.push(Stmt::Store(rslot, Operand::Value(okv)));
    b.terminate(Term::Goto(join));

    // Err: wrap the status code (the out slot was zeroed → no buffer allocated on failure).
    b.cur = err_bb;
    let errv = b.fresh_value(result_ty);
    let ec = make_error_code(b, code, result_ty);
    b.push(Stmt::Let(errv, Rvalue::ResultErr(ec)));
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
    // `?` exits the function: free open arenas and drop owned locals first.
    b.emit_exit_cleanup();
    b.terminate(Term::Return(Some(Operand::Value(propagated))));

    // Ok: continue with the unwrapped value. If the operand was a bound local holding an owned
    // payload (e.g. `r: Result<string,E>`), the payload is now moved into `v`, so null the source
    // slot — its exit `Drop` then frees null, not the moved-out buffer (no double-free). On the
    // Err edge the source's ok payload is already {null,0} (zeroed at construction), so the
    // exit-cleanup drop there is a harmless no-op.
    b.cur = ok_bb;
    let v = b.fresh_value(ok_ty);
    b.push(Stmt::Let(v, Rvalue::ResultUnwrapOk(r)));
    null_moved_source(b, inner);
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

    // Some: unwrap the payload into the result slot. If the source was a bound local with an
    // owned payload (`opt: Option<string>`), null it — the payload moved into the result slot, so
    // its exit `Drop` must free null (the `None` edge already has a {null,0} payload).
    b.cur = some_bb;
    let val = b.fresh_value(ty);
    b.push(Stmt::Let(val, Rvalue::OptionUnwrap(opt_op)));
    b.push(Stmt::Store(result_slot, Operand::Value(val)));
    null_moved_source(b, opt);
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

/// `match scrutinee { … }`: lower per scrutinee kind — a user `enum` (a tag-compare chain over the
/// non-union struct) or builtin `Option`/`Result` (a single 2-way branch on `IsSome`/`IsOk`).
fn lower_match(b: &mut Builder, scrutinee: &hir::Expr, arms: &[hir::MatchArm], ty: Ty) -> Operand {
    // A zero-arm `match` is already a (non-exhaustive) sema error; lower the scrutinee for its
    // effects and yield unit so we never panic on the indexing below.
    if arms.is_empty() {
        lower_expr(b, scrutinee);
        return Operand::Const(Const::Unit);
    }
    let result_slot = (ty != Ty::Unit).then(|| b.new_slot(ty));
    let scrut = lower_expr(b, scrutinee);
    let join_bb = b.new_block();
    match scrutinee.ty {
        Ty::Enum(enum_id) => lower_match_enum(b, enum_id, arms, &scrut, result_slot, join_bb),
        Ty::Option(_) | Ty::Result(..) => lower_match_binary(b, scrutinee.ty, arms, &scrut, result_slot, join_bb),
        // Guarded by sema (`match` requires a sum type); be defensive rather than panic.
        _ => b.terminate(Term::Goto(join_bb)),
    }
    b.cur = join_bb;
    match result_slot {
        Some(slot) => {
            let v = b.fresh_value(ty);
            b.push(Stmt::Let(v, Rvalue::Load(slot)));
            Operand::Value(v)
        }
        None => Operand::Const(Const::Unit),
    }
}

/// A user `enum`: test the scrutinee's tag against each arm's variant and branch to its body,
/// defaulting to the `_`/last arm.
fn lower_match_enum(b: &mut Builder, enum_id: u32, arms: &[hir::MatchArm], scrut: &Operand, result_slot: Option<Slot>, join_bb: BlockId) {
    let default_idx = arms.iter().position(|a| a.variant.is_none()).unwrap_or(arms.len() - 1);
    for (i, arm) in arms.iter().enumerate() {
        if i == default_idx {
            continue;
        }
        let v = arm.variant.expect("a non-default match arm has a variant");
        let eq = b.fresh_value(Ty::Bool);
        b.push(Stmt::Let(eq, Rvalue::EnumTagEq { enum_id, scrutinee: scrut.clone(), variant: v }));
        let arm_bb = b.new_block();
        let next_bb = b.new_block();
        b.terminate(Term::Branch(Operand::Value(eq), arm_bb, next_bb));
        b.cur = arm_bb;
        for (slot, &local) in arm.bindings.iter().enumerate() {
            bind_local(b, local, Rvalue::EnumPayload { enum_id, variant: v, slot: slot as u32, operand: scrut.clone() });
        }
        finish_arm(b, &arm.body, result_slot, join_bb);
        b.cur = next_bb;
    }
    let d = &arms[default_idx];
    if let Some(v) = d.variant {
        for (slot, &local) in d.bindings.iter().enumerate() {
            bind_local(b, local, Rvalue::EnumPayload { enum_id, variant: v, slot: slot as u32, operand: scrut.clone() });
        }
    }
    finish_arm(b, &d.body, result_slot, join_bb);
}

/// Builtin `Option`/`Result` (exactly two variants): one boolean branch on `IsSome`/`IsOk`, the
/// `true` edge to the Some/Ok arm and `false` to the None/Err arm. Variant 0 = Some/Ok, 1 = None/Err
/// (matching `match_variants`); either side may be the `_` wildcard.
fn lower_match_binary(b: &mut Builder, ty: Ty, arms: &[hir::MatchArm], scrut: &Operand, result_slot: Option<Slot>, join_bb: BlockId) {
    let wild = arms.iter().find(|a| a.variant.is_none());
    let pos = arms.iter().find(|a| a.variant == Some(0)).or(wild).expect("exhaustive (sema)");
    let neg = arms.iter().find(|a| a.variant == Some(1)).or(wild).expect("exhaustive (sema)");
    // A lone `_` covers both variants — no test needed.
    if std::ptr::eq(pos, neg) {
        finish_arm(b, &pos.body, result_slot, join_bb);
        return;
    }
    let cond = b.fresh_value(Ty::Bool);
    let test = match ty {
        Ty::Option(_) => Rvalue::OptionIsSome(scrut.clone()),
        _ => Rvalue::ResultIsOk(scrut.clone()),
    };
    b.push(Stmt::Let(cond, test));
    let pos_bb = b.new_block();
    let neg_bb = b.new_block();
    b.terminate(Term::Branch(Operand::Value(cond), pos_bb, neg_bb));
    b.cur = pos_bb;
    bind_binary(b, ty, true, pos, scrut);
    finish_arm(b, &pos.body, result_slot, join_bb);
    b.cur = neg_bb;
    bind_binary(b, ty, false, neg, scrut);
    finish_arm(b, &neg.body, result_slot, join_bb);
}

/// Bind the payload of an `Option`/`Result` arm: Some/Ok → the unwrapped value, Err → the error;
/// None (and any `_` wildcard) binds nothing.
fn bind_binary(b: &mut Builder, ty: Ty, is_pos: bool, arm: &hir::MatchArm, scrut: &Operand) {
    if arm.variant.is_none() || arm.bindings.is_empty() {
        return;
    }
    let rv = match (ty, is_pos) {
        (Ty::Option(_), true) => Rvalue::OptionUnwrap(scrut.clone()),
        (Ty::Result(..), true) => Rvalue::ResultUnwrapOk(scrut.clone()),
        (Ty::Result(..), false) => Rvalue::ResultUnwrapErr(scrut.clone()),
        _ => return,
    };
    bind_local(b, arm.bindings[0], rv);
}

/// Compute an rvalue into a fresh value and store it into a binding local's slot.
fn bind_local(b: &mut Builder, local: u32, rv: Rvalue) {
    let pty = b.slots[local as usize];
    let pv = b.fresh_value(pty);
    b.push(Stmt::Let(pv, rv));
    b.push(Stmt::Store(local, Operand::Value(pv)));
}

/// Lower an arm body and, unless it diverged, store the value into the result slot and jump to join.
fn finish_arm(b: &mut Builder, body: &hir::Expr, result_slot: Option<Slot>, join_bb: BlockId) {
    let av = lower_expr(b, body);
    if !b.is_terminated() {
        if let Some(slot) = result_slot {
            b.push(Stmt::Store(slot, av));
        }
        b.terminate(Term::Goto(join_bb));
    }
}

/// `result.map_err(f)` — branch on `Result`: `Ok(v)` passes through; `Err(e)` becomes `Err(f(e))`.
fn lower_map_err(b: &mut Builder, result: &hir::Expr, f: &hir::Expr, out_ty: Ty) -> Operand {
    let (ok_s, e_s) = match result.ty {
        Ty::Result(o, e) => (o, e),
        _ => return lower_expr(b, result), // guarded by sema
    };
    let e2_ty = match out_ty {
        Ty::Result(_, e2) => align_sema::scalar_to_ty(e2),
        _ => out_ty,
    };
    let rv = lower_expr(b, result);
    let fv = lower_expr(b, f);
    let rslot = b.new_slot(out_ty);
    let is_ok = b.fresh_value(Ty::Bool);
    b.push(Stmt::Let(is_ok, Rvalue::ResultIsOk(rv.clone())));
    let ok_bb = b.new_block();
    let err_bb = b.new_block();
    let join = b.new_block();
    b.terminate(Term::Branch(Operand::Value(is_ok), ok_bb, err_bb));
    // Ok: pass the payload through unchanged.
    b.cur = ok_bb;
    let okp = b.fresh_value(align_sema::scalar_to_ty(ok_s));
    b.push(Stmt::Let(okp, Rvalue::ResultUnwrapOk(rv.clone())));
    let okr = b.fresh_value(out_ty);
    b.push(Stmt::Let(okr, Rvalue::ResultOk(Operand::Value(okp))));
    b.push(Stmt::Store(rslot, Operand::Value(okr)));
    b.terminate(Term::Goto(join));
    // Err: apply `f` to the error, re-wrap.
    b.cur = err_bb;
    let errp = b.fresh_value(align_sema::scalar_to_ty(e_s));
    b.push(Stmt::Let(errp, Rvalue::ResultUnwrapErr(rv)));
    let conv = b.fresh_value(e2_ty);
    b.push(Stmt::Let(
        conv,
        Rvalue::CallIndirect { callee: fv, args: vec![Operand::Value(errp)], param_tys: vec![align_sema::scalar_to_ty(e_s)], ret_ty: e2_ty },
    ));
    let errr = b.fresh_value(out_ty);
    b.push(Stmt::Let(errr, Rvalue::ResultErr(Operand::Value(conv))));
    b.push(Stmt::Store(rslot, Operand::Value(errr)));
    b.terminate(Term::Goto(join));
    b.cur = join;
    let r = b.fresh_value(out_ty);
    b.push(Stmt::Let(r, Rvalue::Load(rslot)));
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
        Ty::DynStructArray(id, _) => format!("array<struct#{id}>"),
        Ty::DynSliceArray(_) => "array<slice>".to_string(),
        Ty::Str => "str".to_string(),
        Ty::String => "string".to_string(),
        Ty::ArenaHandle => "arena".to_string(),
        Ty::Builder => "builder".to_string(),
        Ty::Struct(id) => format!("struct#{id}"),
        Ty::Tuple(id) => format!("tuple#{id}"),
        Ty::Fn(id) => format!("fn#{id}"),
        Ty::Enum(id) => format!("enum#{id}"),
        Ty::Task(_) => "Task".to_string(),
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
