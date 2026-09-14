//! Compiler-selected storage for bounded, nonescaping byte objects.
//!
//! The plan is derived from the actual MIR, never imported as trusted metadata. Unrecognized
//! uses or ambiguous byte extents select the ordinary Buffer ABI. No new source type is formed.
use crate::{Function, Operand, Rvalue, Slot, Stmt, Term, ValueId};
use align_sema::Ty;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const OBJECT_LIMIT: usize = 64;
const FUNCTION_LIMIT: usize = 1024;

#[derive(Default)]
pub struct ByteStoragePlan {
    slots: BTreeMap<Slot, usize>,
    constructors: BTreeMap<ValueId, Slot>,
    writes: BTreeMap<ValueId, usize>,
    lengths: BTreeMap<ValueId, usize>,
}

impl ByteStoragePlan {
    pub fn slots(&self) -> impl Iterator<Item = (Slot, usize)> + '_ {
        self.slots.iter().map(|(&slot, &size)| (slot, size))
    }
    pub fn contains(&self, slot: Slot) -> bool {
        self.slots.contains_key(&slot)
    }
    pub fn constructor(&self, value: ValueId) -> Option<Slot> {
        self.constructors.get(&value).copied()
    }
    pub fn write_offset(&self, value: ValueId) -> Option<usize> {
        self.writes.get(&value).copied()
    }
    pub fn length(&self, value: ValueId) -> Option<usize> {
        self.lengths.get(&value).copied()
    }
}

fn value(op: &Operand) -> Option<ValueId> {
    if let Operand::Value(id) = op {
        Some(*id)
    } else {
        None
    }
}

/// SSA/slot may-origins are only used to reject escapes; they never certify initialization.
fn related(op: &Operand, values: &BTreeSet<ValueId>, slots: &BTreeSet<Slot>) -> bool {
    match op {
        Operand::Value(id) => values.contains(id),
        Operand::BorrowedPlace(place) => slots.contains(&place.slot),
        // These complex place carriers are outside this deliberately local storage proof.
        Operand::BorrowedElementPlace(_) | Operand::BorrowedFixedElementPlace(_) => true,
        Operand::Const(_) | Operand::Arg(_) | Operand::BorrowedCleanupArg(_) => false,
    }
}

fn byte_width(ty: Ty) -> Option<usize> {
    match ty {
        Ty::Int(align_sema::IntTy {
            bits: 8 | 16 | 32 | 64,
            ..
        })
        | Ty::Float(align_sema::FloatTy { bits: 32 | 64 }) => {
            let bits = match ty {
                Ty::Int(t) => t.bits,
                Ty::Float(t) => t.bits,
                _ => return None,
            };
            Some(usize::from(bits) / 8)
        }
        _ => None,
    }
}

fn literal_lengths(f: &Function) -> BTreeMap<ValueId, usize> {
    let mut lengths = BTreeMap::new();
    loop {
        let before = lengths.len();
        for block in &f.blocks {
            for stmt in &block.stmts {
                if let Stmt::Let(id, rv) = stmt {
                    let length = match rv {
                        Rvalue::StrLit(text) => Some(text.len()),
                        Rvalue::Use(op) => value(op).and_then(|id| lengths.get(&id).copied()),
                        _ => None,
                    };
                    if let Some(length) = length {
                        lengths.insert(*id, length);
                    }
                }
            }
        }
        if lengths.len() == before {
            return lengths;
        }
    }
}

fn nonescaping(f: &Function, slot: Slot, constructor: ValueId) -> Option<BTreeSet<ValueId>> {
    if f.params.contains(&slot) {
        return None;
    }
    let mut handles = BTreeSet::from([constructor]);
    let mut values = handles.clone();
    let mut slots = BTreeSet::from([slot]);
    loop {
        let before = (values.len(), slots.len());
        for block in &f.blocks {
            for stmt in &block.stmts {
                match stmt {
                    Stmt::Let(id, Rvalue::Load(source)) if slots.contains(source) => {
                        values.insert(*id);
                        if *source == slot {
                            handles.insert(*id);
                        }
                    }
                    Stmt::Let(id, Rvalue::Use(op) | Rvalue::BufferBytes(op))
                        if related(op, &values, &slots) =>
                    {
                        values.insert(*id);
                    }
                    Stmt::Let(id, Rvalue::SubSlice { base, .. })
                        if related(base, &values, &slots) =>
                    {
                        values.insert(*id);
                    }
                    Stmt::Store(dst, op) if related(op, &values, &slots) => {
                        slots.insert(*dst);
                    }
                    _ => {}
                }
            }
        }
        if before == (values.len(), slots.len()) {
            break;
        }
    }
    let is_related = |op: &Operand| related(op, &values, &slots);
    let is_handle = |op: &Operand| value(op).is_some_and(|id| handles.contains(&id));
    let mut constructor_stores = 0;
    for block in &f.blocks {
        for stmt in &block.stmts {
            let valid = match stmt {
                Stmt::Store(dst, op) if *dst == slot => {
                    constructor_stores += 1;
                    value(op) == Some(constructor)
                }
                Stmt::Store(dst, op) => {
                    !is_handle(op)
                        && (!is_related(op)
                            || (!f.params.contains(dst)
                                && matches!(f.slots.get(*dst as usize), Some(Ty::Slice(_)))))
                }
                Stmt::Drop(target) | Stmt::DropFlagInit(target) => {
                    *target == slot || !slots.contains(target)
                }
                Stmt::Let(id, rv) => match rv {
                    Rvalue::BufferNew(_)
                    | Rvalue::Load(_)
                    | Rvalue::StrLit(_)
                    | Rvalue::RawNull => true,
                    Rvalue::BufferPut {
                        buffer,
                        value: input,
                        ..
                    } => {
                        !is_related(input)
                            && (!is_related(buffer)
                                || (is_handle(buffer) && value(buffer) != Some(constructor)))
                    }
                    Rvalue::BufferAppend { buffer, data } => {
                        !is_related(data)
                            && (!is_related(buffer)
                                || (is_handle(buffer) && value(buffer) != Some(constructor)))
                    }
                    Rvalue::BufferBytes(op) | Rvalue::BufferLen(op) => {
                        !is_related(op) || (is_handle(op) && value(op) != Some(constructor))
                    }
                    Rvalue::SliceLen(op) => !is_handle(op),
                    Rvalue::BytesRead { bytes, offset, .. } => {
                        !is_handle(bytes) && !is_related(offset)
                    }
                    Rvalue::SubSlice {
                        base, start, len, ..
                    } => !is_handle(base) && !is_related(start) && !is_related(len),
                    Rvalue::Use(op) => {
                        !is_handle(op)
                            && (!is_related(op)
                                || matches!(f.value_tys.get(*id as usize), Some(Ty::Slice(_))))
                    }
                    Rvalue::Un(_, op)
                    | Rvalue::Cast { operand: op, .. }
                    | Rvalue::OptionSome(op)
                    | Rvalue::OptionIsSome(op)
                    | Rvalue::OptionUnwrap(op)
                    | Rvalue::ResultOk(op)
                    | Rvalue::ResultErr(op)
                    | Rvalue::ResultIsOk(op)
                    | Rvalue::ResultUnwrapOk(op)
                    | Rvalue::ResultUnwrapErr(op) => !is_related(op),
                    Rvalue::Bin(_, a, b) => !is_related(a) && !is_related(b),
                    Rvalue::Select { cond, a, b } => {
                        !is_related(cond) && !is_related(a) && !is_related(b)
                    }
                    Rvalue::OptionNone => true,
                    Rvalue::Call(_, args) => args.iter().all(|op| !is_related(op)),
                    Rvalue::CallIndirect { callee, args, .. } => {
                        !is_related(callee) && args.iter().all(|op| !is_related(op))
                    }
                    Rvalue::MathOp { operands, .. } => operands.iter().all(|op| !is_related(op)),
                    // A new/unaudited operation may retain nested operands or address a slot.
                    // Reject the object rather than assuming a scalar result means no escape.
                    _ => false,
                },
                _ => false,
            };
            if !valid {
                return None;
            }
        }
        let valid = match &block.term {
            Term::Goto(_) | Term::Unreachable | Term::Return(None) => true,
            Term::Branch(op, ..) | Term::Return(Some(op)) => !is_related(op),
            Term::ReturnWithCleanup(result) => !is_related(&result.0) && !is_related(&result.1),
        };
        if !valid {
            return None;
        }
    }
    (constructor_stores == 1).then_some(handles)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Extent {
    Dead,
    Bytes(usize),
    Mixed,
}

impl Extent {
    fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Mixed }
    }
    fn append(self, width: usize) -> Self {
        match self {
            Self::Bytes(len) => len
                .checked_add(width)
                .filter(|n| *n <= OBJECT_LIMIT)
                .map_or(Self::Mixed, Self::Bytes),
            _ => Self::Mixed,
        }
    }
}

fn write_width(
    rv: &Rvalue,
    handles: &BTreeSet<ValueId>,
    literals: &BTreeMap<ValueId, usize>,
) -> Option<Option<usize>> {
    match rv {
        Rvalue::BufferPut { buffer, scalar, .. }
            if value(buffer).is_some_and(|id| handles.contains(&id)) =>
        {
            Some(byte_width(*scalar))
        }
        Rvalue::BufferAppend { buffer, data }
            if value(buffer).is_some_and(|id| handles.contains(&id)) =>
        {
            Some(value(data).and_then(|id| literals.get(&id).copied()))
        }
        _ => None,
    }
}

fn transfer(
    stmt: &Stmt,
    state: Extent,
    slot: Slot,
    constructor: ValueId,
    handles: &BTreeSet<ValueId>,
    literals: &BTreeMap<ValueId, usize>,
) -> Extent {
    match stmt {
        Stmt::Let(id, Rvalue::BufferNew(_)) if *id == constructor => Extent::Bytes(0),
        Stmt::Drop(target) | Stmt::DropFlagInit(target) if *target == slot => Extent::Dead,
        Stmt::Let(_, rv) => match write_width(rv, handles, literals) {
            Some(Some(width)) => state.append(width),
            Some(None) => Extent::Mixed,
            None => state,
        },
        _ => state,
    }
}

fn object_plan(
    f: &Function,
    slot: Slot,
    constructor: ValueId,
    literals: &BTreeMap<ValueId, usize>,
) -> Option<ByteStoragePlan> {
    let handles = nonescaping(f, slot, constructor)?;
    let mut incoming = vec![None; f.blocks.len()];
    *incoming.get_mut(f.entry as usize)? = Some(Extent::Dead);
    let mut pending = VecDeque::from([f.entry]);
    while let Some(id) = pending.pop_front() {
        let block = f.blocks.get(id as usize)?;
        let mut state = (*incoming.get(id as usize)?)?;
        for stmt in &block.stmts {
            state = transfer(stmt, state, slot, constructor, &handles, literals);
        }
        let successors: &[u32] = match &block.term {
            Term::Goto(target) => std::slice::from_ref(target),
            Term::Branch(_, a, b) => &[*a, *b],
            _ => &[],
        };
        for &next in successors {
            let entry = incoming.get_mut(next as usize)?;
            let joined = Some(entry.map_or(state, |old| old.join(state)));
            if *entry != joined {
                *entry = joined;
                pending.push_back(next);
            }
        }
    }
    let mut plan = ByteStoragePlan::default();
    let mut maximum = 1;
    for block in &f.blocks {
        let mut state = (*incoming.get(block.id as usize)?)?;
        for stmt in &block.stmts {
            if let Stmt::Let(id, rv) = stmt {
                if let Some(width) = write_width(rv, &handles, literals) {
                    let Extent::Bytes(offset) = state else {
                        return None;
                    };
                    let end = offset.checked_add(width?)?;
                    if end > OBJECT_LIMIT {
                        return None;
                    }
                    maximum = maximum.max(end);
                    plan.writes.insert(*id, offset);
                }
                if let Rvalue::BufferBytes(op) | Rvalue::BufferLen(op) = rv
                    && value(op).is_some_and(|id| handles.contains(&id))
                {
                    let Extent::Bytes(length) = state else {
                        return None;
                    };
                    plan.lengths.insert(*id, length);
                }
            }
            state = transfer(stmt, state, slot, constructor, &handles, literals);
        }
    }
    plan.slots.insert(slot, maximum);
    plan.constructors.insert(constructor, slot);
    Some(plan)
}

/// Select complete objects in slot order under a deterministic total stack budget. This is an
/// optimization selector, not a replacement for source, lifetime or malformed-MIR validation.
pub fn plan(f: &Function) -> ByteStoragePlan {
    let constructors: BTreeSet<_> = f
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| {
            if let Stmt::Let(id, Rvalue::BufferNew(Operand::Const(crate::Const::Int(cap, _)))) =
                stmt
                && *cap <= OBJECT_LIMIT as i128
            {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    if constructors.is_empty() {
        return ByteStoragePlan::default();
    }
    let mut candidates = BTreeMap::new();
    for block in &f.blocks {
        for stmt in &block.stmts {
            if let Stmt::Store(slot, Operand::Value(id)) = stmt
                && f.slots.get(*slot as usize) == Some(&Ty::Buffer)
                && constructors.contains(id)
            {
                candidates.insert(*slot, *id);
            }
        }
    }
    if candidates.is_empty() {
        return ByteStoragePlan::default();
    }
    let literals = literal_lengths(f);
    let mut result = ByteStoragePlan::default();
    let mut used = 0;
    for (slot, constructor) in candidates {
        if used == FUNCTION_LIMIT {
            break;
        }
        if let Some(candidate) = object_plan(f, slot, constructor, &literals) {
            let size: usize = candidate.slots.values().sum();
            if used + size > FUNCTION_LIMIT {
                continue;
            }
            used += size;
            result.slots.extend(candidate.slots);
            result.constructors.extend(candidate.constructors);
            result.writes.extend(candidate.writes);
            result.lengths.extend(candidate.lengths);
        }
    }
    result
}
