//! Check program-point availability of operands and of SoA descriptor carriers before
//! validating projections and ownership in the producer graph.
use super::*;
use crate::operand_inputs::OperandInputs;
use crate::producer::{ProducerError, xml_operand_base_ty};
use std::collections::{BTreeSet, HashMap, VecDeque};

type Places = BTreeSet<(Slot, Vec<u32>)>;

#[derive(Clone, Default)]
struct Initialized {
    places: Places,
    values: BTreeSet<ValueId>,
}

fn covers(places: &Places, slot: Slot, path: &[u32]) -> bool {
    (0..=path.len()).any(|n| places.contains(&(slot, path[..n].to_vec())))
}

fn children(program: &Program, ty: Ty, index: u32) -> Option<(Ty, usize)> {
    match ty {
        Ty::Struct(id) => {
            let fields = &program.structs.get(id as usize)?.fields;
            Some((fields.get(index as usize)?.ty, fields.len()))
        }
        Ty::StructArray(row, len) if index < len => Some((Ty::Struct(row), len as usize)),
        Ty::Array(element, len) if index < len => {
            Some((align_sema::scalar_to_ty(element), len as usize))
        }
        _ => None,
    }
}

// A complete store covers its descendants. Adjacent partial stores become a complete
// parent only once every immediate child is ready, including every fixed-array element.
fn initialize(
    program: &Program,
    function: &Function,
    places: &mut Places,
    slot: Slot,
    path: &[u32],
) {
    if covers(places, slot, path) {
        return;
    }
    let Some(mut ty) = function.slots.get(slot as usize).copied() else {
        return;
    };
    let mut widths = Vec::new();
    for &index in path {
        let Some((child, width)) = children(program, ty, index) else {
            return;
        };
        widths.push(width);
        ty = child;
    }
    for depth in (0..=path.len()).rev() {
        let current = &path[..depth];
        places.retain(|(s, p)| *s != slot || !p.starts_with(current));
        places.insert((slot, current.to_vec()));
        if depth == 0 {
            break;
        }
        let parent = &current[..depth - 1];
        if places
            .iter()
            .filter(|(s, p)| *s == slot && p.len() == depth && p.starts_with(parent))
            .count()
            != widths[depth - 1]
        {
            break;
        }
    }
}

fn merge(previous: &mut Initialized, incoming: &Initialized) -> bool {
    let common = previous
        .places
        .union(&incoming.places)
        .filter(|(slot, path)| {
            covers(&previous.places, *slot, path) && covers(&incoming.places, *slot, path)
        })
        .cloned()
        .collect();
    let old = previous.values.len();
    previous.values.retain(|v| incoming.values.contains(v));
    let changed = previous.places != common || old != previous.values.len();
    previous.places = common;
    changed
}

fn constant_index(index: &Operand) -> Option<u32> {
    match index {
        Operand::Const(Const::Int(i, _)) => u32::try_from(*i).ok(),
        _ => None,
    }
}

pub(super) fn validate(program: &Program, function: &Function) -> Result<(), ProducerError> {
    let mut uses = BTreeSet::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            if let Stmt::Let(v, Rvalue::GroupAggMultiCols { .. }) = stmt {
                uses.insert(*v);
            }
        }
    }
    if uses.is_empty() {
        return Ok(());
    }
    let fail = || {
        ProducerError::Lowering(format!(
            "invalid SoA group source or non-dominating operand in '{}'",
            function.name
        ))
    };
    let mut type_cache = HashMap::new();
    let mut carries_soa = |ty| {
        *type_cache.entry(ty).or_insert_with(|| {
            align_sema::ty_contains_leaf(
                ty,
                &program.structs,
                &program.tuples,
                &program.enums,
                &program.tagged_types,
                |t| matches!(t, Ty::Soa(_)),
            )
        })
    };
    let carriers: BTreeSet<_> = function
        .slots
        .iter()
        .enumerate()
        .filter(|(_, ty)| carries_soa(**ty))
        .filter_map(|(i, _)| Slot::try_from(i).ok())
        .collect();
    let mut definitions = BTreeSet::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            if let Stmt::Let(value, rv) = stmt {
                for id in std::iter::once(*value).chain(rv.auxiliary_result()) {
                    if function.value_tys.get(id as usize).is_none() || !definitions.insert(id) {
                        return Err(fail());
                    }
                }
            }
        }
    }
    let mut entries = vec![None; function.blocks.len()];
    *entries.get_mut(function.entry as usize).ok_or_else(&fail)? = Some(Initialized::default());
    let mut pending = VecDeque::from([function.entry]);
    while let Some(id) = pending.pop_front() {
        let block = function
            .blocks
            .get(id as usize)
            .filter(|b| b.id == id)
            .ok_or_else(&fail)?;
        let mut state = entries[id as usize].clone().ok_or_else(&fail)?;
        for stmt in &block.stmts {
            if !stmt.operands_satisfy(&mut |op| input_ready(function, &carriers, &state, op)) {
                return Err(fail());
            }
            match stmt {
                Stmt::Store(slot, op) if carriers.contains(slot) => {
                    if xml_operand_base_ty(function, op)
                        != function.slots.get(*slot as usize).copied()
                    {
                        return Err(fail());
                    }
                    initialize(program, function, &mut state.places, *slot, &[]);
                }
                Stmt::StoreField(slot, fields, _) if carriers.contains(slot) => {
                    initialize(program, function, &mut state.places, *slot, fields)
                }
                Stmt::StoreIndex(slot, index, _) | Stmt::StoreElemField(slot, index, _, _)
                    if carriers.contains(slot) =>
                {
                    if let Some(i) = constant_index(index) {
                        let mut path = vec![i];
                        if let Stmt::StoreElemField(_, _, fields, _) = stmt {
                            path.extend(fields);
                        }
                        initialize(program, function, &mut state.places, *slot, &path);
                    }
                }
                Stmt::Drop(slot)
                | Stmt::DropFlagMoveOut { slot, .. }
                | Stmt::DropFlagInit(slot) => {
                    state.places.retain(|(s, _)| s != slot);
                }
                Stmt::Let(v, rv) => {
                    let result = function.value_tys[*v as usize];
                    if let Rvalue::GroupAggMultiCols { base, .. } = rv {
                        if !covers(&state.places, *base, &[]) {
                            return Err(fail());
                        }
                        uses.remove(v);
                    }
                    if carries_soa(result) {
                        let location = match rv {
                            Rvalue::Load(slot) | Rvalue::MakeSlice(slot, _) => {
                                Some((*slot, Vec::new()))
                            }
                            Rvalue::Field(slot, path) | Rvalue::MakeFieldSlice(slot, path, _) => {
                                Some((*slot, path.clone()))
                            }
                            Rvalue::Index(slot, index) | Rvalue::IndexField(slot, index, _) => {
                                let mut path = Vec::new();
                                if let Some(i) = constant_index(index) {
                                    path.push(i);
                                    if let Rvalue::IndexField(_, _, fields) = rv {
                                        path.extend(fields);
                                    }
                                }
                                Some((*slot, path))
                            }
                            _ => None,
                        };
                        if location.is_some_and(|(slot, path)| !covers(&state.places, slot, &path))
                        {
                            return Err(fail());
                        }
                    }
                    // The JSON lowering zeroes its out slot. The CSV runtime publishes an
                    // empty descriptor before parsing its validated input.
                    if let Rvalue::JsonDecodeSoa { out, .. } | Rvalue::CsvDecode { out, .. } = rv {
                        initialize(program, function, &mut state.places, *out, &[]);
                    }
                    state.values.insert(*v);
                    state.values.extend(rv.auxiliary_result());
                }
                _ => {}
            }
        }
        if !block
            .term
            .operands_satisfy(&mut |op| input_ready(function, &carriers, &state, op))
        {
            return Err(fail());
        }
        for next in crate::byte_ranges::successors(&block.term) {
            let entry = entries.get_mut(next as usize).ok_or_else(&fail)?;
            if let Some(previous) = entry {
                if !merge(previous, &state) {
                    continue;
                }
            } else {
                *entry = Some(state.clone());
            }
            pending.push_back(next);
        }
    }
    if uses.is_empty() { Ok(()) } else { Err(fail()) }
}

fn input_ready(
    function: &Function,
    carriers: &BTreeSet<Slot>,
    state: &Initialized,
    op: &Operand,
) -> bool {
    match op {
        Operand::Value(v) => state.values.contains(v),
        Operand::Arg(i) => {
            let Some(ty) = xml_operand_base_ty(function, op) else {
                return false;
            };
            function.param_modes.get(*i as usize).is_some_and(|mode| {
                *mode != align_ast::ParamMode::Out || matches!(ty, Ty::Slice(_))
            })
        }
        Operand::BorrowedCleanupArg(i) => function
            .borrow_mut_cleanup_slots
            .get(*i as usize)
            .is_some_and(Option::is_some),
        Operand::BorrowedPlace(place) if carriers.contains(&place.slot) => {
            let mut path = Vec::new();
            for segment in &place.path {
                match segment {
                    hir::BorrowedPathSegment::RootSlot => {}
                    hir::BorrowedPathSegment::StructField(field) => path.push(*field),
                    _ => break,
                }
            }
            covers(&state.places, place.slot, &path)
        }
        Operand::BorrowedElementPlace(place) => {
            !carriers.contains(&place.base.slot) || covers(&state.places, place.base.slot, &[])
        }
        Operand::BorrowedFixedElementPlace(place) => {
            let mut path = vec![place.index];
            path.extend(&place.path);
            !carriers.contains(&place.base) || covers(&state.places, place.base, &path)
        }
        Operand::Const(_) | Operand::BorrowedPlace(_) => true,
    }
}
