//! Callable value flow used by conditional task access. Source types remain unchanged.
use super::*;
use hir::{
    CallbackContract, CallbackEffects, CallbackInput, CallbackProjection as P, CallbackRead,
    CallbackRequirement, CallbackReturn, CallbackSource as S,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) type ContractMap = HashMap<String, Option<CallbackContract>>;
type Sources = BTreeSet<S>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Value(BTreeMap<Vec<P>, Sources>);
impl Value {
    fn sources(&self, path: &[P]) -> Sources {
        self.0
            .get(path)
            .cloned()
            .unwrap_or_else(|| [S::Unknown].into())
    }
    fn project(&self, prefix: &[P]) -> Self {
        Self(
            self.0
                .iter()
                .filter_map(|(path, sources)| {
                    path.strip_prefix(prefix)
                        .map(|suffix| (suffix.to_vec(), sources.clone()))
                })
                .collect(),
        )
    }
    fn wrap(self, prefix: &[P]) -> Self {
        Self(
            self.0
                .into_iter()
                .map(|(path, sources)| {
                    let mut full = prefix.to_vec();
                    full.extend(path);
                    (full, sources)
                })
                .collect(),
        )
    }
    fn join(&mut self, other: &Self) {
        for (path, sources) in &other.0 {
            self.0
                .entry(path.clone())
                .or_default()
                .extend(sources.iter().cloned());
        }
    }
    fn invalidate_backing(&mut self) {
        for (path, sources) in &mut self.0 {
            if path.contains(&P::ArrayElement) {
                *sources = [S::Unknown].into();
            }
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(super) struct State {
    guards: std::cell::RefCell<BTreeSet<CallbackRequirement>>,
    locals: HashMap<LocalId, Value>,
    expressions: HashMap<usize, Value>,
    loops: HashMap<usize, Value>,
    borrowed_locals: HashSet<LocalId>,
    pub(super) reference_expressions: HashSet<usize>,
}
impl State {
    pub(super) fn join(&mut self, other: &Self) {
        self.guards
            .get_mut()
            .extend(other.guards.borrow().iter().cloned());
        self.borrowed_locals.extend(&other.borrowed_locals);
        self.reference_expressions
            .extend(&other.reference_expressions);
        for (local, value) in &other.locals {
            self.locals.entry(*local).or_default().join(value);
        }
        for (key, value) in &other.expressions {
            self.expressions.entry(*key).or_default().join(value);
        }
        for (key, value) in &other.loops {
            self.loops.entry(*key).or_default().join(value);
        }
    }
    pub(super) fn invalidate_backing(&mut self) {
        for value in self
            .locals
            .values_mut()
            .chain(self.expressions.values_mut())
            .chain(self.loops.values_mut())
        {
            value.invalidate_backing();
        }
        for local in &self.borrowed_locals {
            if let Some(value) = self.locals.get_mut(local) {
                for sources in value.0.values_mut() {
                    *sources = [S::Unknown].into();
                }
            }
        }
        for key in &self.reference_expressions {
            if let Some(value) = self.expressions.get_mut(key) {
                for sources in value.0.values_mut() {
                    *sources = [S::Unknown].into();
                }
            }
        }
    }
}

impl MoveCheck<'_> {
    pub(super) fn callback_tables(&self) -> EffectTypeTables<'_> {
        EffectTypeTables {
            structs: self.structs,
            enums: self.enums,
            tuples: self.tuples,
            tagged_types: self.tagged_types,
        }
    }
    fn callback_unknown(&self, ty: Ty) -> Value {
        Value(
            fn_effect_leaf_paths(ty, self.callback_tables(), false)
                .into_iter()
                .map(|(path, _)| (path, [S::Unknown].into()))
                .collect(),
        )
    }
    fn callback_value(&self, expression: &Expr) -> Value {
        self.borrows
            .callbacks
            .expressions
            .get(&Self::expr_key(expression))
            .cloned()
            .unwrap_or_else(|| self.callback_unknown(expression.ty))
    }
    fn callback_local(&self, local: LocalId) -> Value {
        self.borrows
            .callbacks
            .locals
            .get(&local)
            .cloned()
            .unwrap_or_else(|| {
                self.f
                    .locals
                    .get(local as usize)
                    .map_or_else(Value::default, |l| self.callback_unknown(l.ty))
            })
    }
    pub(super) fn seed_callback_parameter(&mut self, parameter: u32, local: LocalId) {
        let Some(ty) = self.f.locals.get(local as usize).map(|l| l.ty) else {
            return;
        };
        let value = Value(
            fn_effect_leaf_paths(ty, self.callback_tables(), false)
                .into_iter()
                .map(|(path, _)| {
                    (
                        path.clone(),
                        [S::Parameter(CallbackInput { parameter, path })].into(),
                    )
                })
                .collect(),
        );
        self.borrows.callbacks.locals.insert(local, value);
        if self
            .f
            .param_modes
            .get(parameter as usize)
            .is_some_and(|mode| *mode != ast::ParamMode::ByValue)
        {
            self.borrows.callbacks.borrowed_locals.insert(local);
        }
    }
    pub(super) fn mark_callback_borrowed_local(&mut self, local: LocalId) {
        self.borrows.callbacks.borrowed_locals.insert(local);
    }
    pub(super) fn assign_callback(&mut self, local: LocalId, expression: &Expr) {
        let value = self.callback_value(expression);
        self.borrows.callbacks.locals.insert(local, value);
    }
    pub(super) fn assign_callback_projection(
        &mut self,
        local: LocalId,
        path: &[BorrowProjection],
        expression: &Expr,
    ) {
        let Some(path) = path
            .iter()
            .map(callback_projection)
            .collect::<Option<Vec<_>>>()
        else {
            self.borrows.callbacks.invalidate_backing();
            return;
        };
        let incoming = self.callback_value(expression).wrap(&path);
        let current = self.borrows.callbacks.locals.entry(local).or_default();
        current
            .0
            .retain(|candidate, _| !candidate.starts_with(&path));
        current.join(&incoming);
    }
    pub(super) fn bind_callback_projection(
        &mut self,
        local: LocalId,
        expression: &Expr,
        projection: Option<BorrowProjection>,
    ) {
        let value = projection
            .and_then(|p| callback_projection(&p))
            .map_or_else(
                || {
                    self.f
                        .locals
                        .get(local as usize)
                        .map_or_else(Value::default, |l| self.callback_unknown(l.ty))
                },
                |p| self.callback_value(expression).project(&[p]),
            );
        self.borrows.callbacks.locals.insert(local, value);
    }
    fn substitute_callback_sources(&self, sources: &[S], arguments: &[Value]) -> Sources {
        let mut result = Sources::new();
        for source in sources {
            match source {
                S::Parameter(input) => result.extend(
                    arguments
                        .get(input.parameter as usize)
                        .map_or_else(|| [S::Unknown].into(), |value| value.sources(&input.path)),
                ),
                _ => {
                    result.insert(source.clone());
                }
            }
        }
        result
    }
    fn callback_call_result(&self, target: &str, arguments: &[Value], ty: Ty) -> Value {
        let Some(contract) = self
            .named_callback_contracts
            .get(target)
            .and_then(Option::as_ref)
        else {
            return self.callback_unknown(ty);
        };
        Value(
            contract
                .returns
                .iter()
                .map(|row| {
                    (
                        row.path.clone(),
                        self.substitute_callback_sources(&row.sources, arguments),
                    )
                })
                .collect(),
        )
    }
    fn callback_named_value(&self, target: &str, captures: &[Expr], ty: Ty) -> Value {
        let Some(modes) = self.named_param_modes.get(target) else {
            return self.callback_unknown(ty);
        };
        let Some(access) = self
            .named_mutable_access
            .get(target)
            .and_then(Option::as_ref)
        else {
            return self.callback_unknown(ty);
        };
        let Some(explicit) = modes.len().checked_sub(captures.len()) else {
            return self.callback_unknown(ty);
        };
        let mut effects = CallbackEffects::default();
        let mut may_access = access.direct.clone();
        for row in &access.conditional {
            let safe_capture = (row.requirement.input.parameter as usize).checked_sub(explicit)
                .and_then(|i| captures.get(i))
                .is_some_and(|capture| !row.requirement.input.path.contains(&P::ArrayElement)
                    && self.callback_value(capture).sources(&row.requirement.input.path).iter()
                        .all(|source| matches!(source, S::Known(effects) if Self::callback_read_satisfied(effects, row.requirement.read))));
            if !safe_capture {
                may_access.extend(row.roots.iter().cloned());
            }
        }
        for root in &may_access {
            let (hir::MutableAccessRoot::Contained(index) | hir::MutableAccessRoot::Storage(index)) =
                root;
            if (*index as usize) < explicit {
                effects.argument_writes.push(*index);
            } else {
                effects.environment_write = true;
            }
        }
        for (index, mode) in modes.iter().take(explicit).enumerate() {
            if matches!(mode, ast::ParamMode::BorrowMut | ast::ParamMode::Out)
                && let Ok(index) = u32::try_from(index)
            {
                effects.argument_writes.push(index);
            }
        }
        effects.argument_writes.sort();
        effects.argument_writes.dedup();
        Value([(Vec::new(), [S::Known(effects)].into())].into())
    }
    /// Called after children complete but before the action mutates/replaces an operand.
    pub(super) fn callback_completion(&self, expression: &Expr) -> Value {
        if !matches!(
            expand_tagged_ty(expression.ty, self.tagged_types),
            Ty::Fn(_)
                | Ty::Struct(_)
                | Ty::Tuple(_)
                | Ty::Enum(_)
                | Ty::Option(_)
                | Ty::Result(..)
                | Ty::Array(..)
                | Ty::StructArray(..)
                | Ty::DynArray(_)
                | Ty::DynStructArray(..)
                | Ty::Slice(_)
        ) {
            return Value::default();
        }
        let unknown = || self.callback_unknown(expression.ty);
        let mut result = match &expression.kind {
            ExprKind::Local(local) => self.callback_local(*local),
            ExprKind::Field { root, path } => self
                .callback_local(*root)
                .project(&path.iter().copied().map(P::StructField).collect::<Vec<_>>()),
            ExprKind::Index { recv, .. } => self.callback_value(recv).project(&[P::ArrayElement]),
            ExprKind::IndexField { base, path, .. } => {
                let mut prefix = vec![P::ArrayElement];
                prefix.extend(path.iter().copied().map(P::StructField));
                self.callback_local(*base).project(&prefix)
            }
            ExprKind::ElemField { recv, path, .. } => {
                let mut prefix = vec![P::ArrayElement];
                prefix.extend(path.iter().copied().map(P::StructField));
                self.callback_value(recv).project(&prefix)
            }
            ExprKind::TupleIndex { recv, index } => self
                .callback_value(recv)
                .project(&[P::TupleElement(*index)]),
            ExprKind::FnValue(target) => self.callback_named_value(target, &[], expression.ty),
            ExprKind::Closure { lifted, captures } => {
                self.callback_named_value(lifted, captures, expression.ty)
            }
            ExprKind::StructLit { fields, .. } | ExprKind::Tuple { elems: fields, .. } => {
                let mut value = Value::default();
                for (index, field) in fields.iter().enumerate() {
                    let Ok(index) = u32::try_from(index) else {
                        return unknown();
                    };
                    let projection = if matches!(expression.kind, ExprKind::Tuple { .. }) {
                        P::TupleElement(index)
                    } else {
                        P::StructField(index)
                    };
                    value.join(&self.callback_value(field).wrap(&[projection]));
                }
                value
            }
            ExprKind::EnumValue {
                variant, payload, ..
            } => {
                let mut value = Value::default();
                for (index, field) in payload.iter().enumerate() {
                    let Ok(index) = u32::try_from(index) else {
                        return unknown();
                    };
                    value.join(&self.callback_value(field).wrap(&[P::EnumPayload {
                        variant: *variant,
                        index,
                    }]));
                }
                value
            }
            ExprKind::OptionNone => Value::default(),
            ExprKind::OptionSome(v) => self.callback_value(v).wrap(&[P::OptionSome]),
            ExprKind::ResultOk(v) => self.callback_value(v).wrap(&[P::ResultOk]),
            ExprKind::ResultErr(v) => self.callback_value(v).wrap(&[P::ResultErr]),
            ExprKind::ArrayLit { elems, .. } => {
                let mut value = Value::default();
                for e in elems {
                    value.join(&self.callback_value(e).wrap(&[P::ArrayElement]));
                }
                value
            }
            ExprKind::SliceRange { recv, .. } | ExprKind::ArrayToSlice(recv) => {
                self.callback_value(recv)
            }
            ExprKind::Call { func, args, .. } => self.callback_call_result(
                func,
                &args
                    .iter()
                    .map(|a| self.callback_value(a))
                    .collect::<Vec<_>>(),
                expression.ty,
            ),
            ExprKind::Block(b)
            | ExprKind::Unsafe(b)
            | ExprKind::Arena(b)
            | ExprKind::TaskGroup(b)
            | ExprKind::NamedArena { block: b, .. }
            | ExprKind::FloatScope { block: b, .. } => b
                .value
                .as_deref()
                .map_or_else(Value::default, |v| self.callback_value(v)),
            ExprKind::If { then, els, .. } => {
                let mut value = Value::default();
                for b in [then, els] {
                    if let Some(v) = b.value.as_deref() {
                        value.join(&self.callback_value(v));
                    }
                }
                value
            }
            ExprKind::Match { arms, .. } => {
                let mut value = Value::default();
                for arm in arms {
                    if !hir_expr_diverges(&arm.body) {
                        value.join(&self.callback_value(&arm.body));
                    }
                }
                value
            }
            ExprKind::Loop { .. } => self
                .borrows
                .callbacks
                .loops
                .get(&Self::expr_key(expression))
                .cloned()
                .unwrap_or_else(unknown),
            ExprKind::Try(v) => self.callback_value(v).project(&[P::ResultOk]),
            ExprKind::ResultMapErr { result, f } => {
                let mut value = self
                    .callback_value(result)
                    .project(&[P::ResultOk])
                    .wrap(&[P::ResultOk]);
                let input = self.callback_value(result).project(&[P::ResultErr]);
                let error = match &f.kind {
                    ExprKind::FnValue(target) => self.callback_call_result(
                        target,
                        &[input],
                        match expand_tagged_ty(expression.ty, self.tagged_types) {
                            Ty::Result(_, e) => scalar_to_ty(e),
                            _ => Ty::Error,
                        },
                    ),
                    ExprKind::Closure { lifted, captures } => {
                        let mut args = vec![input];
                        args.extend(captures.iter().map(|c| self.callback_value(c)));
                        self.callback_call_result(
                            lifted,
                            &args,
                            match expand_tagged_ty(expression.ty, self.tagged_types) {
                                Ty::Result(_, e) => scalar_to_ty(e),
                                _ => Ty::Error,
                            },
                        )
                    }
                    _ => self
                        .callback_unknown(expression.ty)
                        .project(&[P::ResultErr]),
                };
                value.join(&error.wrap(&[P::ResultErr]));
                value
            }
            ExprKind::ElseUnwrap { opt, fallback } => {
                let projection =
                    if matches!(expand_tagged_ty(opt.ty, self.tagged_types), Ty::Option(_)) {
                        P::OptionSome
                    } else {
                        P::ResultOk
                    };
                let mut value = self.callback_value(opt).project(&[projection]);
                if !hir_expr_diverges(fallback) {
                    value.join(&self.callback_value(fallback));
                }
                value
            }
            ExprKind::ArrayToArray { source, stages, .. } => {
                let mut element = self.callback_value(source).project(&[P::ArrayElement]);
                for stage in stages {
                    match &stage.kind {
                        StageKind::Map { func, captures } => {
                            if self
                                .named_callback_contracts
                                .get(func)
                                .and_then(Option::as_ref)
                                .is_none()
                            {
                                return unknown();
                            }
                            let mut args = vec![element];
                            args.extend(captures.iter().map(|c| self.callback_value(c)));
                            element = self.callback_call_result(func, &args, Ty::Error);
                        }
                        StageKind::Project { field } => {
                            element = element.project(&[P::StructField(*field)])
                        }
                        _ => {}
                    }
                }
                element.wrap(&[P::ArrayElement])
            }
            _ => unknown(),
        };
        // Only constructors prove an absent payload. In particular, a projection beyond a
        // recursive type cut or an unavailable source mapping must not become an empty proof.
        let constructs_absence = matches!(
            &expression.kind,
            ExprKind::OptionNone
                | ExprKind::OptionSome(_)
                | ExprKind::ResultOk(_)
                | ExprKind::ResultErr(_)
                | ExprKind::EnumValue { .. }
        ) || matches!(&expression.kind, ExprKind::ArrayLit { elems, .. } if elems.is_empty());
        for (path, _) in fn_effect_leaf_paths(expression.ty, self.callback_tables(), false) {
            result.0.entry(path).or_insert_with(|| {
                if constructs_absence {
                    Sources::new()
                } else {
                    [S::Unknown].into()
                }
            });
        }
        result
    }
    pub(super) fn store_callback_completion(&mut self, expression: &Expr, value: Value) {
        if value.0.is_empty() {
            self.borrows
                .callbacks
                .expressions
                .remove(&Self::expr_key(expression));
        } else {
            self.borrows
                .callbacks
                .expressions
                .insert(Self::expr_key(expression), value);
        }
    }
    pub(super) fn collect_callback_break(&mut self, expression: &Expr, key: usize) {
        let value = self.callback_value(expression);
        self.borrows
            .callbacks
            .loops
            .entry(key)
            .or_default()
            .join(&value);
    }
    pub(super) fn assign_callback_tuple(&mut self, locals: &[Option<LocalId>], expression: &Expr) {
        let value = self.callback_value(expression);
        for (i, local) in locals.iter().enumerate() {
            if let (Some(local), Ok(i)) = (local, u32::try_from(i)) {
                self.borrows
                    .callbacks
                    .locals
                    .insert(*local, value.project(&[P::TupleElement(i)]));
            }
        }
    }
    pub(super) fn collect_callback_return(&mut self, expression: &Expr) {
        let value = if self.borrows.callbacks.guards.borrow().is_empty() {
            self.callback_value(expression)
        } else {
            self.callback_unknown(expression.ty)
        };
        self.callback_returns.join(&value);
    }
    pub(super) fn collect_callback_try_error(&mut self, expression: &Expr) {
        let value = if self.borrows.callbacks.guards.borrow().is_empty() {
            self.callback_value(expression)
        } else {
            self.callback_unknown(expression.ty)
        };
        self.callback_returns
            .join(&value.project(&[P::ResultErr]).wrap(&[P::ResultErr]));
    }
    pub(super) fn callback_return_rows(&self) -> Vec<CallbackReturn> {
        let mut rows = fn_effect_leaf_paths(self.f.ret, self.callback_tables(), false)
            .into_iter()
            .map(|(path, _)| CallbackReturn {
                sources: self
                    .callback_returns
                    .0
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| [S::Unknown].into())
                    .into_iter()
                    .collect(),
                path,
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        rows
    }
}
fn callback_projection(projection: &BorrowProjection) -> Option<P> {
    Some(match *projection {
        BorrowProjection::StructField(i) => P::StructField(i),
        BorrowProjection::TupleElement(i) => P::TupleElement(i),
        BorrowProjection::ArrayElement(_) => P::ArrayElement,
        BorrowProjection::EnumPayload { variant, index } => P::EnumPayload { variant, index },
        BorrowProjection::OptionSome => P::OptionSome,
        BorrowProjection::ResultOk => P::ResultOk,
        BorrowProjection::ResultErr => P::ResultErr,
        _ => return None,
    })
}

impl MoveCheck<'_> {
    /// Copied headers and immutable bytes do not grant write authority. A slice's slots do,
    /// including slice<str>; recurse through value records to preserve only writable occurrences.
    pub(super) fn callback_argument_roots(&self, ty: Ty, fact: &BorrowFact) -> BorrowRoots {
        let mut roots = BorrowRoots::new();
        let mut work = vec![(ty, fact.clone(), HashSet::new())];
        while let Some((ty, fact, mut visited)) = work.pop() {
            let ty = expand_tagged_ty(ty, self.tagged_types);
            if !visited.insert(ty) {
                roots.extend(fact.live_roots());
                continue;
            }
            let mut child = |ty, p| work.push((ty, fact.project_exact(p), visited.clone()));
            match ty {
                Ty::Str
                | Ty::Int(_)
                | Ty::Float(_)
                | Ty::Bool
                | Ty::Char
                | Ty::Unit
                | Ty::HttpRequestView
                | Ty::HttpHeaders => {}
                Ty::Struct(id) => {
                    if let Some(def) = self.structs.get(id as usize) {
                        for (i, field) in def.fields.iter().enumerate() {
                            if let Ok(i) = u32::try_from(i) {
                                child(field.ty, BorrowProjection::StructField(i));
                            } else {
                                roots.extend(fact.live_roots());
                            }
                        }
                    } else {
                        roots.extend(fact.live_roots());
                    }
                }
                Ty::Tuple(id) => {
                    if let Some(def) = self.tuples.get(id as usize) {
                        for (i, ty) in def.elems.iter().enumerate() {
                            if let Ok(i) = u32::try_from(i) {
                                child(scalar_to_ty(*ty), BorrowProjection::TupleElement(i));
                            } else {
                                roots.extend(fact.live_roots());
                            }
                        }
                    } else {
                        roots.extend(fact.live_roots());
                    }
                }
                Ty::Enum(id) => {
                    if let Some(def) = self.enums.get(id as usize) {
                        for (variant, arm) in def.variants.iter().enumerate() {
                            for (index, ty) in arm.payload.iter().enumerate() {
                                if let (Ok(variant), Ok(index)) =
                                    (u32::try_from(variant), u32::try_from(index))
                                {
                                    child(
                                        scalar_to_ty(*ty),
                                        BorrowProjection::EnumPayload { variant, index },
                                    );
                                } else {
                                    roots.extend(fact.live_roots());
                                }
                            }
                        }
                    } else {
                        roots.extend(fact.live_roots());
                    }
                }
                Ty::Option(ty) => child(scalar_to_ty(ty), BorrowProjection::OptionSome),
                Ty::Result(ok, err) => {
                    child(scalar_to_ty(ok), BorrowProjection::ResultOk);
                    child(scalar_to_ty(err), BorrowProjection::ResultErr);
                }
                _ => roots.extend(fact.live_roots()),
            }
        }
        roots
    }
    fn callback_read_satisfied(effects: &CallbackEffects, read: CallbackRead) -> bool {
        match read {
            CallbackRead::Environment => !effects.environment_write,
            CallbackRead::Argument(index) => !effects.argument_writes.contains(&index),
        }
    }
    /// Classify one opaque action without losing a coincident unconditional action.
    fn conditional_callback_roots(
        &self,
        sources: Sources,
        read: CallbackRead,
        roots: BorrowRoots,
        mutation: bool,
        task: bool,
    ) -> BorrowRoots {
        if roots.is_empty() {
            return roots;
        }
        let mut direct = BorrowRoots::new();
        for source in sources {
            match source {
                S::Known(effects) if Self::callback_read_satisfied(&effects, read) => {}
                S::Parameter(input) if !mutation => {
                    let requirement = CallbackRequirement { input, read };
                    let guards = self.borrows.callbacks.guards.borrow().clone();
                    if task {
                        self.callback_requirements.borrow_mut().extend(guards);
                    } else {
                        for guard in guards {
                            self.conditional_access
                                .borrow_mut()
                                .entry(guard)
                                .or_default()
                                .extend(self.borrows.summary_roots(roots.clone()));
                        }
                    }
                    if self.callback_requirement_can_replace(&requirement) {
                        self.borrows
                            .callbacks
                            .guards
                            .borrow_mut()
                            .insert(requirement.clone());
                    }
                    if task {
                        self.callback_requirements.borrow_mut().insert(requirement);
                    } else {
                        self.conditional_access
                            .borrow_mut()
                            .entry(requirement)
                            .or_default()
                            .extend(self.borrows.summary_roots(roots.clone()));
                        self.check_pending_callback_access(&roots);
                    }
                }
                _ => direct.extend(roots.iter().cloned()),
            }
        }
        direct
    }
    fn check_pending_callback_access(&self, roots: &BorrowRoots) {
        for (&key, task) in &self.borrows.tasks.pending {
            let mut protected = task.value.non_storage.live_roots();
            protected.extend(self.borrows.header_release_roots(&task.value.headers));
            protected.extend(
                task.value
                    .headers
                    .leaves
                    .values()
                    .flat_map(|leaf| leaf.generations.iter())
                    .map(|r| BorrowRoot::Observation(r.generation.clone())),
            );
            let protected = self.borrows.expand_byte_validation_roots(protected);
            if roots.iter().any(|access| {
                protected
                    .iter()
                    .any(|root| self.task_roots_overlap(access, root))
            }) {
                self.borrows
                    .tasks
                    .conflicts
                    .borrow_mut()
                    .insert(key, task.span);
            }
        }
    }
    pub(super) fn conditional_direct_access(
        &self,
        target: &str,
        args: &[Expr],
        mutation: bool,
    ) -> BorrowRoots {
        let Some(access) = self
            .named_mutable_access
            .get(target)
            .and_then(Option::as_ref)
        else {
            return BorrowRoots::new();
        };
        let mut result = BorrowRoots::new();
        for row in &access.conditional {
            let sources = args
                .get(row.requirement.input.parameter as usize)
                .map_or_else(
                    || [S::Unknown].into(),
                    |a| self.callback_value(a).sources(&row.requirement.input.path),
                );
            let mut roots = BorrowRoots::new();
            for root in &row.roots {
                match root {
                    hir::MutableAccessRoot::Contained(i) => {
                        if let Some(a) = args.get(*i as usize) {
                            roots.extend(
                                self.callback_argument_roots(
                                    a.ty,
                                    &self.callback_argument_value(a),
                                ),
                            );
                        }
                    }
                    hir::MutableAccessRoot::Storage(i) => {
                        if let Some(a) = args.get(*i as usize) {
                            roots.extend(self.access_storage_fact(a).live_roots());
                        }
                    }
                }
            }
            result.extend(self.conditional_callback_roots(
                sources,
                row.requirement.read,
                roots,
                mutation,
                false,
            ));
        }
        result
    }
    pub(super) fn opaque_callback_access(
        &self,
        callee: &Expr,
        arguments: &[(BorrowFact, BorrowFact)],
        mutation: bool,
    ) -> BorrowRoots {
        let sources = self.callback_value(callee).sources(&[]);
        let mut environment = self.completed_value_fact(callee).live_roots();
        environment.extend(
            self.borrows
                .header_release_roots(&self.completed_headers(callee)),
        );
        let mut roots = self.conditional_callback_roots(
            sources.clone(),
            CallbackRead::Environment,
            environment,
            mutation,
            false,
        );
        let signature = match callee.ty {
            Ty::Fn(id) => self.fn_types.get(id as usize),
            _ => None,
        };
        for (index, (value, storage)) in arguments.iter().enumerate() {
            let Some((mode, ty)) = signature.and_then(|f| f.params.get(index)) else {
                roots.extend(value.live_roots());
                roots.extend(storage.live_roots());
                continue;
            };
            let mut argument_roots = self.callback_argument_roots(scalar_to_ty(*ty), value);
            if matches!(mode, ast::ParamMode::BorrowMut | ast::ParamMode::Out) {
                // Exclusive access is unconditional, irrespective of body guarantees.
                roots.extend(storage.live_roots());
                roots.extend(argument_roots);
                continue;
            }
            if *mode == ast::ParamMode::Borrow {
                argument_roots.extend(value.live_roots());
            }
            let Ok(index) = u32::try_from(index) else {
                roots.extend(argument_roots);
                continue;
            };
            roots.extend(self.conditional_callback_roots(
                sources.clone(),
                CallbackRead::Argument(index),
                argument_roots,
                mutation,
                false,
            ));
        }
        roots
    }
    pub(super) fn check_callback_requirements(
        &mut self,
        target: &str,
        arguments: &[Expr],
        span: Span,
    ) {
        if self
            .named_callback_contracts
            .get(target)
            .is_some_and(Option::is_none)
            && arguments
                .iter()
                .any(|a| !fn_effect_leaf_paths(a.ty, self.callback_tables(), false).is_empty())
        {
            self.diags.error(
                "callback access contract is unavailable for this imported consumer".to_owned(),
                span,
            );
            return;
        }
        let requirements = self
            .named_callback_contracts
            .get(target)
            .and_then(Option::as_ref)
            .map(|c| c.requirements.clone())
            .unwrap_or_default();
        for requirement in requirements {
            let sources = arguments
                .get(requirement.input.parameter as usize)
                .map_or_else(
                    || [S::Unknown].into(),
                    |a| self.callback_value(a).sources(&requirement.input.path),
                );
            self.require_callback_sources(sources, requirement.read, span);
        }
    }
    fn require_callback_sources(&mut self, sources: Sources, read: CallbackRead, span: Span) {
        self.callback_requirements
            .borrow_mut()
            .extend(self.borrows.callbacks.guards.borrow().iter().cloned());
        for source in sources {
            match source {
                S::Known(effects) if Self::callback_read_satisfied(&effects, read) => {}
                S::Parameter(input) => { self.callback_requirements.borrow_mut().insert(CallbackRequirement { input, read }); }
                _ => self.diags.error("callback may modify shared task backing; a read-only access guarantee is required".to_owned(), span),
            }
        }
    }
    pub(super) fn task_callback_access(&mut self, closure: &Expr) -> BorrowRoots {
        let ExprKind::Closure { lifted, captures } = &closure.kind else {
            return self.indirect_mutable_access(closure, &[]);
        };
        let Some(access) = self
            .named_mutable_access
            .get(lifted)
            .and_then(Option::as_ref)
            .cloned()
        else {
            return self.indirect_mutable_access(closure, &[]);
        };
        self.check_callback_requirements(lifted, captures, closure.span);
        let mut roots = BorrowRoots::new();
        let mut map = |root: &hir::MutableAccessRoot| -> BorrowRoots {
            match root {
                hir::MutableAccessRoot::Contained(i) => captures
                    .get(*i as usize)
                    .map_or_else(BorrowRoots::new, |a| {
                        self.completed_value_fact(a).live_roots()
                    }),
                hir::MutableAccessRoot::Storage(i) => captures
                    .get(*i as usize)
                    .map_or_else(BorrowRoots::new, |a| {
                        self.access_storage_fact(a).live_roots()
                    }),
            }
        };
        for root in &access.direct {
            roots.extend(map(root));
        }
        for row in &access.conditional {
            let sources = captures
                .get(row.requirement.input.parameter as usize)
                .map_or_else(
                    || [S::Unknown].into(),
                    |a| self.callback_value(a).sources(&row.requirement.input.path),
                );
            let selected = row.roots.iter().flat_map(&mut map).collect();
            roots.extend(self.conditional_callback_roots(
                sources,
                row.requirement.read,
                selected,
                false,
                true,
            ));
        }
        roots
    }
}

impl MoveCheck<'_> {
    pub(super) fn check_callback_formation(&mut self, expression: &Expr) {
        let (target, captures) = match &expression.kind {
            ExprKind::FnValue(target) => (target, &[][..]),
            ExprKind::Closure { lifted, captures } => (lifted, captures.as_slice()),
            _ => return,
        };
        let Some(_) = self
            .named_callback_contracts
            .get(target)
            .and_then(Option::as_ref)
        else {
            if self
                .named_callback_contracts
                .get(target)
                .is_some_and(Option::is_none)
                && let Ty::Fn(id) = expression.ty
                && self.fn_types.get(id as usize).is_none_or(|f| {
                    f.params.iter().any(|(_, ty)| {
                        !fn_effect_leaf_paths(scalar_to_ty(*ty), self.callback_tables(), false)
                            .is_empty()
                    })
                })
            {
                self.diags.error(
                    "callback access contract is unavailable for this imported function value"
                        .to_owned(),
                    expression.span,
                );
            }
            return;
        };
        self.check_callback_target_formation(
            target,
            captures,
            expression.span,
            self.immediate_task_closures
                .contains(&Self::expr_key(expression)),
        );
    }
    pub(super) fn check_callback_stage(&mut self, target: &str, captures: &[Expr], span: Span, callback_inputs: bool) {
        if callback_inputs && self
            .named_callback_contracts
            .get(target)
            .is_some_and(Option::is_none)
        {
            self.diags.error(
                "callback access contract is unavailable for this pipeline consumer".to_owned(),
                span,
            );
            return;
        }
        self.check_callback_target_formation(target, captures, span, false);
    }
    fn check_callback_target_formation(
        &mut self,
        target: &str,
        captures: &[Expr],
        span: Span,
        immediate: bool,
    ) {
        let Some(contract) = self
            .named_callback_contracts
            .get(target)
            .and_then(Option::as_ref)
        else {
            return;
        };
        let count = self.named_param_modes.get(target).map_or(0, Vec::len);
        let Some(explicit) = count.checked_sub(captures.len()) else {
            return;
        };
        for requirement in contract.requirements.clone() {
            let Some(capture) = (requirement.input.parameter as usize).checked_sub(explicit) else {
                self.diags.error(
                    "a function with task callback requirements must be called directly".to_owned(),
                    span,
                );
                continue;
            };
            if immediate {
                continue;
            }
            if requirement.input.path.contains(&P::ArrayElement) {
                self.diags.error("a reusable closure cannot retain a task callback guarantee through replaceable backing".to_owned(), span);
                continue;
            }
            let sources = captures.get(capture).map_or_else(
                || [S::Unknown].into(),
                |c| self.callback_value(c).sources(&requirement.input.path),
            );
            self.require_callback_sources(sources, requirement.read, span);
        }
    }
    pub(super) fn invalidate_call_callback_contents(
        &mut self,
        target: Option<&str>,
        arguments: &[Expr],
    ) {
        let summary = target
            .and_then(|name| self.named_mutable_access.get(name))
            .and_then(Option::as_ref);
        let may_replace = arguments.iter().enumerate().any(|(index, arg)| {
            !fn_effect_leaf_paths(arg.ty, self.callback_tables(), false).is_empty()
                && summary.is_none_or(|s| {
                    s.direct
                        .contains(&hir::MutableAccessRoot::Contained(index as u32))
                        || s.direct
                            .contains(&hir::MutableAccessRoot::Storage(index as u32))
                })
        });
        let conditional_writer = summary.is_some_and(|s| {
            s.conditional.iter().any(|row| {
                arguments
                    .get(row.requirement.input.parameter as usize)
                    .is_none_or(|a| {
                        self.callback_value(a)
                            .sources(&row.requirement.input.path)
                            .iter()
                            .any(|source| match source {
                                S::Unknown => true,
                                S::Known(effects) => {
                                    !Self::callback_read_satisfied(effects, row.requirement.read)
                                }
                                S::Parameter(_) => false,
                            })
                    })
            })
        });
        if may_replace || conditional_writer {
            self.borrows.callbacks.invalidate_backing();
            // Exclusive formal slots may also replace a directly held callback value.
            if let Some(modes) = target.and_then(|name| self.named_param_modes.get(name)) {
                for (arg, mode) in arguments.iter().zip(modes) {
                    if matches!(mode, ast::ParamMode::BorrowMut | ast::ParamMode::Out)
                        && let Some(place) = Self::mutable_actual_place(arg)
                    {
                        let value = self
                            .f
                            .locals
                            .get(place.root as usize)
                            .map_or_else(Value::default, |l| self.callback_unknown(l.ty));
                        self.borrows.callbacks.locals.insert(place.root, value);
                    }
                }
            }
        }
    }
}

pub(super) fn validate_program_contract(
    program: &Program,
    params: &[Ty],
    ret: Ty,
    access: &hir::MutableAccessSummary,
    contract: &Option<CallbackContract>,
) -> Result<(), &'static str> {
    hir::validate_callback_record_order(contract, params.len(), false)?;
    let tables = EffectTypeTables {
        structs: &program.structs,
        enums: &program.enums,
        tuples: &program.tuples,
        tagged_types: &program.tagged_types,
    };
    let signature = |id: u32| {
        program.fn_types.get(id as usize).map(|f| {
            (
                (f.params.clone(), f.ret),
                f.params.iter().map(|(mode, _)| *mode).collect::<Vec<_>>(),
            )
        })
    };
    let mut results = fn_effect_leaf_paths(ret, tables, false)
        .into_iter()
        .map(|(path, id)| signature(id).map(|(signature, modes)| (path, signature, modes)))
        .collect::<Option<Vec<_>>>()
        .ok_or("callback result signature")?;
    results.sort_by(|a, b| a.0.cmp(&b.0));
    hir::validate_callback_contract(
        contract,
        access,
        false,
        |input| {
            let ty = *params.get(input.parameter as usize)?;
            let (_, id) = fn_effect_leaf_paths(ty, tables, false)
                .into_iter()
                .find(|(path, _)| path == &input.path)?;
            signature(id)
        },
        &results,
    )
}

impl MoveCheck<'_> {
    fn callback_requirement_can_replace(&self, requirement: &CallbackRequirement) -> bool {
        if requirement.read == CallbackRead::Environment {
            return true;
        }
        let Some(local) = self
            .f
            .params
            .get(requirement.input.parameter as usize)
            .and_then(|i| self.f.locals.get(*i as usize))
        else {
            return true;
        };
        self.callback_read_can_replace(local.ty, &requirement.input.path, requirement.read)
    }
    fn callback_read_can_replace(&self, ty: Ty, path: &[P], read: CallbackRead) -> bool {
        let CallbackRead::Argument(index) = read else {
            return true;
        };
        fn_effect_leaf_paths(ty, self.callback_tables(), false)
            .into_iter()
            .find(|(p, _)| p == path)
            .and_then(|(_, id)| self.fn_types.get(id as usize))
            .and_then(|f| f.params.get(index as usize))
            .is_none_or(|(_, ty)| {
                !fn_effect_leaf_paths(scalar_to_ty(*ty), self.callback_tables(), false).is_empty()
            })
    }
    pub(super) fn invalidate_indirect_callback_contents(
        &mut self,
        callee: &Expr,
        arguments: &[Expr],
    ) {
        let sources = self.callback_value(callee).sources(&[]);
        let replaces = sources.iter().any(|source| match source {
            S::Unknown => true,
            S::Parameter(_) => false,
            S::Known(effects) => {
                effects.environment_write
                    || effects.argument_writes.iter().any(|i| {
                        arguments.get(*i as usize).is_none_or(|a| {
                            !fn_effect_leaf_paths(a.ty, self.callback_tables(), false).is_empty()
                        })
                    })
            }
        });
        if replaces {
            self.borrows.callbacks.invalidate_backing();
        }
    }
}

impl MoveCheck<'_> {
    /// Scalar slices carry their writable backing in the generation lane, even when their
    /// elements carry no non-storage lifetime roots. Keep each header's typed occurrence.
    pub(super) fn callback_argument_value(&self, expression: &Expr) -> BorrowFact {
        let mut value = self.completed_value_fact(expression);
        let headers = self.completed_headers(expression);
        for (path, leaf) in &headers.leaves {
            if leaf.descriptor.is_none_or(|d| {
                d.kind == StorageHeaderKind::View
                    && !matches!(
                        expand_tagged_ty(d.ty, self.tagged_types),
                        Ty::Str | Ty::HttpHeaders | Ty::HttpRequestView
                    )
            }) {
                let selected = ProjectedHeaderFact {
                    leaves: [(Vec::new(), leaf.clone())].into(),
                };
                let mut roots = self.borrows.header_release_roots(&selected);
                if !leaf.known {
                    roots.extend(self.completed_storage_fact(expression).live_roots());
                }
                value
                    .projected
                    .entry(path.clone())
                    .or_default()
                    .extend(roots);
            }
        }
        value
    }
}

impl MoveCheck<'_> {
    pub(super) fn invalidate_callback_backing_for_access(&mut self, roots: &BorrowRoots) {
        let overlaps = self.borrows.callbacks.locals.iter().any(|(local, value)| {
            let borrowed = self.borrows.callbacks.borrowed_locals.contains(local);
            if !borrowed && !value.0.keys().any(|path| path.contains(&P::ArrayElement)) {
                return false;
            }
            let mut backing = self.local_borrow_fact(*local).live_roots();
            if borrowed {
                backing.extend(self.local_storage_roots(*local));
            }
            backing.extend(
                self.borrows
                    .header_release_roots(&self.local_headers(*local)),
            );
            roots.iter().any(|access| {
                backing
                    .iter()
                    .any(|root| self.task_roots_overlap(access, root))
            })
        });
        // A borrowed argument denotes a place, unlike a completed by-value callback copy.
        // If its exact place overlap is unavailable, do not preserve its former contents.
        let reference_live = !roots.is_empty()
            && self
                .borrows
                .callbacks
                .reference_expressions
                .iter()
                .any(|key| {
                    self.borrows
                        .callbacks
                        .expressions
                        .get(key)
                        .is_some_and(|value| !value.0.is_empty())
                });
        if overlaps || reference_live {
            self.borrows.callbacks.invalidate_backing();
        }
    }
}
