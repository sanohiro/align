//! Remove byte-read guards only after proving the original reached edge safe.
//! This pass consumes actual MIR, never serialized optimizer metadata. Unknown
//! operations, mutable parameter aliases and ambiguous recurrences keep the guard.
use crate::{
    BlockId, Const, DirectCall, Function, Operand, RuntimeKey, Rvalue, Slot, Stmt, Term, ValueId,
};
use align_ast::{BinOp, ParamMode};
use align_sema::{IntTy, Scalar, Ty};
use std::collections::{BTreeMap, BTreeSet};

fn integer() -> Ty {
    Ty::Int(IntTy {
        bits: 64,
        signed: true,
    })
}
fn bytes() -> Ty {
    Ty::Slice(Scalar::Int(IntTy {
        bits: 8,
        signed: false,
    }))
}
fn literal(op: &Operand, n: i128) -> bool {
    matches!(op, Operand::Const(Const::Int(value, ty)) if *value == n && *ty == integer())
}

fn same(a: &Operand, b: &Operand) -> bool {
    matches!((a, b), (Operand::Value(left), Operand::Value(right)) if left == right)
}

struct Facts<'a> {
    f: &'a Function,
    defs: BTreeMap<ValueId, (BlockId, usize, &'a Rvalue)>,
    stores: BTreeMap<Slot, Vec<(BlockId, usize, &'a Operand)>>,
}

impl<'a> Facts<'a> {
    fn new(f: &'a Function) -> Option<Self> {
        if f.blocks.get(f.entry as usize).is_none()
            || f.param_modes.len() != f.params.len()
            || f.param_modes
                .iter()
                .any(|mode| !matches!(mode, ParamMode::ByValue | ParamMode::Borrow))
        {
            return None;
        }
        let mut facts = Self {
            f,
            defs: BTreeMap::new(),
            stores: BTreeMap::new(),
        };
        for (index, block) in f.blocks.iter().enumerate() {
            if block.id as usize != index {
                return None;
            }
            for successor in successors(&block.term) {
                f.blocks.get(successor as usize)?;
            }
            for (position, stmt) in block.stmts.iter().enumerate() {
                match stmt {
                    Stmt::Store(slot, op) => {
                        f.slots.get(*slot as usize)?;
                        facts
                            .stores
                            .entry(*slot)
                            .or_default()
                            .push((block.id, position, op));
                    }
                    Stmt::Let(id, rv) => {
                        f.value_tys.get(*id as usize)?;
                        if facts.defs.insert(*id, (block.id, position, rv)).is_some() {
                            return None;
                        }
                        match rv {
                            Rvalue::Load(_)
                            | Rvalue::Use(_)
                            | Rvalue::SliceLen(_)
                            | Rvalue::Bin(..)
                            | Rvalue::Un(..)
                            | Rvalue::Cast { .. }
                            | Rvalue::BytesRead { .. } => {}
                            Rvalue::Call(
                                DirectCall::Runtime(RuntimeKey::RangeFail | RuntimeKey::BoundsFail),
                                _,
                            ) => {}
                            // In particular no opaque effects or hidden place writes.
                            _ => return None,
                        }
                    }
                    _ => return None,
                }
            }
        }
        // Every incoming descriptor is bound exactly once, to its own argument.
        // Borrow parameters therefore cannot be overwritten through an alias.
        for (arg, slot) in f.params.iter().enumerate() {
            let stores = facts.stores.get(slot)?;
            if stores.len() != 1
                || stores[0].0 != f.entry
                || !matches!(stores[0].2, Operand::Arg(index) if *index as usize == arg)
            {
                return None;
            }
        }
        Some(facts)
    }

    fn rv(&self, op: &Operand) -> Option<&'a Rvalue> {
        let Operand::Value(id) = op else {
            return None;
        };
        self.defs.get(id).map(|(_, _, rv)| *rv)
    }

    fn ty(&self, op: &Operand) -> Option<Ty> {
        match op {
            Operand::Value(id) => self.f.value_tys.get(*id as usize).copied(),
            Operand::Const(Const::Int(_, ty)) => Some(*ty),
            _ => None,
        }
    }

    fn load(&self, op: &Operand, ty: Ty) -> Option<Slot> {
        let Rvalue::Load(slot) = self.rv(op)? else {
            return None;
        };
        (self.ty(op) == Some(ty) && self.f.slots.get(*slot as usize) == Some(&ty)).then_some(*slot)
    }

    fn bin(&self, op: &Operand, kind: BinOp) -> Option<(&'a Operand, &'a Operand)> {
        let Rvalue::Bin(actual, left, right) = self.rv(op)? else {
            return None;
        };
        (*actual == kind).then_some((left, right))
    }

    fn length(&self, op: &Operand) -> Option<Slot> {
        let Rvalue::SliceLen(source) = self.rv(op)? else {
            return None;
        };
        if self.ty(op) != Some(integer()) {
            return None;
        }
        let slot = self.load(source, bytes())?;
        // Only a stable incoming byte view is admitted in this capability.
        self.f.params.contains(&slot).then_some(slot)
    }

    fn limit(&self, op: &Operand, width: i128) -> Option<Slot> {
        let (length, divisor) = self.bin(op, BinOp::Div)?;
        if self.ty(op) != Some(integer()) || !literal(divisor, width) {
            return None;
        }
        self.length(length)
    }

    fn reachable(&self, start: BlockId, target: BlockId, avoid: Option<BlockId>) -> bool {
        let mut seen = BTreeSet::new();
        let mut pending = vec![start];
        while let Some(block) = pending.pop() {
            if Some(block) == avoid || !seen.insert(block) {
                continue;
            }
            if block == target {
                return true;
            }
            pending.extend(successors(&self.f.blocks[block as usize].term));
        }
        false
    }

    fn dominates(&self, before: BlockId, after: BlockId) -> bool {
        self.reachable(self.f.entry, after, None)
            && !self.reachable(self.f.entry, after, Some(before))
    }

    fn definition_precedes(&self, operand: &Operand, block: BlockId, position: usize) -> bool {
        match operand {
            Operand::Value(id) => self.defs.get(id).is_some_and(|(owner, index, _)| {
                if *owner == block {
                    *index < position
                } else {
                    self.dominates(*owner, block)
                }
            }),
            Operand::Const(_) => true,
            _ => false,
        }
    }

    fn expression_precedes(&self, operand: &Operand, block: BlockId, position: usize) -> bool {
        let mut pending = vec![(operand, block, position)];
        while let Some((op, owner, before)) = pending.pop() {
            if !self.definition_precedes(op, owner, before) {
                return false;
            }
            if let Operand::Value(id) = op {
                let Some((block, index, rv)) = self.defs.get(id) else {
                    return false;
                };
                match rv {
                    Rvalue::Load(slot) => {
                        if self.f.params.contains(slot) {
                            let Some(stores) = self.stores.get(slot) else {
                                return false;
                            };
                            let Some((bound, at, _)) = stores.first() else {
                                return false;
                            };
                            if if *bound == *block {
                                *at >= *index
                            } else {
                                !self.dominates(*bound, *block)
                            } {
                                return false;
                            }
                        }
                    }
                    Rvalue::SliceLen(op) | Rvalue::Use(op) => pending.push((op, *block, *index)),
                    Rvalue::Bin(_, a, b) => {
                        pending.push((a, *block, *index));
                        pending.push((b, *block, *index));
                    }
                    _ => return false,
                }
            }
        }
        true
    }

    fn defined_in(&self, op: &Operand, block: BlockId) -> bool {
        matches!(op, Operand::Value(id) if self.defs.get(id).is_some_and(|(owner, _, _)| *owner == block))
    }

    fn recurrence(&self, slot: Slot, header: BlockId, admitted: BlockId, read: BlockId) -> bool {
        let Some(stores) = self.stores.get(&slot) else {
            return false;
        };
        if stores.len() != 2 {
            return false;
        }
        let Some(initial) = stores.iter().find(|(_, _, op)| literal(op, 0)) else {
            return false;
        };
        if !self.dominates(initial.0, header) {
            return false;
        }
        if self.defs.values().any(|(owner, position, rv)| {
            matches!(rv, Rvalue::Load(source) if *source == slot)
                && if *owner == initial.0 {
                    *position <= initial.1
                } else {
                    !self.dominates(initial.0, *owner)
                }
        }) {
            return false;
        }
        let Some(step) = stores.iter().find(|entry| !literal(entry.2, 0)) else {
            return false;
        };
        let Some((value, one)) = self.bin(step.2, BinOp::Add) else {
            return false;
        };
        if !literal(one, 1)
            || self.load(value, integer()) != Some(slot)
            || self.ty(step.2) != Some(integer())
            || !self.expression_precedes(step.2, step.0, step.1)
            || !self.defined_in(value, step.0)
            || !self.dominates(admitted, step.0)
        {
            return false;
        }
        // The step must close this iteration. It cannot run twice or feed a
        // read without rechecking the loop admission on the next iteration.
        if !matches!(self.f.blocks[step.0 as usize].term, Term::Goto(target) if target == header) {
            return false;
        }
        if step.0 != read && self.reachable(step.0, read, Some(header)) {
            return false;
        }
        // No initialization or step may intervene between admission and a read.
        // For a read in the step block, the caller checks its statement position.
        !self.reachable(admitted, initial.0, Some(header))
    }

    fn safe_guard(&self, block: BlockId) -> Option<BlockId> {
        let current = &self.f.blocks[block as usize];
        let Term::Branch(condition, fail, ok) = &current.term else {
            return None;
        };
        let fail_block = &self.f.blocks[*fail as usize];
        let [Stmt::Let(_, Rvalue::Call(DirectCall::Runtime(RuntimeKey::RangeFail), args))] =
            fail_block.stmts.as_slice()
        else {
            return None;
        };
        if args.len() != 3 || !matches!(fail_block.term, Term::Unreachable) {
            return None;
        }
        let (start, end, length) = (&args[0], &args[1], &args[2]);
        let (prefix, over) = self.bin(condition, BinOp::Or)?;
        let (negative, inverted) = self.bin(prefix, BinOp::Or)?;
        let (n_start, zero) = self.bin(negative, BinOp::Lt)?;
        let (i_start, i_end) = self.bin(inverted, BinOp::Gt)?;
        let (o_end, o_length) = self.bin(over, BinOp::Gt)?;
        if !same(n_start, start)
            || !literal(zero, 0)
            || !same(i_start, start)
            || !same(i_end, end)
            || !same(o_end, end)
            || !same(o_length, length)
        {
            return None;
        }
        for operand in [condition, prefix, over, negative, inverted] {
            if self.ty(operand) != Some(Ty::Bool) {
                return None;
            }
        }
        let (scaled, amount) = self.bin(end, BinOp::Add)?;
        if !same(scaled, start) || self.ty(end) != Some(integer()) {
            return None;
        }
        let width = [1, 2, 4, 8]
            .into_iter()
            .find(|width| literal(amount, *width))?;
        let (index, factor) = self.bin(start, BinOp::Mul)?;
        if !literal(factor, width) || self.ty(start) != Some(integer()) {
            return None;
        }
        let slot = self.load(index, integer())?;
        if !self.defined_in(index, block) {
            return None;
        }
        let source = self.length(length)?;
        // The actual read must agree with all three checked operands and width.
        let mut read_position = None;
        for (position, statement) in self.f.blocks[*ok as usize].stmts.iter().enumerate() {
            if let Stmt::Let(
                id,
                Rvalue::BytesRead {
                    bytes: view,
                    offset,
                    scalar,
                    ..
                },
            ) = statement
            {
                let bits = match scalar {
                    Ty::Int(ty) if matches!(ty.bits, 8 | 16 | 32 | 64) => ty.bits,
                    Ty::Float(ty) if matches!(ty.bits, 32 | 64) => ty.bits,
                    _ => return None,
                };
                if same(offset, start)
                    && self.load(view, bytes()) == Some(source)
                    && self.expression_precedes(view, *ok, position)
                    && i128::from(bits) == width * 8
                    && self.f.value_tys.get(*id as usize) == Some(scalar)
                {
                    read_position = Some(position);
                    break;
                }
            }
        }
        let read_position = read_position?;
        if !self.expression_precedes(start, *ok, read_position) {
            return None;
        }
        for operand in [condition, start, end, length] {
            if !self.expression_precedes(operand, block, current.stmts.len()) {
                return None;
            }
        }
        for header in &self.f.blocks {
            let Term::Branch(test, yes, no) = &header.term else {
                continue;
            };
            let (index, limit, admitted) = if let Some((index, limit)) = self.bin(test, BinOp::Ge) {
                (index, limit, *no)
            } else if let Some((index, limit)) = self.bin(test, BinOp::Lt) {
                (index, limit, *yes)
            } else {
                continue;
            };
            if self.ty(test) != Some(Ty::Bool)
                || self.load(index, integer()) != Some(slot)
                || !self.defined_in(index, header.id)
                || !self.expression_precedes(test, header.id, header.stmts.len())
                || self.limit(limit, width) != Some(source)
                || !self.dominates(admitted, block)
                || !self.dominates(header.id, block)
                || !self.recurrence(slot, header.id, admitted, *ok)
            {
                continue;
            }
            // A mutable-slot load used by the guard/read must precede the step.
            let stores = self.stores.get(&slot)?;
            if stores
                .iter()
                .any(|(owner, position, _)| *owner == *ok && *position <= read_position)
            {
                continue;
            }
            return Some(*ok);
        }
        None
    }
}

fn successors(term: &Term) -> Vec<BlockId> {
    match term {
        Term::Goto(target) => vec![*target],
        Term::Branch(_, yes, no) => vec![*yes, *no],
        Term::Return(_) | Term::ReturnWithCleanup(_) | Term::Unreachable => Vec::new(),
    }
}

/// Restrict this first recurrence proof to scalar/read-only bodies. Rewriting a
/// proven branch retains its statements, trap block, IDs and source coordinates;
/// there is no hoisted trap, speculative load or unchecked-load IR extension.
pub fn simplify(function: &mut Function) {
    if !function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .any(|stmt| matches!(stmt, Stmt::Let(_, Rvalue::BytesRead { .. })))
    {
        return;
    }
    let edits = {
        let Some(facts) = Facts::new(function) else {
            return;
        };
        function
            .blocks
            .iter()
            .filter_map(|block| facts.safe_guard(block.id).map(|ok| (block.id, ok)))
            .collect::<Vec<_>>()
    };
    let mut original = Vec::new();
    for (block, ok) in edits {
        let term = std::mem::replace(&mut function.blocks[block as usize].term, Term::Goto(ok));
        original.push((block, term));
    }
    if Facts::new(function).is_none() {
        for (block, term) in original {
            function.blocks[block as usize].term = term;
        }
    }
}
