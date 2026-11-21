//! Resolve a copied view's backing at the descriptor load, including loop and call edges.
use super::*;

pub(super) fn parameter_type(ty: Ty) -> bool {
    matches!(
        ty,
        Ty::Bitset | Ty::Slice(Scalar::Int(_) | Scalar::Float(_))
    )
}

pub(super) fn read_type(ty: Ty) -> bool {
    parameter_type(ty) || matches!(ty, Ty::Bool | Ty::Int(_) | Ty::Str)
}

pub(super) fn native_value(function: &Function, operand: &Operand, ty: Ty) -> bool {
    direct_value_form(function, operand)
        || matches!(operand, Operand::Arg(i)
        if matches!(ty, Ty::Slice(_)) && function.param_modes.get(*i as usize) == Some(&align_ast::ParamMode::Out))
}

fn call_arguments(value: &Rvalue) -> Option<&[Operand]> {
    match value {
        Rvalue::Call(DirectCall::Program(_), args)
        | Rvalue::CallIndirect { args, .. }
        | Rvalue::RawCall { args, .. } => Some(args),
        Rvalue::CallWithCleanup(site) => Some(&site.args),
        Rvalue::CallIndirectWithCleanup(site) => Some(&site.args),
        _ => None,
    }
}

// Keep every possible function address. SSA joins and local copies can select a named
// function without making its descriptor effects unknown.
fn call_effect_targets(
    function: &Function,
    call: &Rvalue,
    index: usize,
) -> Option<Vec<ProgramCall>> {
    use align_ast::ParamMode::{Borrow, ByValue};
    let (callee, modes) = match call {
        Rvalue::Call(DirectCall::Program(target), _) => return Some(vec![target.clone()]),
        Rvalue::CallWithCleanup(site) => return Some(vec![site.target.clone()]),
        Rvalue::CallIndirect {
            callee, signature, ..
        } => (callee, &signature.param_modes),
        Rvalue::CallIndirectWithCleanup(site) => (&site.callee, &site.signature.param_modes),
        Rvalue::RawCall { signature, .. }
            if matches!(signature.param_modes.get(index), Some(Borrow | ByValue)) =>
        {
            return Some(Vec::new());
        }
        _ => return None,
    };
    if matches!(modes.get(index), Some(Borrow | ByValue)) {
        return Some(Vec::new());
    }
    let mut pending = vec![callee];
    let mut values = HashSet::new();
    let mut slots = HashSet::new();
    let mut targets = BTreeSet::new();
    while let Some(operand) = pending.pop() {
        let Operand::Value(id) = operand else {
            return None;
        };
        if !values.insert(*id) {
            continue;
        }
        let mut definitions = function
            .blocks
            .iter()
            .flat_map(|b| &b.stmts)
            .filter_map(|stmt| match stmt {
                Stmt::Let(value, rv) if value == id => Some(rv),
                _ => None,
            });
        match definitions.next()? {
            Rvalue::FnAddr { target, .. } | Rvalue::Closure { lifted: target, .. } => {
                targets.insert(target.clone());
            }
            Rvalue::Use(child) => pending.push(child),
            Rvalue::Select { a, b, .. } => pending.extend([a, b]),
            Rvalue::Load(slot) if !function.params.contains(slot) => {
                if slots.insert(*slot) {
                    for stmt in function.blocks.iter().flat_map(|b| &b.stmts) {
                        match stmt {
                            Stmt::Store(dest, value) if dest == slot => pending.push(value),
                            Stmt::Let(_, rv) if call_arguments(rv).is_some_and(|args| args.iter()
                                .any(|op| matches!(op, Operand::BorrowedPlace(p) if p.slot == *slot))) => return None,
                            _ => {}
                        }
                    }
                }
            }
            _ => return None,
        }
        if definitions.next().is_some() {
            return None;
        }
    }
    (!targets.is_empty()).then(|| targets.into_iter().collect())
}

// A call can replace the descriptor itself only through a mutable place. Follow local
// forwarding to a fixed point; a separate unit supplies its certified no-rebind record.
fn preserves_header(program: &Program, caller: &Function, call: &Rvalue, index: usize) -> bool {
    use align_ast::ParamMode::{Borrow, ByValue};
    let Some(targets) = call_effect_targets(caller, call, index) else {
        return false;
    };
    let mut pending: Vec<_> = targets.into_iter().map(|target| (target, index)).collect();
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
                    if let Some(args) = call_arguments(value) {
                        for (i, argument) in args.iter().enumerate() {
                            if matches!(argument, Operand::BorrowedPlace(place) if place.slot == *slot)
                            {
                                let Some(targets) = call_effect_targets(function, value, i) else {
                                    return false;
                                };
                                pending.extend(targets.into_iter().map(|target| (target, i)));
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
        writable: bool,
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
                    Stmt::Let(id, _)
                        if !writable
                            && self
                                .graph
                                .slot_stores
                                .producers
                                .get(slot as usize)
                                .is_some_and(|producers| {
                                    producers.iter().any(|(producer, _)| producer == id)
                                }) =>
                    {
                        let source = self.queue(XmlAccessNode::Slot(slot, path.clone()));
                        Self::add_source(&mut equation, source);
                        found = true;
                        break;
                    }
                    Stmt::Let(_, value) if writable => {
                        if let Some(args) = call_arguments(value) {
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
                                    && !preserves_header(self.graph.program, function, value, arg)
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
                if !native_value(
                    function,
                    operand,
                    xml_operand_base_ty(function, operand).unwrap_or(Ty::Error),
                ) {
                    return invalid();
                }
                let source = if writable {
                    self.buffer_source(operand, tail.to_vec())
                } else {
                    self.read_source(&mut equation, operand, selected, tail.to_vec())
                };
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
                let source = if writable {
                    self.buffer_source(&Operand::Arg(arg), path.clone())
                } else {
                    self.source(&Operand::Arg(arg), selected, path.clone())
                };
                Self::add_source(&mut equation, source);
            }
            for &pred in &predecessors[i] {
                pending.push((pred, function.blocks[pred].stmts.len()));
            }
        }
        equation
    }
}
