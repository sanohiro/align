//! Emission-scoped exposure of bounded scalar byte readers.
//!
//! Prepared bodies never enter the scope-independent MIR cache. A partition may
//! consume only definitions included in its own implementation identity.
use crate::{
    Block, DirectCall, ExceptionalEdge, Function, Operand, Program, ProgramCall, Rvalue, Stmt,
    Term,
};
use align_ast::ParamMode;
use align_sema::Ty;
use std::borrow::Cow;
use std::collections::BTreeSet;

fn scalar(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Unit
    )
}

/// The same mapper defines eligibility and cloning, so a newly added operation
/// cannot accidentally be admitted without remapping its operands.
fn map_rvalue(
    rv: &Rvalue,
    slot_base: u32,
    mut operand: impl FnMut(&Operand) -> Option<Operand>,
) -> Option<Rvalue> {
    Some(match rv {
        Rvalue::Load(slot) => Rvalue::Load(slot.checked_add(slot_base)?),
        Rvalue::Use(a) => Rvalue::Use(operand(a)?),
        Rvalue::Un(op, a) => Rvalue::Un(*op, operand(a)?),
        Rvalue::Bin(op, a, b) => Rvalue::Bin(*op, operand(a)?, operand(b)?),
        Rvalue::Cast {
            operand: a,
            from,
            to,
        } => Rvalue::Cast {
            operand: operand(a)?,
            from: *from,
            to: *to,
        },
        Rvalue::SliceLen(a) => Rvalue::SliceLen(operand(a)?),
        Rvalue::BytesRead {
            bytes,
            offset,
            scalar,
            be,
        } => Rvalue::BytesRead {
            bytes: operand(bytes)?,
            offset: operand(offset)?,
            scalar: *scalar,
            be: *be,
        },
        Rvalue::Call(
            DirectCall::Runtime(
                key @ (crate::RuntimeKey::RangeFail | crate::RuntimeKey::BoundsFail),
            ),
            args,
        ) => Rvalue::Call(
            DirectCall::Runtime(*key),
            args.iter().map(&mut operand).collect::<Option<_>>()?,
        ),
        _ => return None,
    })
}

fn eligible(f: &Function) -> bool {
    if !super::byte_ranges::structurally_valid(f)
        || f.blocks.get(f.entry as usize).is_none()
        || !scalar(f.ret)
        || f.ret == Ty::Unit
        || f.blocks.len() > 16
        || f.blocks.is_empty()
        || f.blocks.iter().map(|b| b.stmts.len()).sum::<usize>() > 128
        || f.return_cleanup != align_sema::hir::ReturnCleanupAbi::None
        || f.param_modes.len() != f.params.len()
        || f.param_modes
            .iter()
            .any(|m| !matches!(m, ParamMode::ByValue | ParamMode::Borrow))
        || f.slots
            .iter()
            .any(|ty| !scalar(*ty) && !matches!(ty, Ty::Slice(_)))
    {
        return false;
    }
    for (arg, slot) in f.params.iter().enumerate() {
        let stores: Vec<_> = f
            .blocks
            .iter()
            .flat_map(|b| {
                b.stmts.iter().filter_map(move |s| match s {
                    Stmt::Store(target, value) if target == slot => Some((b.id, value)),
                    _ => None,
                })
            })
            .collect();
        if !matches!(stores.as_slice(), [(block, Operand::Arg(index))] if *block == f.entry && *index as usize == arg)
        {
            return false;
        }
    }
    // Topological elimination rejects cycles, including unreachable malformed
    // cycles. Check all successor IDs before indexing any block.
    let mut incoming = vec![0usize; f.blocks.len()];
    for (i, b) in f.blocks.iter().enumerate() {
        if usize::try_from(b.id).ok() != Some(i) {
            return false;
        }
        for next in super::byte_ranges::successors(&b.term) {
            let Some(n) = incoming.get_mut(next as usize) else {
                return false;
            };
            *n += 1;
        }
        if matches!(b.term, Term::ReturnWithCleanup(_)) {
            return false;
        }
        for s in &b.stmts {
            match s {
                Stmt::Store(slot, _) if f.slots.get(*slot as usize).is_some() => {}
                Stmt::Let(id, rv)
                    if f.value_tys.get(*id as usize).is_some()
                        && map_rvalue(rv, 0, |a| match a {
                            Operand::Const(_) | Operand::Value(_) | Operand::Arg(_) => {
                                Some(a.clone())
                            }
                            _ => None,
                        })
                        .is_some() => {}
                _ => return false,
            }
        }
    }
    let mut pending: Vec<_> = incoming
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect();
    let mut count = 0;
    while let Some(i) = pending.pop() {
        count += 1;
        for next in super::byte_ranges::successors(&f.blocks[i].term) {
            incoming[next as usize] -= 1;
            if incoming[next as usize] == 0 {
                pending.push(next as usize);
            }
        }
    }
    count == f.blocks.len()
        && f.blocks.iter().any(|b| {
            b.stmts
                .iter()
                .any(|s| matches!(s, Stmt::Let(_, Rvalue::BytesRead { .. })))
        })
}

fn expose(caller: &mut Function, block: usize, position: usize, leaf: &Function) -> Option<()> {
    let Stmt::Let(result, Rvalue::Call(_, args)) = caller.blocks.get(block)?.stmts.get(position)?
    else {
        return None;
    };
    if args.len() != leaf.params.len() || caller.value_tys.get(*result as usize) != Some(&leaf.ret)
    {
        return None;
    }
    let result = *result;
    let args = args.clone();
    let slot_base = u32::try_from(caller.slots.len()).ok()?;
    let value_base = u32::try_from(caller.value_tys.len()).ok()?;
    let block_base = u32::try_from(caller.blocks.len()).ok()?;
    let continuation = block_base.checked_add(u32::try_from(leaf.blocks.len()).ok()?)?;
    let result_slot = slot_base.checked_add(u32::try_from(leaf.slots.len()).ok()?)?;
    value_base
        .checked_add(u32::try_from(leaf.value_tys.len()).ok()?)?
        .checked_add(u32::try_from(args.len()).ok()?)?;
    let mut bindings = Vec::new();
    let mut materialized = Vec::new();
    let mut materialized_tys = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let expected = *leaf.slots.get(*leaf.params.get(i)? as usize)?;
        match arg {
            Operand::BorrowedPlace(place)
                if leaf.param_modes[i] == ParamMode::Borrow
                    && place.path.is_empty()
                    && place.cleanup.is_none()
                    && place.ty == expected
                    && caller.slots.get(place.slot as usize) == Some(&expected) =>
            {
                let id = value_base
                    .checked_add(u32::try_from(leaf.value_tys.len() + materialized.len()).ok()?)?;
                materialized.push(Stmt::Let(id, Rvalue::Load(place.slot)));
                materialized_tys.push(expected);
                bindings.push(Operand::Value(id));
            }
            Operand::Value(id) if caller.value_tys.get(*id as usize) == Some(&expected) => {
                bindings.push(arg.clone())
            }
            Operand::Const(_) if caller.operand_ty(arg) == expected => bindings.push(arg.clone()),
            _ => return None,
        }
    }
    let remap = |op: &Operand| -> Option<Operand> {
        Some(match op {
            Operand::Value(id) if leaf.value_tys.get(*id as usize).is_some() => {
                Operand::Value(id.checked_add(value_base)?)
            }
            Operand::Arg(i) => bindings.get(*i as usize)?.clone(),
            Operand::Const(_) => op.clone(),
            _ => return None,
        })
    };
    let mut cloned = Vec::new();
    let cloned_exceptional_edges = leaf
        .exceptional_edges
        .iter()
        .map(|edge| {
            Some(ExceptionalEdge {
                block: edge.block.checked_add(block_base)?,
                unlikely: edge.unlikely,
                kind: edge.kind,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    for b in &leaf.blocks {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            stmts.push(match s {
                Stmt::Let(id, rv) => Stmt::Let(
                    id.checked_add(value_base)?,
                    map_rvalue(rv, slot_base, remap)?,
                ),
                Stmt::Store(slot, a) => Stmt::Store(slot.checked_add(slot_base)?, remap(a)?),
                _ => return None,
            });
        }
        let mut lines = b.stmt_lines.clone();
        let term = match &b.term {
            Term::Goto(next) => Term::Goto(next.checked_add(block_base)?),
            Term::Branch(c, yes, no) => Term::Branch(
                remap(c)?,
                yes.checked_add(block_base)?,
                no.checked_add(block_base)?,
            ),
            Term::Return(Some(a)) => {
                lines.resize(stmts.len(), (0, 0));
                lines.push((0, 0));
                stmts.push(Stmt::Store(result_slot, remap(a)?));
                Term::Goto(continuation)
            }
            Term::Unreachable => Term::Unreachable,
            _ => return None,
        };
        cloned.push(Block {
            id: b.id.checked_add(block_base)?,
            stmts,
            stmt_lines: lines,
            term,
        });
    }
    let entry = leaf.entry.checked_add(block_base)?;
    let current = &mut caller.blocks[block];
    current.stmt_lines.resize(current.stmts.len(), (0, 0));
    let call_line = current.stmt_lines[position];
    let mut tail = current.stmts.split_off(position);
    tail[0] = Stmt::Let(result, Rvalue::Load(result_slot));
    let tail_lines = current.stmt_lines.split_off(position);
    current
        .stmt_lines
        .extend(std::iter::repeat_n(call_line, materialized.len()));
    current.stmts.extend(materialized);
    let term = std::mem::replace(&mut current.term, Term::Goto(entry));
    cloned.push(Block {
        id: continuation,
        stmts: tail,
        stmt_lines: tail_lines,
        term,
    });
    caller.slots.extend_from_slice(&leaf.slots);
    caller.slots.push(leaf.ret);
    caller.slot_align.resize(slot_base as usize, None);
    caller
        .slot_align
        .extend((0..leaf.slots.len()).map(|i| leaf.slot_align.get(i).copied().flatten()));
    caller.slot_align.push(None);
    caller.value_tys.extend_from_slice(&leaf.value_tys);
    caller.value_tys.extend(materialized_tys);
    caller.blocks.extend(cloned);
    caller.exceptional_edges.extend(cloned_exceptional_edges);
    Some(())
}

// LLVM lowering consumes defining SSA values before their uses. Splicing must
// restore that ordering; numeric append order is not control-flow order.
fn order_blocks(f: &mut Function) -> Vec<u32> {
    let mut seen = BTreeSet::new();
    let mut postorder = Vec::new();
    let mut pending = vec![(f.entry, false)];
    while let Some((id, exit)) = pending.pop() {
        if exit {
            postorder.push(id);
            continue;
        }
        if !seen.insert(id) {
            continue;
        }
        pending.push((id, true));
        for next in super::byte_ranges::successors(&f.blocks[id as usize].term)
            .into_iter()
            .rev()
        {
            pending.push((next, false));
        }
    }
    postorder.reverse();
    postorder.extend(
        f.blocks
            .iter()
            .filter_map(|b| (!seen.contains(&b.id)).then_some(b.id)),
    );
    let mut remap = vec![0; f.blocks.len()];
    for (index, old) in postorder.iter().enumerate() {
        // Existing IDs already bound this inventory to u32.
        let Ok(index) = u32::try_from(index) else {
            return Vec::new();
        };
        remap[*old as usize] = index;
    }
    let mut blocks = Vec::with_capacity(f.blocks.len());
    for old in postorder {
        let mut b = f.blocks[old as usize].clone();
        b.id = remap[old as usize];
        match &mut b.term {
            Term::Goto(next) => *next = remap[*next as usize],
            Term::Branch(_, yes, no) => {
                *yes = remap[*yes as usize];
                *no = remap[*no as usize];
            }
            Term::StrMatch { cases, otherwise, .. } => {
                for (_, target) in cases {
                    let Ok(index) = usize::try_from(*target) else { return Vec::new() };
                    *target = remap[index];
                }
                let Ok(index) = usize::try_from(*otherwise) else { return Vec::new() };
                *otherwise = remap[index];
            }
            _ => {}
        }
        blocks.push(b);
    }
    f.entry = remap[f.entry as usize];
    for edge in &mut f.exceptional_edges {
        edge.block = remap[edge.block as usize];
    }
    f.blocks = blocks;
    remap
}

/// Prepare an owned view using exactly the bodies covered by this emission's
/// cache identity. The original program is reusable for another scope.
pub fn prepare<'a>(program: &'a Program, defined: &BTreeSet<ProgramCall>) -> Cow<'a, Program> {
    let leaves: Vec<_> = program
        .fns
        .iter()
        .filter(|f| defined.contains(&f.name) && eligible(f))
        .collect();
    let mut prepared = Cow::Borrowed(program);
    for (index, f) in program.fns.iter().enumerate() {
        if !defined.contains(&f.name) || !super::byte_ranges::structurally_valid(f) {
            continue;
        }
        let mut sites = Vec::new();
        let mut cost = 0;
        for (bi, b) in f.blocks.iter().enumerate() {
            for (si, s) in b.stmts.iter().enumerate() {
                let Stmt::Let(_, Rvalue::Call(DirectCall::Program(name), _)) = s else {
                    continue;
                };
                let Some(leaf) = leaves.iter().find(|l| &l.name == name && l.name != f.name) else {
                    continue;
                };
                let size = leaf.blocks.iter().map(|b| b.stmts.len()).sum::<usize>();
                if sites.len() == 32 || cost + size > 2048 {
                    continue;
                }
                sites.push((bi, si, *leaf));
                cost += size;
            }
        }
        if sites.is_empty()
            && !f.blocks.iter().any(|b| {
                b.stmts
                    .iter()
                    .any(|s| matches!(s, Stmt::Let(_, Rvalue::BytesRead { .. })))
            })
        {
            continue;
        }
        let prepared = prepared.to_mut();
        let function_name = prepared.fns[index].name.clone();
        let function = &mut prepared.fns[index];
        // Select in source order, splice in reverse so original coordinates stay valid.
        for (bi, si, leaf) in sites.into_iter().rev() {
            let _ = expose(function, bi, si, leaf);
        }
        let block_remap = order_blocks(function);
        super::byte_ranges::simplify(function);
        super::byte_ranges::snapshot_descriptors(function);
        if let Some(facts) = prepared
            .loop_facts
            .iter_mut()
            .find(|facts| facts.function == function_name.as_str())
            && (block_remap.is_empty()
                || facts
                    .counted
                    .iter_mut()
                    .any(|fact| !fact.remap_blocks(&block_remap)))
        {
            facts.counted.clear();
        }
    }
    prepared
}
