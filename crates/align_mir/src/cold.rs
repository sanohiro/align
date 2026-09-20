use std::collections::{HashMap, HashSet};

use align_sema::Ty;

use crate::{
    DirectCall, Function, Operand, Program, ProgramCall, Rvalue, StaticData, StaticDataTarget,
    Stmt, Successor, Term,
};

#[derive(Clone, Copy)]
struct CallSite {
    caller: usize,
    block: u32,
}

pub(crate) fn infer(program: &mut Program) {
    for function in &mut program.fns {
        function.cold = false;
    }
    let mut calls: HashMap<ProgramCall, Vec<CallSite>> = HashMap::new();
    let mut address_taken = HashSet::new();
    let exceptional_regions = program
        .fns
        .iter()
        .map(exceptional_region)
        .collect::<Vec<_>>();

    for (caller, function) in program.fns.iter().enumerate() {
        let reachable = reachable(function);
        for block in &function.blocks {
            if !reachable.get(block.id as usize).copied().unwrap_or(false) {
                continue;
            }
            for statement in &block.stmts {
                let Stmt::Let(_, value) = statement else {
                    continue;
                };
                match value {
                    Rvalue::Call(DirectCall::Program(target), _) => {
                        calls.entry(target.clone()).or_default().push(CallSite {
                            caller,
                            block: block.id,
                        });
                    }
                    Rvalue::CallWithCleanup(call) => {
                        calls
                            .entry(call.target.clone())
                            .or_default()
                            .push(CallSite {
                                caller,
                                block: block.id,
                            });
                    }
                    Rvalue::FnAddr { target, .. } => {
                        address_taken.insert(target.clone());
                    }
                    Rvalue::Closure { lifted, .. } => {
                        address_taken.insert(lifted.clone());
                    }
                    Rvalue::SqliteCallbackDescriptor(descriptor) => {
                        address_taken.insert(descriptor.target.clone());
                    }
                    Rvalue::StaticData(data) => collect_static_targets(data, &mut address_taken),
                    Rvalue::ParMapParallel { func, stages, .. } => {
                        address_taken.insert(func.clone());
                        address_taken.extend(stages.iter().filter_map(|stage| stage.func.clone()));
                    }
                    Rvalue::ParMapReduce { func, .. } => {
                        address_taken.insert(func.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    let candidates = program
        .fns
        .iter()
        .map(|function| {
            !function.exportable
                && !address_taken.contains(&function.name)
                && returns_only_err(function)
        })
        .collect::<Vec<_>>();
    let mut cold = vec![false; program.fns.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for callee in 0..program.fns.len() {
            if cold[callee] || !candidates[callee] {
                continue;
            }
            let Some(sites) = calls.get(&program.fns[callee].name) else {
                continue;
            };
            let mut outside_cycle = false;
            let qualifies = sites.iter().all(|site| {
                if site.caller == callee {
                    return true;
                }
                outside_cycle = true;
                cold[site.caller] || exceptional_regions[site.caller].contains(&site.block)
            });
            if qualifies && outside_cycle {
                cold[callee] = true;
                changed = true;
            }
        }
    }
    for (function, cold) in program.fns.iter_mut().zip(cold) {
        function.cold = cold;
    }
}

fn collect_static_targets(data: &StaticData, targets: &mut HashSet<ProgramCall>) {
    for relocation in &data.relocations {
        match &relocation.target {
            StaticDataTarget::Function(target) => {
                targets.insert(target.clone());
            }
            StaticDataTarget::Record(record) => collect_static_targets(record, targets),
            StaticDataTarget::Bytes { .. } => {}
        }
    }
}

fn reachable(function: &Function) -> Vec<bool> {
    let mut reached = vec![false; function.blocks.len()];
    let mut pending = vec![function.entry];
    while let Some(id) = pending.pop() {
        let Some(slot) = reached.get_mut(id as usize) else {
            return Vec::new();
        };
        if std::mem::replace(slot, true) {
            continue;
        }
        let Some(block) = function.blocks.get(id as usize) else {
            return Vec::new();
        };
        match block.term {
            Term::Goto(next) => pending.push(next),
            Term::Branch(_, yes, no) => pending.extend([yes, no]),
            Term::StrMatch { ref cases, otherwise, .. } => {
                pending.extend(cases.iter().map(|(_, target)| *target));
                pending.push(otherwise);
            }
            Term::Return(_) | Term::ReturnWithCleanup(_) | Term::Unreachable => {}
        }
    }
    reached
}

fn exceptional_region(function: &Function) -> HashSet<u32> {
    let count = function.blocks.len();
    let reached = reachable(function);
    if count == 0 || reached.len() != count {
        return HashSet::new();
    }
    let mut predecessors = vec![Vec::new(); count];
    for block in &function.blocks {
        let successors = match block.term {
            Term::Goto(next) => vec![next],
            Term::Branch(_, yes, no) => vec![yes, no],
            Term::StrMatch { ref cases, otherwise, .. } => cases
                .iter()
                .map(|(_, target)| *target)
                .chain(std::iter::once(otherwise))
                .collect(),
            Term::Return(_) | Term::ReturnWithCleanup(_) | Term::Unreachable => Vec::new(),
        };
        for target in successors {
            let Some(incoming) = predecessors.get_mut(target as usize) else {
                return HashSet::new();
            };
            incoming.push(block.id);
        }
    }
    let universe = reached
        .iter()
        .enumerate()
        .filter_map(|(id, reached)| reached.then_some(id as u32))
        .collect::<HashSet<_>>();
    let mut dominators = vec![universe; count];
    let Some(entry) = dominators.get_mut(function.entry as usize) else {
        return HashSet::new();
    };
    *entry = HashSet::from([function.entry]);
    let mut changed = true;
    while changed {
        changed = false;
        for id in 0..count as u32 {
            if id == function.entry || !reached[id as usize] {
                continue;
            }
            let mut incoming = predecessors[id as usize]
                .iter()
                .copied()
                .filter(|predecessor| reached[*predecessor as usize]);
            let Some(first) = incoming.next() else {
                return HashSet::new();
            };
            let mut next = dominators[first as usize].clone();
            for predecessor in incoming {
                next.retain(|candidate| dominators[predecessor as usize].contains(candidate));
            }
            next.insert(id);
            if next != dominators[id as usize] {
                dominators[id as usize] = next;
                changed = true;
            }
        }
    }
    let roots = function
        .exceptional_edges
        .iter()
        .filter_map(
            |edge| match function.blocks.get(edge.block as usize)?.term {
                Term::Branch(_, yes, no) => Some(match edge.unlikely {
                    Successor::Then => yes,
                    Successor::Else => no,
                }),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    (0..count as u32)
        .filter(|block| {
            reached[*block as usize]
                && roots
                    .iter()
                    .any(|root| dominators[*block as usize].contains(root))
        })
        .collect()
}

fn returns_only_err(function: &Function) -> bool {
    if !matches!(function.ret, Ty::Result(..))
        || function.blocks.is_empty()
        || function.entry as usize >= function.blocks.len()
    {
        return false;
    }
    let reached = reachable(function);
    if reached.len() != function.blocks.len() {
        return false;
    }
    let mut values = HashMap::new();
    let mut stores: HashMap<u32, Vec<&Operand>> = HashMap::new();
    for block in &function.blocks {
        if !reached[block.id as usize] {
            continue;
        }
        for statement in &block.stmts {
            match statement {
                Stmt::Let(id, value) if values.insert(*id, value).is_some() => return false,
                Stmt::Store(slot, value) => stores.entry(*slot).or_default().push(value),
                _ => {}
            }
        }
    }
    for block in &function.blocks {
        if !reached[block.id as usize] {
            continue;
        }
        let returned = match &block.term {
            Term::Return(Some(value)) => Some(value),
            Term::ReturnWithCleanup(value) => Some(&value.0),
            Term::Return(None) => return false,
            Term::Goto(_) | Term::Branch(..) | Term::StrMatch { .. } | Term::Unreachable => None,
        };
        if let Some(value) = returned
            && !operand_is_err(value, &values, &stores, &mut HashSet::new())
        {
            return false;
        }
    }
    true
}

fn operand_is_err<'a>(
    operand: &'a Operand,
    values: &HashMap<u32, &'a Rvalue>,
    stores: &HashMap<u32, Vec<&'a Operand>>,
    visiting: &mut HashSet<(bool, u32)>,
) -> bool {
    let Operand::Value(id) = operand else {
        return false;
    };
    if !visiting.insert((false, *id)) {
        return false;
    }
    let result = match values.get(id).copied() {
        Some(Rvalue::ResultErr(_)) => true,
        Some(Rvalue::Use(value)) => operand_is_err(value, values, stores, visiting),
        Some(Rvalue::Load(slot)) => slot_is_err(*slot, values, stores, visiting),
        _ => false,
    };
    visiting.remove(&(false, *id));
    result
}

fn slot_is_err<'a>(
    slot: u32,
    values: &HashMap<u32, &'a Rvalue>,
    stores: &HashMap<u32, Vec<&'a Operand>>,
    visiting: &mut HashSet<(bool, u32)>,
) -> bool {
    if !visiting.insert((true, slot)) {
        return false;
    }
    let result = stores.get(&slot).is_some_and(|writes| {
        !writes.is_empty()
            && writes
                .iter()
                .all(|value| operand_is_err(value, values, stores, visiting))
    });
    visiting.remove(&(true, slot));
    result
}

#[cfg(test)]
mod tests {
    use align_ast::ParamMode;
    use align_sema::{IntTy, Scalar};

    use super::*;
    use crate::{Block, Const, ExceptionalEdge};

    fn result_ty() -> Ty {
        let scalar = Scalar::Int(IntTy {
            bits: 64,
            signed: true,
        });
        Ty::Result(scalar, scalar)
    }

    fn function(name: &str, blocks: Vec<Block>, value_tys: Vec<Ty>) -> Function {
        Function {
            name: ProgramCall::from_validated(name),
            params: Vec::new(),
            param_modes: Vec::<ParamMode>::new(),
            borrow_mut_cleanup_slots: Vec::new(),
            ret: result_ty(),
            return_borrow: align_sema::hir::ReturnBorrowSummary::None,
            return_region: align_sema::hir::ReturnRegionSummary::None,
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            slots: Vec::new(),
            slot_align: Vec::new(),
            value_tys,
            blocks,
            entry: 0,
            exceptional_edges: Vec::new(),
            cold: false,
            exportable: false,
            available_externally: false,
        }
    }

    fn err_function(name: &str, calls: &[&str]) -> Function {
        let mut statements = calls
            .iter()
            .enumerate()
            .map(|(id, target)| {
                Stmt::Let(
                    id as u32,
                    Rvalue::Call(
                        DirectCall::Program(ProgramCall::from_validated(target)),
                        vec![],
                    ),
                )
            })
            .collect::<Vec<_>>();
        let err = calls.len() as u32;
        statements.push(Stmt::Let(
            err,
            Rvalue::ResultErr(Operand::Const(Const::Int(
                1,
                Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                }),
            ))),
        ));
        function(
            name,
            vec![Block {
                id: 0,
                stmts: statements,
                stmt_lines: Vec::new(),
                term: Term::Return(Some(Operand::Value(err))),
            }],
            vec![result_ty(); calls.len() + 1],
        )
    }

    fn exceptional_caller(target: &str) -> Function {
        let mut function = function(
            "caller",
            vec![
                Block {
                    id: 0,
                    stmts: Vec::new(),
                    stmt_lines: Vec::new(),
                    term: Term::Branch(Operand::Const(Const::Bool(true)), 1, 2),
                },
                Block {
                    id: 1,
                    stmts: Vec::new(),
                    stmt_lines: Vec::new(),
                    term: Term::Unreachable,
                },
                Block {
                    id: 2,
                    stmts: vec![Stmt::Let(
                        0,
                        Rvalue::Call(
                            DirectCall::Program(ProgramCall::from_validated(target)),
                            vec![],
                        ),
                    )],
                    stmt_lines: Vec::new(),
                    term: Term::Unreachable,
                },
            ],
            vec![result_ty()],
        );
        function.exceptional_edges.push(ExceptionalEdge {
            block: 0,
            unlikely: Successor::Else,
            kind: crate::ExceptionalKind::ResultPropagate,
        });
        function
    }

    #[test]
    fn cold_inference_closes_two_levels_and_self_recursion() {
        let mut program = Program::default();
        program.fns = vec![
            exceptional_caller("first"),
            err_function("first", &["second"]),
            err_function("second", &[]),
        ];
        infer(&mut program);
        assert!(!program.fns[0].cold);
        assert!(program.fns[1].cold);
        assert!(program.fns[2].cold);

        let mut recursive = Program::default();
        recursive.fns = vec![
            exceptional_caller("recursive"),
            err_function("recursive", &["recursive"]),
        ];
        infer(&mut recursive);
        assert!(recursive.fns[1].cold);
    }

    #[test]
    fn cold_inference_fails_closed_for_hot_exported_and_address_taken_functions() {
        let base = vec![exceptional_caller("error"), err_function("error", &[])];

        let mut mixed = Program::default();
        mixed.fns = base.clone();
        mixed.fns.push(err_function("hot", &["error"]));
        infer(&mut mixed);
        assert!(!mixed.fns[1].cold);

        let mut exported = Program::default();
        exported.fns = base.clone();
        exported.fns[1].exportable = true;
        infer(&mut exported);
        assert!(!exported.fns[1].cold);

        let mut addressed = Program::default();
        addressed.fns = base;
        addressed.fns.push(function(
            "addressor",
            vec![Block {
                id: 0,
                stmts: vec![Stmt::Let(
                    0,
                    Rvalue::FnAddr {
                        target: ProgramCall::from_validated("error"),
                        signature: Box::new(crate::FnSignatureFacts {
                            param_modes: Vec::new(),
                            return_borrow: align_sema::hir::ReturnBorrowSummary::None,
                            return_region: align_sema::hir::ReturnRegionSummary::None,
                            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
                        }),
                    },
                )],
                stmt_lines: Vec::new(),
                term: Term::Unreachable,
            }],
            vec![Ty::Fn(0)],
        ));
        infer(&mut addressed);
        assert!(!addressed.fns[1].cold);
    }
}
