//! Program-point lifetimes for descriptors pointing into an arena or a borrowed batch.
//! The state is sparse: ordinary scalar locals, SSA values and caller-owned views add no entries.
use super::*;
use crate::operand_inputs::OperandInputs;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Origin {
    Arena(ValueId),
    Columns { allocation: ValueId, arena: ValueId },
    Owner(Slot, Vec<u32>),
    // A moved owner remains usable at its consuming call. Borrowing it converts its
    // release place into a dependency before move/drop/replacement can retire it.
    ResourcePlace(Slot, Vec<u32>),
    MovedResource,
    Expired,
}

// Projection coordinates are relative to a value. The empty path covers the whole value.
// A tagged payload uses its physical variant and payload ordinal at two successive levels.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Origins(BTreeSet<(Vec<u32>, Origin)>);

impl Origins {
    fn root(origin: Origin) -> Self {
        Self(BTreeSet::from([(vec![], origin)]))
    }

    fn extend(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    fn project(&self, path: &[u32]) -> Self {
        Self(
            self.0
                .iter()
                .filter_map(|(p, origin)| {
                    if let Some(tail) = p.strip_prefix(path) {
                        Some((tail.to_vec(), origin.clone()))
                    } else if path.starts_with(p) {
                        Some((vec![], origin.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }

    fn under(self, path: &[u32]) -> Self {
        Self(
            self.0
                .into_iter()
                .map(|(p, o)| ([path, &p].concat(), o))
                .collect(),
        )
    }

    fn flat(self) -> Self {
        Self(self.0.into_iter().map(|(_, o)| (vec![], o)).collect())
    }

    fn stored(mut self) -> Self {
        // Once an owning value reaches its destination, a later borrow binds that destination.
        self.0
            .retain(|(_, o)| !matches!(o, Origin::ResourcePlace(..) | Origin::MovedResource));
        self
    }

    fn borrowed(self) -> Self {
        Self(
            self.0
                .into_iter()
                .map(|(p, o)| {
                    (
                        p,
                        match o {
                            Origin::ResourcePlace(slot, path) => Origin::Owner(slot, path),
                            Origin::MovedResource => Origin::Expired,
                            other => other,
                        },
                    )
                })
                .collect(),
        )
    }

    fn live(&self) -> bool {
        self.0.iter().all(|(_, o)| *o != Origin::Expired)
    }
}

#[derive(Clone, Default, Eq, PartialEq)]
pub(super) struct Backing {
    arenas: Vec<ValueId>,
    slots: BTreeMap<Slot, Origins>,
    values: BTreeMap<ValueId, Origins>,
    // Text cells hold independent byte views. The column buffer itself owns only their headers.
    text_fields: BTreeMap<(ValueId, ValueId, u32), Origins>,
}

fn record(map: &mut BTreeMap<u32, Origins>, id: u32, origins: Origins) {
    if origins.0.is_empty() {
        map.remove(&id);
    } else {
        map.insert(id, origins);
    }
}

impl Backing {
    pub(super) fn at_entry(function: &Function) -> Self {
        let mut state = Self::default();
        for (i, ty) in function.slots.iter().enumerate() {
            if matches!(ty, Ty::Resource(_)) {
                if let Ok(slot) = u32::try_from(i) {
                    state
                        .slots
                        .insert(slot, Origins::root(Origin::MovedResource));
                }
            }
        }
        state
    }

    pub(super) fn merge(&mut self, incoming: &Self) -> Option<bool> {
        if self.arenas != incoming.arenas {
            return None;
        }
        let mut changed = false;
        for (map, other) in [
            (&mut self.slots, &incoming.slots),
            (&mut self.values, &incoming.values),
        ] {
            for (id, facts) in other {
                let current = map.entry(*id).or_default();
                let n = current.0.len();
                current.0.extend(facts.0.iter().cloned());
                changed |= current.0.len() != n;
            }
        }
        for (key, facts) in &incoming.text_fields {
            let current = self.text_fields.entry(*key).or_default();
            let n = current.0.len();
            current.0.extend(facts.0.iter().cloned());
            changed |= current.0.len() != n;
        }
        Some(changed)
    }

    fn expire(&mut self, killed: impl Fn(&Origin) -> bool) {
        for facts in self
            .slots
            .values_mut()
            .chain(self.values.values_mut())
            .chain(self.text_fields.values_mut())
        {
            if facts.0.iter().any(|(_, origin)| killed(origin)) {
                facts.0 = std::mem::take(&mut facts.0)
                    .into_iter()
                    .map(|(p, o)| {
                        if killed(&o) {
                            (
                                p,
                                if matches!(o, Origin::ResourcePlace(..)) {
                                    Origin::MovedResource
                                } else {
                                    Origin::Expired
                                },
                            )
                        } else {
                            (p, o)
                        }
                    })
                    .collect();
            }
        }
    }

    fn write(&mut self, slot: Slot, path: &[u32], origins: Origins) {
        self.expire(|o| matches!(o, Origin::Owner(s, p) | Origin::ResourcePlace(s,p) if *s == slot && (p.starts_with(path) || path.starts_with(p))));
        let mut facts = self.slots.remove(&slot).unwrap_or_default();
        facts.0.retain(|(p, _)| !p.starts_with(path));
        facts.extend(origins.under(path));
        record(&mut self.slots, slot, facts);
    }

    fn read(&self, slot: Slot, path: &[u32]) -> Origins {
        self.slots
            .get(&slot)
            .map_or_else(Origins::default, |o| o.project(path))
    }

    fn op(&self, op: &Operand) -> Origins {
        match op {
            Operand::Value(v) => self.values.get(v).cloned().unwrap_or_default(),
            Operand::BorrowedPlace(p) => {
                let path = borrowed_path(&p.path);
                let mut o = self.read(p.slot, &path);
                if matches!(p.ty, Ty::Resource(_)) {
                    o.extend(Origins::root(Origin::Owner(p.slot, path)));
                }
                o
            }
            Operand::BorrowedFixedElementPlace(p) => {
                let path = [&[p.index][..], &p.path].concat();
                let mut o = self.read(p.base, &path);
                if matches!(p.ty, Ty::Resource(_)) {
                    o.extend(Origins::root(Origin::Owner(p.base, path)));
                }
                o
            }
            Operand::BorrowedElementPlace(p) => {
                self.read(p.base.slot, &borrowed_path(&p.base.path)).flat()
            }
            Operand::Const(_) | Operand::Arg(_) | Operand::BorrowedCleanupArg(_) => {
                Origins::default()
            }
        }
    }

    fn text(&self, source: &Origins, field: u32) -> Origins {
        let mut result = Origins::default();
        for (_, origin) in &source.0 {
            match origin {
                Origin::Columns { allocation, arena } => {
                    if let Some(stored) = self.text_fields.get(&(*arena, *allocation, field)) {
                        result.extend(stored.clone());
                    }
                }
                // Native decoded fields can point either into the source or into the decoder's arena.
                // A DB batch likewise owns the native bytes as well as the column headers.
                o => result.extend(Origins::root(o.clone())),
            }
        }
        result
    }

    fn local_arena(&self, handle: &Operand) -> Option<Origins> {
        let Operand::Value(v) = handle else {
            return None;
        };
        self.arenas
            .contains(v)
            .then(|| Origins::root(Origin::Arena(*v)))
    }

    fn call(
        &self,
        args: &[Operand],
        callee: Option<&Operand>,
        borrow: &hir::ReturnBorrowSummary,
        region: &hir::ReturnRegionSummary,
    ) -> Option<Origins> {
        let mut out = Origins::default();
        let mut add = |params: &[u32], captures: &[u32]| -> Option<()> {
            for &i in params {
                out.extend(self.op(args.get(i as usize)?).borrowed().flat());
            }
            for &i in captures {
                out.extend(self.op(callee?).project(&[i]).borrowed().flat());
            }
            Some(())
        };
        if let hir::ReturnBorrowSummary::Roots { params, captures } = borrow {
            add(params, captures)?;
        }
        if let hir::ReturnRegionSummary::Roots { params, captures } = region {
            add(params, captures)?;
        }
        Some(out)
    }

    fn direct(&self, program: &Program, target: &ProgramCall, args: &[Operand]) -> Option<Origins> {
        let (b, r) = program
            .fns
            .iter()
            .find(|f| f.name == *target)
            .map(|f| (&f.return_borrow, &f.return_region))
            .or_else(|| {
                program
                    .imported_fns
                    .iter()
                    .find(|f| f.name == *target)
                    .map(|f| (&f.return_borrow, &f.return_region))
            })
            .or_else(|| {
                program
                    .externs
                    .iter()
                    .find(|f| f.name == *target)
                    .map(|f| (&f.return_borrow, &f.return_region))
            })?;
        self.call(args, None, b, r)
    }

    pub(super) fn valid_terminator(&self, term: &Term) -> bool {
        let returns = matches!(term, Term::Return(_) | Term::ReturnWithCleanup(_));
        (!returns || self.arenas.is_empty()) && term.operands_satisfy(&mut |op| self.op(op).live())
    }

    pub(super) fn step(
        &mut self,
        program: &Program,
        f: &Function,
        stmt: &Stmt,
        read: Option<(Slot, Vec<u32>)>,
    ) -> Option<()> {
        match stmt {
            Stmt::Let(v, Rvalue::ArenaBegin) => {
                if self.arenas.contains(v) {
                    return None;
                }
                self.arenas.push(*v);
            }
            Stmt::ArenaEnd(Operand::Value(v)) if self.arenas.last() == Some(v) => {
                self.arenas.pop();
                self.expire(|o| {
                    *o == Origin::Arena(*v) || matches!(o,Origin::Columns {arena,..} if arena == v)
                });
                self.text_fields.retain(|(arena, _, _), _| arena != v);
            }
            Stmt::ArenaEnd(_) => return None,
            Stmt::StoreColumn {
                base,
                struct_id,
                field,
                value,
                ..
            } => {
                if !stmt.operands_satisfy(&mut |op| self.op(op).live()) {
                    return None;
                }
                if program
                    .structs
                    .get(*struct_id as usize)?
                    .fields
                    .get(*field as usize)?
                    .ty
                    == Ty::Str
                {
                    let contents = self.op(value).flat();
                    for (_, origin) in self.op(base).0 {
                        if let Origin::Columns { allocation, arena } = origin {
                            self.text_fields
                                .entry((arena, allocation, *field))
                                .or_default()
                                .extend(contents.clone());
                        }
                    }
                }
            }
            Stmt::Store(slot, op) => self.write(*slot, &[], self.op(op).stored()),
            Stmt::StoreField(slot, path, op) => self.write(*slot, path, self.op(op).stored()),
            Stmt::StoreIndex(slot, index, op) | Stmt::StoreElemField(slot, index, _, op) => {
                let origins = self.op(op).stored();
                if let Some(i) = super::group_source::constant_index(index) {
                    let mut path = vec![i];
                    if let Stmt::StoreElemField(_, _, fields, _) = stmt {
                        path.extend(fields);
                    }
                    self.write(*slot, &path, origins);
                } else {
                    self.expire(|o| matches!(o, Origin::Owner(s,_) | Origin::ResourcePlace(s,_) if s == slot));
                    if !origins.0.is_empty() {
                        self.slots.entry(*slot).or_default().extend(origins.flat());
                    }
                }
            }
            Stmt::Drop(slot) | Stmt::DropFlagMoveOut { slot, .. } | Stmt::DropFlagInit(slot) => {
                // A removed resource cannot originate fresh references by being loaded again.
                // The SSA value in transit to an owning destination is still usable.
                let retired = if matches!(f.slots.get(*slot as usize), Some(Ty::Resource(_))) {
                    Origins::root(Origin::MovedResource)
                } else {
                    Origins::default()
                };
                self.write(*slot, &[], retired)
            }
            Stmt::DropField(slot, p) => self.write(*slot, p, Origins::default()),
            Stmt::DropElem(slot, index, _)
            | Stmt::DropElemField(slot, index, _)
            | Stmt::NullElemField(slot, index, _) => {
                let mut path = Vec::new();
                if let Some(i) = super::group_source::constant_index(index) {
                    path.push(i);
                    if let Stmt::DropElemField(_, _, fields) | Stmt::NullElemField(_, _, fields) =
                        stmt
                    {
                        path.extend(fields);
                    }
                }
                self.expire(|o| {
                    matches!(o, Origin::Owner(s,p) | Origin::ResourcePlace(s,p)
                    if *s == *slot && (p.starts_with(&path) || path.starts_with(p)))
                });
            }
            Stmt::NullTupleField(slot, i) | Stmt::NullStructField(slot, i) => {
                self.write(*slot, &[*i], Origins::default())
            }
            Stmt::Let(v, rv) => {
                let ty = *f.value_tys.get(*v as usize)?;
                let place = read
                    .as_ref()
                    .map(|(s, p)| self.read(*s, p))
                    .unwrap_or_default();
                let mut inputs = Origins::default();
                rv.operands_satisfy(&mut |op| {
                    inputs.extend(self.op(op));
                    true
                });
                let mut result = match rv {
                    Rvalue::JsonDecodeSoa { arena, out, .. }
                    | Rvalue::CsvDecode { arena, out, .. } => {
                        let mut storage = self.local_arena(arena)?;
                        storage.extend(inputs.clone().flat());
                        self.write(*out, &[], storage);
                        Origins::default()
                    }
                    Rvalue::SoaAlloc {
                        handle: Operand::Value(arena),
                        ..
                    } if self.arenas.contains(arena) => Origins::root(Origin::Columns {
                        allocation: *v,
                        arena: *arena,
                    }),
                    Rvalue::SoaAlloc { .. } => return None,
                    Rvalue::SoaColumn { field, .. } => {
                        let mut column = place.clone().flat();
                        if ty == Ty::Slice(Scalar::Str) {
                            column.extend(self.text(&place, *field).under(&[0]));
                        }
                        column
                    }
                    Rvalue::IndexColumn { base, field, .. } if ty == Ty::Str => {
                        self.text(&self.op(base), *field)
                    }
                    Rvalue::SliceIndex(slice, _) | Rvalue::SliceIndexNoalias { slice, .. }
                        if ty == Ty::Str =>
                    {
                        let mut elements = self.op(slice);
                        elements
                            .0
                            .retain(|(p, o)| !p.is_empty() || !matches!(o, Origin::Columns { .. }));
                        elements.project(&[0])
                    }
                    Rvalue::SubSlice { base, .. } | Rvalue::SlicePtr(base) => self.op(base),
                    Rvalue::Load(_)
                    | Rvalue::Field(..)
                    | Rvalue::Index(..)
                    | Rvalue::IndexField(..)
                    | Rvalue::MakeSlice(..)
                    | Rvalue::MakeFieldSlice(..) => place,
                    Rvalue::MakeTuple { elems, .. }
                    | Rvalue::Closure {
                        captures: elems, ..
                    } => {
                        let mut o = Origins::default();
                        for (i, e) in elems.iter().enumerate() {
                            o.extend(self.op(e).under(&[u32::try_from(i).ok()?]));
                        }
                        o
                    }
                    Rvalue::TupleIndex { tuple, index } => self.op(tuple).project(&[*index]),
                    Rvalue::MakeEnum {
                        variant, payload, ..
                    } => {
                        let mut o = Origins::default();
                        for (i, e) in payload.iter().enumerate() {
                            o.extend(self.op(e).under(&[*variant, u32::try_from(i).ok()?]));
                        }
                        o
                    }
                    Rvalue::EnumPayload {
                        operand,
                        variant,
                        slot,
                        ..
                    } => self.op(operand).project(&[*variant, *slot]),
                    Rvalue::ResultOk(o) | Rvalue::OptionSome(o) => self.op(o).under(&[0]),
                    Rvalue::ResultErr(o) => self.op(o).under(&[1]),
                    Rvalue::ResultUnwrapOk(o) | Rvalue::OptionUnwrap(o) => self.op(o).project(&[0]),
                    Rvalue::ResultUnwrapErr(o) => self.op(o).project(&[1]),
                    Rvalue::Use(op) => self.op(op),
                    Rvalue::ResourceBorrow { owner, .. } => self.op(owner).borrowed(),
                    Rvalue::Select { a, b, .. } => {
                        let mut o = self.op(a);
                        o.extend(self.op(b));
                        o
                    }
                    Rvalue::Call(DirectCall::Program(t), args) => self.direct(program, t, args)?,
                    Rvalue::CallWithCleanup(c) => self.direct(program, &c.target, &c.args)?,
                    Rvalue::CallIndirect {
                        callee,
                        args,
                        signature,
                        ..
                    }
                    | Rvalue::RawCall {
                        callee,
                        args,
                        signature,
                        ..
                    } => self.call(
                        args,
                        Some(callee),
                        &signature.return_borrow,
                        &signature.return_region,
                    )?,
                    Rvalue::CallIndirectWithCleanup(c) => self.call(
                        &c.args,
                        Some(&c.callee),
                        &c.signature.return_borrow,
                        &c.signature.return_region,
                    )?,
                    Rvalue::GroupAggMultiCols {
                        struct_id,
                        key_field,
                        ..
                    } => {
                        if !place.live() {
                            return None;
                        }
                        if program
                            .structs
                            .get(*struct_id as usize)?
                            .fields
                            .get(*key_field as usize)?
                            .ty
                            == Ty::Str
                        {
                            let text = self.text(&place, *key_field);
                            if !text.live() {
                                return None;
                            }
                            text.under(&[0, 0])
                        } else {
                            Origins::default()
                        }
                    }
                    // Only data-carrying outputs retain an input backing. Numeric/header observations
                    // use the input here and remain independent after they have copied the result.
                    _ => inputs.clone().flat(),
                };
                // All explicit and implicit accesses observe live backing at the native read.
                // Aggregate projection checks the chosen component, preserving unrelated siblings.
                let observed = match rv {
                    Rvalue::Load(_)
                        if matches!(
                            ty,
                            Ty::Tuple(_)
                                | Ty::Struct(_)
                                | Ty::Enum(_)
                                | Ty::Option(_)
                                | Ty::Result(..)
                                | Ty::Tagged(_)
                        ) =>
                    {
                        Origins::default()
                    }
                    Rvalue::SliceLen(slice) => {
                        let mut header = self.op(slice);
                        header.0.retain(|(p, _)| p.is_empty());
                        header
                    }
                    Rvalue::Field(..)
                    | Rvalue::IndexField(..)
                    | Rvalue::TupleIndex { .. }
                    | Rvalue::EnumPayload { .. }
                    | Rvalue::ResultUnwrapOk(_)
                    | Rvalue::ResultUnwrapErr(_)
                    | Rvalue::OptionUnwrap(_)
                    | Rvalue::ResourceBorrow { .. } => result.clone(),
                    _ => {
                        let mut all = inputs;
                        if let Some((s, p)) = &read {
                            all.extend(self.read(*s, p));
                        }
                        all
                    }
                };
                if !observed.live() {
                    return None;
                }
                if matches!(ty, Ty::Resource(_)) {
                    if let Some((slot, path)) = read {
                        result
                            .0
                            .retain(|(_, o)| !matches!(o, Origin::ResourcePlace(..)));
                        result.extend(Origins::root(Origin::ResourcePlace(slot, path)));
                    }
                } else if matches!(
                    ty,
                    Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Unit
                ) {
                    result = Origins::default();
                }
                // A finite, well-typed aggregate can contribute at most two coordinates per layout
                // or constructor level. Reject cyclic forged MIR before it can grow unbounded paths.
                let bound = program
                    .structs
                    .len()
                    .saturating_add(program.enums.len())
                    .saturating_add(program.tuples.len())
                    .saturating_add(program.tagged_types.len())
                    .saturating_add(f.value_tys.len())
                    .saturating_add(1)
                    .saturating_mul(2);
                if result.0.iter().any(|(path, _)| path.len() > bound) {
                    return None;
                }
                record(&mut self.values, *v, result);
            }
            _ => {
                if !stmt.operands_satisfy(&mut |op| self.op(op).live()) {
                    return None;
                }
            }
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // MIR passes the loaded owner by value to its consuming callee after clearing the
    // source place. Existing views retain the previous generation across every copy.
    #[test]
    fn resource_transfer_and_replacement_preserve_only_the_current_generation() {
        let mut state = Backing::default();
        let owner = Origins::root(Origin::ResourcePlace(2, vec![1]));
        record(&mut state.values, 0, owner.clone());
        record(&mut state.values, 1, owner.borrowed());
        state.write(5, &[], state.op(&Operand::Value(1)));
        let mut other = state.clone();
        other.write(2, &[0], Origins::default());
        assert!(other.read(5, &[]).live());
        other.write(2, &[1], Origins::default());
        assert!(!other.read(5, &[]).live());
        assert!(other.op(&Operand::Value(0)).live());
        assert!(!other.op(&Operand::Value(0)).borrowed().live());
        assert!(state.merge(&other).is_some());
        assert!(!state.read(5, &[]).live());
        state.write(5, &[], Origins::default());
        assert!(state.read(5, &[]).live());
    }
}

fn borrowed_path(path: &[hir::BorrowedPathSegment]) -> Vec<u32> {
    let mut out = Vec::new();
    for p in path {
        match p {
            hir::BorrowedPathSegment::RootSlot => {}
            hir::BorrowedPathSegment::StructField(i) => out.push(*i),
            hir::BorrowedPathSegment::EnumPayload {
                variant,
                payload_ordinal,
            } => out.extend([*variant, *payload_ordinal]),
            hir::BorrowedPathSegment::OptionSome | hir::BorrowedPathSegment::ResultOk => {
                out.push(0)
            }
            hir::BorrowedPathSegment::ResultErr => out.push(1),
        }
    }
    out
}
