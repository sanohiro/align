//! Resolve a copied view's backing at the descriptor load, including loop and call edges.
use super::*;

pub(super) fn parameter_type(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::Bitset | Ty::Slice(Scalar::Int(_) | Scalar::Float(_))
    )
}

fn call_arguments(
    value: &Rvalue,
) -> Option<(Option<&ProgramCall>, &[Operand], &[align_ast::ParamMode])> {
    match value {
        Rvalue::Call(DirectCall::Program(target), args) => Some((Some(target), args, &[])),
        Rvalue::CallWithCleanup(site) => Some((Some(&site.target), &site.args, &[])),
        Rvalue::CallIndirect {
            args, signature, ..
        }
        | Rvalue::RawCall {
            args, signature, ..
        } => Some((None, args, &signature.param_modes)),
        Rvalue::CallIndirectWithCleanup(site) => {
            Some((None, &site.args, &site.signature.param_modes))
        }
        _ => None,
    }
}

// A call can replace the descriptor itself only through a mutable place. Follow local
// forwarding to a fixed point; a separate unit supplies its certified no-rebind record.
fn preserves_header(
    program: &Program,
    target: Option<&ProgramCall>,
    index: usize,
    modes: &[align_ast::ParamMode],
) -> bool {
    use align_ast::ParamMode::{Borrow, ByValue};
    let Some(target) = target else {
        return matches!(modes.get(index), Some(Borrow | ByValue));
    };
    let mut pending = vec![(target.clone(), index)];
    let mut seen = HashSet::new();
    while let Some((name, index)) = pending.pop() {
        if !seen.insert((name.clone(), index)) {
            continue;
        }
        let Some(function) = program.fns.iter().find(|f| f.name == name) else {
            let Some(import) = program
                .imported_fns
                .iter()
                .find(|f| f.name == name && f.producer_certified)
            else {
                return false;
            };
            if matches!(import.param_modes.get(index), Some(Borrow | ByValue)) {
                continue;
            }
            if import
                .mutable_view_effect
                .as_ref()
                .and_then(|effects| effects.get(index))
                .and_then(Option::as_ref)
                .is_none_or(|effect| effect.may_rebind)
            {
                return false;
            }
            continue;
        };
        if matches!(function.param_modes.get(index), Some(Borrow | ByValue)) {
            continue;
        }
        let Some(slot) = function.params.get(index) else {
            return false;
        };
        for stmt in function.blocks.iter().flat_map(|block| &block.stmts) {
            match stmt {
                // The initial borrowed-parameter store aliases the incoming place in codegen.
                Stmt::Store(destination, Operand::Arg(arg))
                    if destination == slot && *arg as usize == index =>
                {
                    continue;
                }
                Stmt::Store(destination, _) | Stmt::StoreField(destination, _, _)
                    if destination == slot =>
                {
                    return false;
                }
                Stmt::Let(_, value) => {
                    if let Some((callee, args, modes)) = call_arguments(value) {
                        for (i, argument) in args.iter().enumerate() {
                            if matches!(argument, Operand::BorrowedPlace(place) if place.slot == *slot)
                            {
                                if let Some(target) = callee {
                                    pending.push((target.clone(), i));
                                } else if !matches!(modes.get(i), Some(Borrow | ByValue)) {
                                    return false;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    true
}

impl XmlAccessAnalyzer<'_> {
    pub(super) fn packed_slot_at(
        &mut self,
        slot: Slot,
        path: Vec<XmlAccessPathSegment>,
        at: ValueId,
    ) -> XmlAccessEquation {
        let invalid = || XmlAccessEquation {
            invalid: true,
            ..Default::default()
        };
        let mut equation = XmlAccessEquation::default();
        let function = self.graph.function;
        let Some(root) = function.slots.get(slot as usize).copied() else {
            return invalid();
        };
        let Some(selected) = xml_selected_ty(self.graph.program, root, &path) else {
            return invalid();
        };
        let mut predecessors = vec![Vec::new(); function.blocks.len()];
        let mut start = None;
        for (i, block) in function.blocks.iter().enumerate() {
            if block.id as usize != i {
                return invalid();
            }
            for next in crate::byte_ranges::successors(&block.term) {
                let Some(pred) = predecessors.get_mut(next as usize) else {
                    return invalid();
                };
                pred.push(i);
            }
            for (n, stmt) in block.stmts.iter().enumerate() {
                if matches!(stmt, Stmt::Let(id, _) if *id == at) && start.replace((i, n)).is_some()
                {
                    return invalid();
                }
            }
        }
        let Some(start) = start else {
            return invalid();
        };
        let mut pending = vec![start];
        let mut seen = HashSet::new();
        while let Some((i, before)) = pending.pop() {
            if !seen.insert((i, before)) {
                continue;
            }
            let block = &function.blocks[i];
            let mut found = false;
            for stmt in block.stmts[..before].iter().rev() {
                let (operand, tail) = match stmt {
                    Stmt::Store(dest, Operand::Arg(arg))
                        if *dest == slot
                            && matches!(
                                function.param_modes.get(*arg as usize),
                                Some(
                                    align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut
                                )
                            ) =>
                    {
                        if function.params.get(*arg as usize) != Some(&slot) {
                            return invalid();
                        }
                        continue;
                    }
                    Stmt::Store(dest, op) if *dest == slot => (op, &path[..]),
                    Stmt::StoreField(dest, fields, op) if *dest == slot => {
                        let prefix: Vec<_> = fields
                            .iter()
                            .copied()
                            .map(XmlAccessPathSegment::StructField)
                            .collect();
                        let Some(tail) = path.strip_prefix(prefix.as_slice()) else {
                            continue;
                        };
                        (op, tail)
                    }
                    Stmt::Let(_, value) => {
                        if let Some((name, args, modes)) = call_arguments(value) {
                            for (arg, op) in args.iter().enumerate() {
                                let Operand::BorrowedPlace(place) = op else {
                                    continue;
                                };
                                if place.slot != slot {
                                    continue;
                                }
                                let Some((_, prefix)) =
                                    xml_borrowed_path(self.graph.program, root, &place.path)
                                else {
                                    return invalid();
                                };
                                if path.starts_with(&prefix)
                                    && !preserves_header(self.graph.program, name, arg, modes)
                                {
                                    return invalid();
                                }
                            }
                        }
                        continue;
                    }
                    _ => continue,
                };
                if xml_operand_base_ty(function, operand)
                    .and_then(|ty| xml_selected_ty(self.graph.program, ty, tail))
                    != Some(selected)
                {
                    return invalid();
                }
                let source = self.buffer_source(operand, tail.to_vec());
                Self::add_source(&mut equation, source);
                found = true;
                break;
            }
            if found {
                continue;
            }
            if block.id == function.entry {
                let Ok(Some(arg)) = self.buffer_parameter(slot) else {
                    return invalid();
                };
                if !matches!(
                    function.param_modes.get(arg),
                    Some(align_ast::ParamMode::Borrow | align_ast::ParamMode::BorrowMut)
                ) {
                    return invalid();
                }
                let Ok(arg) = u32::try_from(arg) else {
                    return invalid();
                };
                let source = self.buffer_source(&Operand::Arg(arg), path.clone());
                Self::add_source(&mut equation, source);
            }
            for &pred in &predecessors[i] {
                pending.push((pred, function.blocks[pred].stmts.len()));
            }
        }
        equation
    }
}
