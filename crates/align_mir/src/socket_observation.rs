//! Certify bound socket inputs at the native call and isolate each record's output slot.
use super::{Block, Const, Function, Operand, Rvalue, Slot, Stmt, Term, ValueId};
use crate::operand_inputs::OperandInputs;
use crate::producer::ProducerError;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

fn borrowed_root(operand: &Operand) -> Option<Slot> {
    match operand {
        Operand::BorrowedPlace(place) => Some(place.slot),
        Operand::BorrowedElementPlace(place) => Some(place.base.slot),
        Operand::BorrowedFixedElementPlace(place) => Some(place.base),
        _ => None,
    }
}

// A bound expression is either a projected match binding (loaded by the call), or a local
// loaded immediately before the call. Both source operands are names, so no side effect
// may intervene between loading their handles and the native use.
fn current_root(block: &Block, position: usize, operand: &Operand) -> Option<Slot> {
    match operand {
        Operand::BorrowedPlace(place) => Some(place.slot),
        Operand::Value(value) => block.stmts[..position]
            .iter()
            .rev()
            .map_while(|statement| match statement {
                Stmt::Let(id, Rvalue::Load(slot)) => Some((*id, *slot)),
                _ => None,
            })
            .find_map(|(id, slot)| (id == *value).then_some(slot)),
        _ => None,
    }
}

// Direct slot mutations (borrowed addresses and native writers are inventoried separately).
fn modified_slot(statement: &Stmt) -> Option<Slot> {
    match statement {
        Stmt::Store(slot, _)
        | Stmt::StoreField(slot, ..)
        | Stmt::StoreIndex(slot, ..)
        | Stmt::StoreConstArray { slot, .. }
        | Stmt::StoreElemField(slot, ..)
        | Stmt::DropFlagInit(slot)
        | Stmt::DropFlagMoveOut { slot, .. }
        | Stmt::NullTupleField(slot, _)
        | Stmt::NullStructField(slot, _)
        | Stmt::NullElemField(slot, ..)
        | Stmt::Drop(slot)
        | Stmt::DropField(slot, _)
        | Stmt::DropElem(slot, ..)
        | Stmt::DropElemField(slot, ..)
        | Stmt::ArrayTruncate { root: slot, .. } => Some(*slot),
        _ => None,
    }
}

pub(super) fn validate(function: &Function) -> Result<(), ProducerError> {
    let fail = || {
        ProducerError::Lowering(format!(
            "socket observation in '{}' requires live bound inputs and a private record output",
            function.name
        ))
    };
    let mut outputs = HashSet::new();
    let mut success_blocks = HashMap::new();
    let mut sources = HashMap::new();
    let mut tracked = BTreeSet::new();
    for block in &function.blocks {
        for (position, stmt) in block.stmts.iter().enumerate() {
            if let Stmt::Let(
                id,
                Rvalue::SocketObserve {
                    socket,
                    buffer,
                    out,
                },
            ) = stmt
            {
                let socket = current_root(block, position, socket).ok_or_else(&fail)?;
                let buffer = match buffer {
                    Some(op) => Some(current_root(block, position, op).ok_or_else(&fail)?),
                    None => None,
                };
                if !outputs.insert(*out) || sources.insert(*id, (socket, buffer)).is_some() {
                    return Err(fail());
                }
                let Term::Branch(Operand::Value(test), yes, no) = &block.term else {
                    return Err(fail());
                };
                if yes == no || !block.stmts[position + 1..].iter().any(|s| matches!(s,
                    Stmt::Let(condition, Rvalue::Bin(align_ast::BinOp::Eq, Operand::Value(status),
                        Operand::Const(Const::Int(0, ty))))
                        if condition == test && status == id && *ty == super::status_ty()
                )) { return Err(fail()); }
                success_blocks.insert(*out, (block.id, *yes));
                tracked.insert(socket);
                tracked.extend(buffer);
            }
        }
    }
    if outputs.is_empty() {
        return Ok(());
    }
    let private = |op: &Operand| borrowed_root(op).is_none_or(|slot| !outputs.contains(&slot));
    let mut loads = HashMap::<ValueId, Slot>::new();
    let mut transferred = HashSet::new();
    let mut predecessors = HashMap::<_, BTreeSet<_>>::new();
    for block in &function.blocks {
        for next in crate::byte_ranges::successors(&block.term) {
            predecessors.entry(next).or_default().insert(block.id);
        }
        if !block.term.operands_satisfy(&mut |op| private(op)) {
            return Err(fail());
        }
        for stmt in &block.stmts {
            if modified_slot(stmt).is_some_and(|slot| outputs.contains(&slot))
                || !stmt.operands_satisfy(&mut |op| private(op))
            {
                return Err(fail());
            }
            match stmt {
                Stmt::Let(value, Rvalue::Load(slot)) => {
                    loads.insert(*value, *slot);
                    if let Some((_, success)) = success_blocks.get(slot)
                        && (*success != block.id || !transferred.insert(*slot))
                    {
                        return Err(fail());
                    }
                }
                Stmt::Let(
                    _,
                    Rvalue::MakeSlice(slot, _)
                    | Rvalue::MakeFieldSlice(slot, ..)
                    | Rvalue::Field(slot, _)
                    | Rvalue::Index(slot, _)
                    | Rvalue::IndexField(slot, ..),
                ) if outputs.contains(slot) => return Err(fail()),
                Stmt::DropFlagMoveOut { flag, .. } if outputs.contains(flag) => return Err(fail()),
                _ => {}
            }
        }
    }

    if transferred != outputs
        || success_blocks.values().any(|(source, target)| {
            *target == function.entry
                || predecessors
                    .get(target)
                    .is_none_or(|pred| pred.len() != 1 || !pred.contains(source))
        })
    {
        return Err(fail());
    }

    // Must-initialization: merge by intersection. A zeroed or dropped owner is never a
    // usable handle. The static producer graph separately authenticates the stored type,
    // active projection, exclusive buffer permission and the native output's unique writer.
    let entry = function
        .params
        .iter()
        .zip(&function.param_modes)
        .filter(|(slot, mode)| {
            tracked.contains(slot)
                && matches!(
                    mode,
                    align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut
                )
        })
        .map(|(slot, _)| *slot)
        .collect::<BTreeSet<_>>();
    let mut entries = vec![None; function.blocks.len()];
    *entries.get_mut(function.entry as usize).ok_or_else(&fail)? = Some(entry);
    let mut pending = VecDeque::from([function.entry]);
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop_front() {
        let block = function
            .blocks
            .get(id as usize)
            .filter(|b| b.id == id)
            .ok_or_else(&fail)?;
        let mut live = entries[id as usize].clone().ok_or_else(&fail)?;
        for stmt in &block.stmts {
            match stmt {
                Stmt::Let(id, Rvalue::SocketObserve { .. }) => {
                    let (socket, buffer) = sources.get(id).ok_or_else(&fail)?;
                    if !live.contains(socket) || buffer.is_some_and(|s| !live.contains(&s)) {
                        return Err(fail());
                    }
                    seen.insert(*id);
                }
                Stmt::Store(slot, _) if tracked.contains(slot) => {
                    live.insert(*slot);
                }
                Stmt::Let(_, rv) => {
                    // Calls receive Move arguments as values and reborrows as places.
                    // A value handed to a consuming callee no longer certifies its old slot.
                    let arguments: &[Operand] = match rv {
                        Rvalue::Call(super::DirectCall::Program(_), args)
                        | Rvalue::Call(
                            super::DirectCall::Runtime(
                                super::RuntimeKey::BufferFree
                                | super::RuntimeKey::TcpConnFree
                                | super::RuntimeKey::TcpListenerFree
                                | super::RuntimeKey::UdpSocketFree,
                            ),
                            args,
                        )
                        | Rvalue::CallIndirect { args, .. }
                        | Rvalue::RawCall { args, .. } => args,
                        Rvalue::CallWithCleanup(call) => &call.args,
                        Rvalue::CallIndirectWithCleanup(call) => &call.args,
                        _ => &[],
                    };
                    for op in arguments {
                        if let Operand::Value(value) = op
                            && let Some(slot) = loads.get(value)
                        {
                            live.remove(slot);
                        }
                    }
                }
                Stmt::DropValue(operand) => {
                    let root = match operand {
                        Operand::Value(value) => loads.get(value).copied(),
                        _ => borrowed_root(operand),
                    };
                    if let Some(slot) = root {
                        live.remove(&slot);
                    }
                }
                _ => {
                    if let Some(slot) = modified_slot(stmt) {
                        live.remove(&slot);
                    }
                }
            }
        }
        for next in crate::byte_ranges::successors(&block.term) {
            let entry = entries.get_mut(next as usize).ok_or_else(&fail)?;
            if let Some(previous) = entry {
                let before = previous.len();
                previous.retain(|slot| live.contains(slot));
                if previous.len() == before {
                    continue;
                }
            } else {
                *entry = Some(live.clone());
            }
            pending.push_back(next);
        }
    }
    if seen.len() == sources.len() {
        Ok(())
    } else {
        Err(fail())
    }
}
