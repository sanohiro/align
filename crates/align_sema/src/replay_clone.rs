//! Stack-bounded cloning for checked-HIR body replay.
//!
//! The derived `Clone` implementations for HIR are correct for ordinary compiler inputs, but
//! they recursively clone every boxed expression. The replay boundary accepts producer-valid
//! bodies up to `MAX_CHECKED_HIR_DEPTH` on a 2 MiB owner stack, so replay must rebuild the tree from
//! a child-first explicit worklist instead of calling those derived implementations.

use crate::hir::{self, ExprKind, StageKind, Stmt, TemplatePart};

// The explicit worklist keeps recursive HIR replay off the owner stack. Boxing the Stmt arm
// would add an allocation to every replayed statement, which defeats that boundary's purpose.
#[allow(clippy::large_enum_variant)]
enum CloneValue {
    Expr(hir::Expr),
    Block(hir::Block),
    Stmt(hir::Stmt),
    MatchArm(hir::MatchArm),
    Stage(hir::Stage),
    TemplatePart(TemplatePart),
}

struct ChildValues {
    values: std::vec::IntoIter<CloneValue>,
}

impl ChildValues {
    fn new(values: Vec<CloneValue>) -> Self {
        Self {
            values: values.into_iter(),
        }
    }

    fn expr(&mut self) -> Option<hir::Expr> {
        match self.values.next()? {
            CloneValue::Expr(expr) => Some(expr),
            _ => None,
        }
    }

    fn expr_box(&mut self) -> Option<Box<hir::Expr>> {
        Some(Box::new(self.expr()?))
    }

    fn exprs(&mut self, count: usize) -> Option<Vec<hir::Expr>> {
        (0..count).map(|_| self.expr()).collect()
    }

    fn optional_expr_box(&mut self, present: bool) -> Option<Option<Box<hir::Expr>>> {
        if present {
            Some(Some(self.expr_box()?))
        } else {
            Some(None)
        }
    }

    fn block(&mut self) -> Option<hir::Block> {
        match self.values.next()? {
            CloneValue::Block(block) => Some(block),
            _ => None,
        }
    }

    fn stmt(&mut self) -> Option<hir::Stmt> {
        match self.values.next()? {
            CloneValue::Stmt(stmt) => Some(stmt),
            _ => None,
        }
    }

    fn stmts(&mut self, count: usize) -> Option<Vec<hir::Stmt>> {
        (0..count).map(|_| self.stmt()).collect()
    }

    fn arm(&mut self) -> Option<hir::MatchArm> {
        match self.values.next()? {
            CloneValue::MatchArm(arm) => Some(arm),
            _ => None,
        }
    }

    fn arms(&mut self, count: usize) -> Option<Vec<hir::MatchArm>> {
        (0..count).map(|_| self.arm()).collect()
    }

    fn stage(&mut self) -> Option<hir::Stage> {
        match self.values.next()? {
            CloneValue::Stage(stage) => Some(stage),
            _ => None,
        }
    }

    fn stages(&mut self, count: usize) -> Option<Vec<hir::Stage>> {
        (0..count).map(|_| self.stage()).collect()
    }

    fn part(&mut self) -> Option<TemplatePart> {
        match self.values.next()? {
            CloneValue::TemplatePart(part) => Some(part),
            _ => None,
        }
    }

    fn parts(&mut self, count: usize) -> Option<Vec<TemplatePart>> {
        (0..count).map(|_| self.part()).collect()
    }

    fn has_no_remaining(mut self) -> bool {
        self.values.next().is_none()
    }
}

fn clone_match_arm(clones: &mut ChildValues, arm: &hir::MatchArm) -> Option<hir::MatchArm> {
    Some(hir::MatchArm {
        variants: arm.variants.clone(),
        bindings: arm.bindings.clone(),
        body: clones.expr()?,
    })
}

fn clone_stage(clones: &mut ChildValues, stage: &hir::Stage) -> Option<hir::Stage> {
    let kind = match &stage.kind {
        StageKind::Map { func, captures } => StageKind::Map {
            func: func.clone(),
            captures: clones.exprs(captures.len())?,
        },
        StageKind::Where { func, captures } => StageKind::Where {
            func: func.clone(),
            captures: clones.exprs(captures.len())?,
        },
        StageKind::WhereField { field } => StageKind::WhereField { field: *field },
        StageKind::WhereStrContains { .. } => StageKind::WhereStrContains {
            needle: clones.expr()?,
        },
        StageKind::Project { field } => StageKind::Project { field: *field },
    };
    Some(hir::Stage {
        kind,
        out_ty: stage.out_ty,
    })
}

fn clone_template_part(clones: &mut ChildValues, part: &TemplatePart) -> Option<TemplatePart> {
    Some(match part {
        TemplatePart::Text(text) => TemplatePart::Text(text.clone()),
        TemplatePart::Hole(_) => TemplatePart::Hole(clones.expr()?),
        TemplatePart::JsonStr(_) => TemplatePart::JsonStr(clones.expr()?),
        TemplatePart::OptionField { name, .. } => TemplatePart::OptionField {
            access: clones.expr()?,
            name: name.clone(),
        },
        TemplatePart::OptionStructField {
            name, struct_id, ..
        } => TemplatePart::OptionStructField {
            access: clones.expr()?,
            name: name.clone(),
            struct_id: *struct_id,
        },
        TemplatePart::PopComma => TemplatePart::PopComma,
        TemplatePart::StructArrayField { struct_id, .. } => TemplatePart::StructArrayField {
            access: clones.expr()?,
            struct_id: *struct_id,
        },
        TemplatePart::ScalarArrayField { elem, .. } => TemplatePart::ScalarArrayField {
            access: clones.expr()?,
            elem: *elem,
        },
        TemplatePart::UnionValue { enum_id, .. } => TemplatePart::UnionValue {
            access: clones.expr()?,
            enum_id: *enum_id,
        },
    })
}

fn clone_block(clones: &mut ChildValues, block: &hir::Block) -> Option<hir::Block> {
    let stmts = clones.stmts(block.stmts.len())?;
    let value = clones.optional_expr_box(block.value.is_some())?;
    Some(hir::Block { stmts, value })
}

fn clone_stmt(clones: &mut ChildValues, stmt: &Stmt) -> Option<Stmt> {
    Some(match stmt {
        Stmt::Let { local, .. } => Stmt::Let {
            local: *local,
            init: clones.expr()?,
        },
        Stmt::LetTuple {
            locals, tuple_id, ..
        } => Stmt::LetTuple {
            locals: locals.clone(),
            tuple_id: *tuple_id,
            init: clones.expr()?,
        },
        Stmt::Assign {
            local,
            drop_old,
            drop_new,
            ..
        } => Stmt::Assign {
            local: *local,
            value: clones.expr()?,
            drop_old: std::cell::Cell::new(drop_old.get()),
            drop_new: std::cell::Cell::new(drop_new.get()),
        },
        Stmt::AssignIndex { base, .. } => Stmt::AssignIndex {
            base: *base,
            index: clones.expr()?,
            value: clones.expr()?,
        },
        Stmt::AssignVecLane { local, lane, .. } => Stmt::AssignVecLane {
            local: *local,
            lane: *lane,
            value: clones.expr()?,
        },
        Stmt::AssignField { root, path, .. } => Stmt::AssignField {
            root: *root,
            path: path.clone(),
            value: clones.expr()?,
        },
        Stmt::AssignElemField {
            base,
            path,
            struct_id,
            soa,
            ..
        } => Stmt::AssignElemField {
            base: *base,
            index: clones.expr()?,
            path: path.clone(),
            struct_id: *struct_id,
            soa: *soa,
            value: clones.expr()?,
        },
        Stmt::AssignElem {
            base,
            struct_id,
            soa,
            ..
        } => Stmt::AssignElem {
            base: *base,
            index: clones.expr()?,
            struct_id: *struct_id,
            soa: *soa,
            value: clones.expr()?,
        },
        Stmt::Return(_) => Stmt::Return(if matches!(stmt, Stmt::Return(Some(_))) {
            Some(clones.expr()?)
        } else {
            None
        }),
        Stmt::Break { accepted, value } => Stmt::Break {
            value: if value.is_some() {
                Some(clones.expr()?)
            } else {
                None
            },
            accepted: *accepted,
        },
        Stmt::Expr(_) => Stmt::Expr(clones.expr()?),
    })
}

fn take_exprs<C: ChildCount>(clones: &mut ChildValues, count: C) -> Option<Vec<hir::Expr>> {
    clones.exprs(count.child_count())
}

fn take_boxed_expr(clones: &mut ChildValues) -> Option<Box<hir::Expr>> {
    clones.expr_box()
}

fn take_optional_boxed_expr(
    clones: &mut ChildValues,
    present: bool,
) -> Option<Option<Box<hir::Expr>>> {
    clones.optional_expr_box(present)
}

fn finish_children<T>(
    values: Vec<CloneValue>,
    build: impl FnOnce(&mut ChildValues) -> Option<T>,
) -> Option<T> {
    let mut children = ChildValues::new(values);
    let value = build(&mut children)?;
    if !children.has_no_remaining() {
        return None;
    }
    Some(value)
}

trait ChildCount {
    fn child_count(self) -> usize;
}

impl ChildCount for usize {
    fn child_count(self) -> usize {
        self
    }
}

impl ChildCount for &[hir::Expr] {
    fn child_count(self) -> usize {
        self.len()
    }
}

fn clone_expr_kind(clones: &mut ChildValues, kind: &ExprKind) -> Option<ExprKind> {
    macro_rules! boxed {
        ($expr:expr) => {{
            let _ = $expr;
            take_boxed_expr(clones)?
        }};
    }

    Some(match kind {
        // These variants contain no nested HIR records. Their derived clones are therefore
        // independent of body depth.
        ExprKind::Unit
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Local(_)
        | ExprKind::FnValue(_)
        | ExprKind::SqliteCallbackDescriptor { .. }
        | ExprKind::Wait
        | ExprKind::Field { .. }
        | ExprKind::SoaColumn { .. }
        | ExprKind::IndexField { .. }
        | ExprKind::OptionNone
        | ExprKind::ArrayGroupAgg { .. }
        | ExprKind::ArrayGroupAggMulti { .. }
        | ExprKind::ArrayDictEncode { .. }
        | ExprKind::ReaderStdin
        | ExprKind::WriterStd { .. }
        | ExprKind::TimeNow
        | ExprKind::TimeInstant
        | ExprKind::ProcessCpuCount
        | ExprKind::ProcessAbort
        | ExprKind::RandSeed
        | ExprKind::RawNull
        | ExprKind::HttpClient => kind.clone(),
        ExprKind::Unary { op, expr } => ExprKind::Unary {
            op: *op,
            expr: boxed!(expr),
        },
        ExprKind::Cast(expr) => ExprKind::Cast(boxed!(expr)),
        ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary {
            op: *op,
            lhs: boxed!(lhs),
            rhs: boxed!(rhs),
        },
        ExprKind::IntArith { op, mode, lhs, rhs } => ExprKind::IntArith {
            op: *op,
            mode: *mode,
            lhs: boxed!(lhs),
            rhs: boxed!(rhs),
        },
        ExprKind::MathOp { fn_, operands } => ExprKind::MathOp {
            fn_: *fn_,
            operands: take_exprs(clones, operands.len())?,
        },
        ExprKind::Closure { lifted, captures } => ExprKind::Closure {
            lifted: lifted.clone(),
            captures: take_exprs(clones, captures.len())?,
        },
        ExprKind::CallFnValue { callee, args } => ExprKind::CallFnValue {
            callee: boxed!(callee),
            args: take_exprs(clones, args.len())?,
        },
        ExprKind::TaskGroup(_) => ExprKind::TaskGroup(clones.block()?),
        ExprKind::EnumValue {
            enum_id,
            variant,
            payload,
        } => ExprKind::EnumValue {
            enum_id: *enum_id,
            variant: *variant,
            payload: take_exprs(clones, payload.len())?,
        },
        ExprKind::Match { scrutinee, arms } => ExprKind::Match {
            scrutinee: boxed!(scrutinee),
            arms: clones.arms(arms.len())?,
        },
        ExprKind::ResultMapErr { result, f } => ExprKind::ResultMapErr {
            result: boxed!(result),
            f: boxed!(f),
        },
        ExprKind::Spawn { closure, fallible } => ExprKind::Spawn {
            closure: boxed!(closure),
            fallible: *fallible,
        },
        ExprKind::TaskGet(expr) => ExprKind::TaskGet(boxed!(expr)),
        ExprKind::Call {
            func,
            args,
            type_args,
        } => ExprKind::Call {
            func: func.clone(),
            args: take_exprs(clones, args.len())?,
            type_args: type_args.clone(),
        },
        ExprKind::If { cond, .. } => ExprKind::If {
            cond: boxed!(cond),
            then: clones.block()?,
            els: clones.block()?,
        },
        ExprKind::StructLit { struct_id, fields } => ExprKind::StructLit {
            struct_id: *struct_id,
            fields: take_exprs(clones, fields.len())?,
        },
        ExprKind::Tuple { tuple_id, elems } => ExprKind::Tuple {
            tuple_id: *tuple_id,
            elems: take_exprs(clones, elems.len())?,
        },
        ExprKind::TupleIndex { recv, index } => ExprKind::TupleIndex {
            recv: boxed!(recv),
            index: *index,
        },
        ExprKind::Block(_) => ExprKind::Block(clones.block()?),
        ExprKind::OptionSome(expr) => ExprKind::OptionSome(boxed!(expr)),
        ExprKind::ElseUnwrap { opt, fallback } => ExprKind::ElseUnwrap {
            opt: boxed!(opt),
            fallback: boxed!(fallback),
        },
        ExprKind::ResultOk(expr) => ExprKind::ResultOk(boxed!(expr)),
        ExprKind::ResultErr(expr) => ExprKind::ResultErr(boxed!(expr)),
        ExprKind::Try(expr) => ExprKind::Try(boxed!(expr)),
        ExprKind::Loop {
            body: _,
            diverges,
            body_locals,
        } => ExprKind::Loop {
            body: clones.block()?,
            diverges: *diverges,
            body_locals: body_locals.clone(),
        },
        ExprKind::Arena(_) => ExprKind::Arena(clones.block()?),
        ExprKind::NamedArena { local, .. } => {
            ExprKind::NamedArena { local: *local, block: clones.block()? }
        }
        ExprKind::Unsafe(_) => ExprKind::Unsafe(clones.block()?),
        ExprKind::RawAlloc(expr) => ExprKind::RawAlloc(boxed!(expr)),
        ExprKind::RawFree(expr) => ExprKind::RawFree(boxed!(expr)),
        ExprKind::RawIsNull(expr) => ExprKind::RawIsNull(boxed!(expr)),
        ExprKind::RawLoad {
            ptr,
            offset,
            scalar,
        } => ExprKind::RawLoad {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
            scalar: *scalar,
        },
        ExprKind::RawPointerLoad { ptr, offset } => ExprKind::RawPointerLoad {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
        },
        ExprKind::StaticDescriptorView { ptr, offset } => ExprKind::StaticDescriptorView {
            ptr: boxed!(ptr),
            offset: *offset,
        },
        ExprKind::RawCall {
            guard,
            callee,
            args,
            param_tys,
            param_modes,
            return_borrow,
            return_region,
            return_cleanup,
        } => {
            let guard = match guard {
                Some(_) => Some(Box::new(clones.expr()?)),
                None => None,
            };
            ExprKind::RawCall {
                guard,
                callee: boxed!(callee),
                args: take_exprs(clones, args.len())?,
                param_tys: param_tys.clone(),
                param_modes: param_modes.clone(),
                return_borrow: return_borrow.clone(),
                return_region: return_region.clone(),
                return_cleanup: *return_cleanup,
            }
        }
        ExprKind::RawStore { ptr, offset, value } => ExprKind::RawStore {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
            value: boxed!(value),
        },
        ExprKind::RawOffset { ptr, offset } => ExprKind::RawOffset {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
        },
        ExprKind::ResourceFromRaw {
            raw, resource, parent,
        } => ExprKind::ResourceFromRaw {
            raw: boxed!(raw),
            resource: *resource,
            parent: take_optional_boxed_expr(clones, parent.is_some())?,
        },
        ExprKind::ResourceBorrow { owner, resource } => ExprKind::ResourceBorrow {
            owner: boxed!(owner),
            resource: *resource,
        },
        ExprKind::ResourceRaw { reference, resource } => ExprKind::ResourceRaw {
            reference: boxed!(reference),
            resource: *resource,
        },
        ExprKind::ResourceIntoRaw { owner, resource } => ExprKind::ResourceIntoRaw {
            owner: boxed!(owner),
            resource: *resource,
        },
        ExprKind::ResourceViewFromRaw { owner, ptr, len, resource, view } => {
            ExprKind::ResourceViewFromRaw {
                owner: boxed!(owner),
                ptr: boxed!(ptr),
                len: boxed!(len),
                resource: *resource,
                view: *view,
            }
        }
        ExprKind::HeapNew(expr) => ExprKind::HeapNew(boxed!(expr)),
        ExprKind::BoxGet(expr) => ExprKind::BoxGet(boxed!(expr)),
        ExprKind::BoxClone(expr) => ExprKind::BoxClone(boxed!(expr)),
        ExprKind::StrClone(expr) => ExprKind::StrClone(boxed!(expr)),
        ExprKind::CloneIn { value, region } => ExprKind::CloneIn {
            value: boxed!(value),
            region: boxed!(region),
        },
        ExprKind::StrPredicate {
            kind,
            haystack,
            needle,
        } => ExprKind::StrPredicate {
            kind: *kind,
            haystack: boxed!(haystack),
            needle: boxed!(needle),
        },
        ExprKind::StrTrim { kind, recv } => ExprKind::StrTrim {
            kind: *kind,
            recv: boxed!(recv),
        },
        ExprKind::StrBorrow(expr) => ExprKind::StrBorrow(boxed!(expr)),
        ExprKind::ArrayBuilderNew { elem, region } => ExprKind::ArrayBuilderNew {
            elem: *elem,
            region: take_optional_boxed_expr(clones, region.is_some())?,
        },
        ExprKind::BuilderNew { capacity } => ExprKind::BuilderNew {
            capacity: take_optional_boxed_expr(clones, capacity.is_some())?,
        },
        ExprKind::BuilderWrite { builder, arg, kind } => ExprKind::BuilderWrite {
            builder: boxed!(builder),
            arg: boxed!(arg),
            kind: *kind,
        },
        ExprKind::BuilderToString(expr) => ExprKind::BuilderToString(boxed!(expr)),
        ExprKind::ArrayLit {
            elems,
            elem,
            pooled,
        } => ExprKind::ArrayLit {
            elems: take_exprs(clones, elems.len())?,
            elem: *elem,
            pooled: *pooled,
        },
        ExprKind::ConstArray { elems, elem, len } => ExprKind::ConstArray {
            elems: take_exprs(clones, elems.len())?,
            elem: *elem,
            len: *len,
        },
        ExprKind::ArrayZip { sources, tuple_id } => ExprKind::ArrayZip {
            sources: take_exprs(clones, sources.len())?,
            tuple_id: *tuple_id,
        },
        ExprKind::Select { mask, a, b } => ExprKind::Select {
            mask: boxed!(mask),
            a: boxed!(a),
            b: boxed!(b),
        },
        ExprKind::VecSumWhere { vec, mask } => ExprKind::VecSumWhere {
            vec: boxed!(vec),
            mask: boxed!(mask),
        },
        ExprKind::VecDot { a, b } => ExprKind::VecDot {
            a: boxed!(a),
            b: boxed!(b),
        },
        ExprKind::VecMinMax { vec, max } => ExprKind::VecMinMax {
            vec: boxed!(vec),
            max: *max,
        },
        ExprKind::VecSum { vec } => ExprKind::VecSum { vec: boxed!(vec) },
        ExprKind::VecLoad {
            src,
            index,
            elem,
            n,
        } => ExprKind::VecLoad {
            src: boxed!(src),
            index: boxed!(index),
            elem: *elem,
            n: *n,
        },
        ExprKind::VecStore {
            dst,
            index,
            value,
            elem,
            n,
        } => ExprKind::VecStore {
            dst: boxed!(dst),
            index: boxed!(index),
            value: boxed!(value),
            elem: *elem,
            n: *n,
        },
        ExprKind::VecLit { elems, elem } => ExprKind::VecLit {
            elems: take_exprs(clones, elems.len())?,
            elem: *elem,
        },
        ExprKind::ArraySum { source, stages } => ExprKind::ArraySum {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
        },
        ExprKind::ArrayCount { source, stages } => ExprKind::ArrayCount {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
        },
        ExprKind::ArrayAnyAll {
            source,
            stages,
            func,
            captures,
            all,
        } => ExprKind::ArrayAnyAll {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            func: func.clone(),
            captures: take_exprs(clones, captures.len())?,
            all: *all,
        },
        ExprKind::ArrayMinMax {
            source,
            stages,
            is_max,
        } => ExprKind::ArrayMinMax {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            is_max: *is_max,
        },
        ExprKind::ArrayReduce {
            source,
            stages,
            func,
            captures,
            init,
        } => ExprKind::ArrayReduce {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            func: func.clone(),
            captures: take_exprs(clones, captures.len())?,
            init: boxed!(init),
        },
        ExprKind::ArrayScan {
            source,
            stages,
            func,
            captures,
            init,
            elem,
        } => ExprKind::ArrayScan {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            func: func.clone(),
            captures: take_exprs(clones, captures.len())?,
            init: boxed!(init),
            elem: *elem,
        },
        ExprKind::ArrayDot { a, b, elem } => ExprKind::ArrayDot {
            a: boxed!(a),
            b: boxed!(b),
            elem: *elem,
        },
        ExprKind::ArraySort {
            source,
            stages,
            elem,
        } => ExprKind::ArraySort {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            elem: *elem,
        },
        ExprKind::ArraySortBy {
            source,
            stages,
            key_func,
            captures,
            key_ty,
            elem,
        } => ExprKind::ArraySortBy {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            key_func: key_func.clone(),
            captures: take_exprs(clones, captures.len())?,
            key_ty: *key_ty,
            elem: *elem,
        },
        ExprKind::ArrayToArray {
            source,
            stages,
            elem,
        } => ExprKind::ArrayToArray {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            elem: *elem,
        },
        ExprKind::ArrayToSoa { source, struct_id } => ExprKind::ArrayToSoa {
            source: boxed!(source),
            struct_id: *struct_id,
        },
        ExprKind::ArrayMapInto {
            source,
            stages,
            dst,
            elem,
        } => ExprKind::ArrayMapInto {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            dst: boxed!(dst),
            elem: *elem,
        },
        ExprKind::ArrayPartition {
            source,
            stages,
            func,
            captures,
            elem,
        } => ExprKind::ArrayPartition {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            func: func.clone(),
            captures: take_exprs(clones, captures.len())?,
            elem: *elem,
        },
        ExprKind::ArrayParMap {
            source,
            stages,
            func,
            captures,
            elem,
        } => ExprKind::ArrayParMap {
            source: boxed!(source),
            stages: clones.stages(stages.len())?,
            func: func.clone(),
            captures: take_exprs(clones, captures.len())?,
            elem: *elem,
        },
        ExprKind::ArrayChunks { source, n, elem } => ExprKind::ArrayChunks {
            source: boxed!(source),
            n: boxed!(n),
            elem: *elem,
        },
        ExprKind::ArrayToSlice(expr) => ExprKind::ArrayToSlice(boxed!(expr)),
        ExprKind::Len(expr) => ExprKind::Len(boxed!(expr)),
        ExprKind::Index { recv, index } => ExprKind::Index {
            recv: boxed!(recv),
            index: boxed!(index),
        },
        ExprKind::SliceRange { recv, start, end } => ExprKind::SliceRange {
            recv: boxed!(recv),
            start: take_optional_boxed_expr(clones, start.is_some())?,
            end: take_optional_boxed_expr(clones, end.is_some())?,
        },
        ExprKind::ElemField {
            recv,
            index,
            path,
            struct_id,
        } => ExprKind::ElemField {
            recv: boxed!(recv),
            index: boxed!(index),
            path: path.clone(),
            struct_id: *struct_id,
        },
        ExprKind::Template(parts) => ExprKind::Template(clones.parts(parts.len())?),
        ExprKind::JsonDecode { struct_id, input } => ExprKind::JsonDecode {
            struct_id: *struct_id,
            input: boxed!(input),
        },
        ExprKind::JsonDecodeArray { elem, input } => ExprKind::JsonDecodeArray {
            elem: *elem,
            input: boxed!(input),
        },
        ExprKind::JsonDecodeScalar { scalar, input } => ExprKind::JsonDecodeScalar {
            scalar: *scalar,
            input: boxed!(input),
        },
        ExprKind::JsonDecodeStructArray { struct_id, input } => ExprKind::JsonDecodeStructArray {
            struct_id: *struct_id,
            input: boxed!(input),
        },
        ExprKind::JsonDecodeSoa { struct_id, input } => ExprKind::JsonDecodeSoa {
            struct_id: *struct_id,
            input: boxed!(input),
        },
        ExprKind::JsonDecodeUnion { enum_id, input } => ExprKind::JsonDecodeUnion {
            enum_id: *enum_id,
            input: boxed!(input),
        },
        ExprKind::JsonDoc { input } => ExprKind::JsonDoc {
            input: boxed!(input),
        },
        ExprKind::JsonDocKind { doc } => ExprKind::JsonDocKind { doc: boxed!(doc) },
        ExprKind::JsonDocGet { doc, key } => ExprKind::JsonDocGet {
            doc: boxed!(doc),
            key: boxed!(key),
        },
        ExprKind::JsonDocAt { doc, index } => ExprKind::JsonDocAt {
            doc: boxed!(doc),
            index: boxed!(index),
        },
        ExprKind::JsonDocAsStr { doc } => ExprKind::JsonDocAsStr { doc: boxed!(doc) },
        ExprKind::JsonDocAsScalar { doc, scalar } => ExprKind::JsonDocAsScalar {
            doc: boxed!(doc),
            scalar: *scalar,
        },
        ExprKind::JsonDocLen { doc } => ExprKind::JsonDocLen { doc: boxed!(doc) },
        ExprKind::JsonDocKey { doc, index } => ExprKind::JsonDocKey {
            doc: boxed!(doc),
            index: boxed!(index),
        },
        ExprKind::JsonDocElems { doc } => ExprKind::JsonDocElems { doc: boxed!(doc) },
        ExprKind::JsonScan { struct_id, input } => ExprKind::JsonScan {
            struct_id: *struct_id,
            input: boxed!(input),
        },
        ExprKind::FsReadFile { path } => ExprKind::FsReadFile { path: boxed!(path) },
        ExprKind::ReaderOpen { path } => ExprKind::ReaderOpen { path: boxed!(path) },
        ExprKind::WriterCreate { path } => ExprKind::WriterCreate { path: boxed!(path) },
        ExprKind::ReaderRead { reader, buffer } => ExprKind::ReaderRead {
            reader: boxed!(reader),
            buffer: boxed!(buffer),
        },
        ExprKind::ReaderBuffered { reader } => ExprKind::ReaderBuffered {
            reader: boxed!(reader),
        },
        ExprKind::ReaderReadLine { reader, buffer } => ExprKind::ReaderReadLine {
            reader: boxed!(reader),
            buffer: boxed!(buffer),
        },
        ExprKind::BytesAsStr { bytes } => ExprKind::BytesAsStr {
            bytes: boxed!(bytes),
        },
        ExprKind::WriterWrite {
            writer,
            arg,
            builder,
        } => ExprKind::WriterWrite {
            writer: boxed!(writer),
            arg: boxed!(arg),
            builder: *builder,
        },
        ExprKind::WriterFlush { writer } => ExprKind::WriterFlush {
            writer: boxed!(writer),
        },
        ExprKind::IoCopy { reader, writer } => ExprKind::IoCopy {
            reader: boxed!(reader),
            writer: boxed!(writer),
        },
        ExprKind::FileCreateRw { path } => ExprKind::FileCreateRw { path: boxed!(path) },
        ExprKind::FileOpenRw { path } => ExprKind::FileOpenRw { path: boxed!(path) },
        ExprKind::FilePread {
            file,
            buffer,
            offset,
        } => ExprKind::FilePread {
            file: boxed!(file),
            buffer: boxed!(buffer),
            offset: boxed!(offset),
        },
        ExprKind::FilePwrite { file, data, offset } => ExprKind::FilePwrite {
            file: boxed!(file),
            data: boxed!(data),
            offset: boxed!(offset),
        },
        ExprKind::FileLen { file } => ExprKind::FileLen { file: boxed!(file) },
        ExprKind::BufferNew { capacity } => ExprKind::BufferNew {
            capacity: boxed!(capacity),
        },
        ExprKind::BufferBytes { buffer } => ExprKind::BufferBytes {
            buffer: boxed!(buffer),
        },
        ExprKind::StrBytes { inner } => ExprKind::StrBytes {
            inner: boxed!(inner),
        },
        ExprKind::BufferLen { buffer } => ExprKind::BufferLen {
            buffer: boxed!(buffer),
        },
        ExprKind::BytesRead { bytes, offset, be } => ExprKind::BytesRead {
            bytes: boxed!(bytes),
            offset: boxed!(offset),
            be: *be,
        },
        ExprKind::BufferPut { buffer, value, be } => ExprKind::BufferPut {
            buffer: boxed!(buffer),
            value: boxed!(value),
            be: *be,
        },
        ExprKind::BufferAppend { buffer, data } => ExprKind::BufferAppend {
            buffer: boxed!(buffer),
            data: boxed!(data),
        },
        ExprKind::ArrayBuilderPush {
            builder,
            value,
            moves_value,
        } => ExprKind::ArrayBuilderPush {
            builder: boxed!(builder),
            value: boxed!(value),
            moves_value: *moves_value,
        },
        ExprKind::ArrayBuilderAppend { builder, data } => ExprKind::ArrayBuilderAppend {
            builder: boxed!(builder),
            data: boxed!(data),
        },
        ExprKind::ArrayBuilderBuild(expr) => ExprKind::ArrayBuilderBuild(boxed!(expr)),
        ExprKind::FsWriteFile {
            path,
            data,
            builder,
        } => ExprKind::FsWriteFile {
            path: boxed!(path),
            data: boxed!(data),
            builder: *builder,
        },
        ExprKind::FsExists { path } => ExprKind::FsExists { path: boxed!(path) },
        ExprKind::FsRemove { path } => ExprKind::FsRemove { path: boxed!(path) },
        ExprKind::FsReadDir { path } => ExprKind::FsReadDir { path: boxed!(path) },
        ExprKind::DnsResolve { host } => ExprKind::DnsResolve { host: boxed!(host) },
        ExprKind::TcpConnect { host, port } => ExprKind::TcpConnect {
            host: boxed!(host),
            port: boxed!(port),
        },
        ExprKind::ConnReader { conn } => ExprKind::ConnReader { conn: boxed!(conn) },
        ExprKind::ConnWriter { conn } => ExprKind::ConnWriter { conn: boxed!(conn) },
        ExprKind::TcpReadTimeout { conn, ns } => ExprKind::TcpReadTimeout {
            conn: boxed!(conn),
            ns: boxed!(ns),
        },
        ExprKind::TcpWriteTimeout { conn, ns } => ExprKind::TcpWriteTimeout {
            conn: boxed!(conn),
            ns: boxed!(ns),
        },
        ExprKind::TcpListen { host, port } => ExprKind::TcpListen {
            host: boxed!(host),
            port: boxed!(port),
        },
        ExprKind::TcpAccept { listener } => ExprKind::TcpAccept {
            listener: boxed!(listener),
        },
        ExprKind::UdpBind { host, port } => ExprKind::UdpBind {
            host: boxed!(host),
            port: boxed!(port),
        },
        ExprKind::UdpSendTo {
            sock,
            data,
            host,
            port,
        } => ExprKind::UdpSendTo {
            sock: boxed!(sock),
            data: boxed!(data),
            host: boxed!(host),
            port: boxed!(port),
        },
        ExprKind::UdpRecvFrom { sock, buffer } => ExprKind::UdpRecvFrom {
            sock: boxed!(sock),
            buffer: boxed!(buffer),
        },
        ExprKind::FsReadFileView { path } => ExprKind::FsReadFileView { path: boxed!(path) },
        ExprKind::FsReadBytesView { path } => ExprKind::FsReadBytesView { path: boxed!(path) },
        ExprKind::PathJoin { a, b } => ExprKind::PathJoin {
            a: boxed!(a),
            b: boxed!(b),
        },
        ExprKind::PathComponent { kind, path } => ExprKind::PathComponent {
            kind: *kind,
            path: boxed!(path),
        },
        ExprKind::PathNormalize { path } => ExprKind::PathNormalize { path: boxed!(path) },
        ExprKind::EnvGet { name } => ExprKind::EnvGet { name: boxed!(name) },
        ExprKind::EnvSet { name, value } => ExprKind::EnvSet {
            name: boxed!(name),
            value: boxed!(value),
        },
        ExprKind::TimeSleep { ns } => ExprKind::TimeSleep { ns: boxed!(ns) },
        ExprKind::ProcessExit { code } => ExprKind::ProcessExit { code: boxed!(code) },
        ExprKind::ProcessSpawn { cmd, args } => ExprKind::ProcessSpawn {
            cmd: boxed!(cmd),
            args: boxed!(args),
        },
        ExprKind::ChildWait { child } => ExprKind::ChildWait {
            child: boxed!(child),
        },
        ExprKind::ChildKill { child, sig } => ExprKind::ChildKill {
            child: boxed!(child),
            sig: boxed!(sig),
        },
        ExprKind::ProcessExec { cmd, args } => ExprKind::ProcessExec {
            cmd: boxed!(cmd),
            args: boxed!(args),
        },
        ExprKind::ProcessCommand { cmd, args } => ExprKind::ProcessCommand {
            cmd: boxed!(cmd),
            args: boxed!(args),
        },
        ExprKind::CommandCwd { command, dir } => ExprKind::CommandCwd {
            command: boxed!(command),
            dir: boxed!(dir),
        },
        ExprKind::CommandTimeout { command, ns } => ExprKind::CommandTimeout {
            command: boxed!(command),
            ns: boxed!(ns),
        },
        ExprKind::CommandEnv {
            command,
            name,
            value,
        } => ExprKind::CommandEnv {
            command: boxed!(command),
            name: boxed!(name),
            value: boxed!(value),
        },
        ExprKind::CommandEnvClear { command } => ExprKind::CommandEnvClear {
            command: boxed!(command),
        },
        ExprKind::CommandRun { command } => ExprKind::CommandRun {
            command: boxed!(command),
        },
        ExprKind::RunOutputCode { out } => ExprKind::RunOutputCode { out: boxed!(out) },
        ExprKind::RunOutputStdout { out } => ExprKind::RunOutputStdout { out: boxed!(out) },
        ExprKind::RunOutputStderr { out } => ExprKind::RunOutputStderr { out: boxed!(out) },
        ExprKind::EncodingEncode { kind, data } => ExprKind::EncodingEncode {
            kind: *kind,
            data: boxed!(data),
        },
        ExprKind::EncodingDecode { kind, input } => ExprKind::EncodingDecode {
            kind: *kind,
            input: boxed!(input),
        },
        ExprKind::Utf8Valid { data } => ExprKind::Utf8Valid { data: boxed!(data) },
        ExprKind::Compress { kind, data, level } => ExprKind::Compress {
            kind: *kind,
            data: boxed!(data),
            level: boxed!(level),
        },
        ExprKind::Decompress { kind, data } => ExprKind::Decompress {
            kind: *kind,
            data: boxed!(data),
        },
        ExprKind::RandSeedWith { seed } => ExprKind::RandSeedWith { seed: boxed!(seed) },
        ExprKind::RandNext { rng } => ExprKind::RandNext { rng: boxed!(rng) },
        ExprKind::RandRange { rng, lo, hi } => ExprKind::RandRange {
            rng: boxed!(rng),
            lo: boxed!(lo),
            hi: boxed!(hi),
        },
        ExprKind::RandShuffle { rng, xs, elem } => ExprKind::RandShuffle {
            rng: boxed!(rng),
            xs: boxed!(xs),
            elem: *elem,
        },
        ExprKind::RandSample { rng, xs, k, elem } => ExprKind::RandSample {
            rng: boxed!(rng),
            xs: boxed!(xs),
            k: boxed!(k),
            elem: *elem,
        },
        ExprKind::RegexCompile { pattern } => ExprKind::RegexCompile {
            pattern: boxed!(pattern),
        },
        ExprKind::RegexIsMatch { regex, text } => ExprKind::RegexIsMatch {
            regex: boxed!(regex),
            text: boxed!(text),
        },
        ExprKind::RegexFind { regex, text, start } => ExprKind::RegexFind {
            regex: boxed!(regex),
            text: boxed!(text),
            start: take_optional_boxed_expr(clones, start.is_some())?,
        },
        ExprKind::RegexFindAll { regex, text } => ExprKind::RegexFindAll {
            regex: boxed!(regex),
            text: boxed!(text),
        },
        ExprKind::RegexSplit { regex, text } => ExprKind::RegexSplit {
            regex: boxed!(regex),
            text: boxed!(text),
        },
        ExprKind::RegexReplace {
            regex,
            text,
            repl,
            all,
        } => ExprKind::RegexReplace {
            regex: boxed!(regex),
            text: boxed!(text),
            repl: boxed!(repl),
            all: *all,
        },
        ExprKind::RegexCaptures { regex, text } => ExprKind::RegexCaptures {
            regex: boxed!(regex),
            text: boxed!(text),
        },
        ExprKind::RegexGroupCount { regex } => ExprKind::RegexGroupCount {
            regex: boxed!(regex),
        },
        ExprKind::RegexGroupIndex { regex, name } => ExprKind::RegexGroupIndex {
            regex: boxed!(regex),
            name: boxed!(name),
        },
        ExprKind::CapturesGroup { caps, index } => ExprKind::CapturesGroup {
            caps: boxed!(caps),
            index: boxed!(index),
        },
        ExprKind::CliCommand { name } => ExprKind::CliCommand { name: boxed!(name) },
        ExprKind::CliFlag {
            cmd,
            kind,
            name,
            default,
        } => ExprKind::CliFlag {
            cmd: boxed!(cmd),
            kind: *kind,
            name: boxed!(name),
            default: take_optional_boxed_expr(clones, default.is_some())?,
        },
        ExprKind::CliParse { cmd, args } => ExprKind::CliParse {
            cmd: boxed!(cmd),
            args: boxed!(args),
        },
        ExprKind::CliGetBool { parsed, name } => ExprKind::CliGetBool {
            parsed: boxed!(parsed),
            name: boxed!(name),
        },
        ExprKind::CliGetI64 { parsed, name } => ExprKind::CliGetI64 {
            parsed: boxed!(parsed),
            name: boxed!(name),
        },
        ExprKind::CliGetStr { parsed, name } => ExprKind::CliGetStr {
            parsed: boxed!(parsed),
            name: boxed!(name),
        },
        ExprKind::CliUsage { cmd } => ExprKind::CliUsage { cmd: boxed!(cmd) },
        ExprKind::HttpRequest { method, url } => ExprKind::HttpRequest {
            method: boxed!(method),
            url: boxed!(url),
        },
        ExprKind::HttpHeader { req, name, value } => ExprKind::HttpHeader {
            req: boxed!(req),
            name: boxed!(name),
            value: boxed!(value),
        },
        ExprKind::HttpBody { req, data } => ExprKind::HttpBody {
            req: boxed!(req),
            data: boxed!(data),
        },
        ExprKind::HttpRequestTimeout { req, ns } => ExprKind::HttpRequestTimeout {
            req: boxed!(req),
            ns: boxed!(ns),
        },
        ExprKind::HttpParse { data } => ExprKind::HttpParse { data: boxed!(data) },
        ExprKind::HttpRespStatus { resp } => ExprKind::HttpRespStatus { resp: boxed!(resp) },
        ExprKind::HttpRespHeader { resp, name } => ExprKind::HttpRespHeader {
            resp: boxed!(resp),
            name: boxed!(name),
        },
        ExprKind::HttpRespBody { resp } => ExprKind::HttpRespBody { resp: boxed!(resp) },
        ExprKind::HttpClientTimeout { client, ns } => ExprKind::HttpClientTimeout {
            client: boxed!(client),
            ns: boxed!(ns),
        },
        ExprKind::HttpClientGet { client, url } => ExprKind::HttpClientGet {
            client: boxed!(client),
            url: boxed!(url),
        },
        ExprKind::HttpClientPost { client, url, body } => ExprKind::HttpClientPost {
            client: boxed!(client),
            url: boxed!(url),
            body: boxed!(body),
        },
        ExprKind::HttpClientRequest { client, req } => ExprKind::HttpClientRequest {
            client: boxed!(client),
            req: boxed!(req),
        },
        ExprKind::HttpGetMany {
            client,
            urls,
            max_concurrency,
        } => ExprKind::HttpGetMany {
            client: boxed!(client),
            urls: boxed!(urls),
            max_concurrency: boxed!(max_concurrency),
        },
        ExprKind::HttpServe { host, port, shared } => ExprKind::HttpServe {
            host: boxed!(host),
            port: boxed!(port),
            shared: *shared,
        },
        ExprKind::HttpAccept { server } => ExprKind::HttpAccept {
            server: boxed!(server),
        },
        ExprKind::HttpCtxMethod { ctx } => ExprKind::HttpCtxMethod { ctx: boxed!(ctx) },
        ExprKind::HttpCtxPath { ctx } => ExprKind::HttpCtxPath { ctx: boxed!(ctx) },
        ExprKind::HttpCtxHeaders { ctx } => ExprKind::HttpCtxHeaders { ctx: boxed!(ctx) },
        ExprKind::HttpCtxHeader { headers, name } => ExprKind::HttpCtxHeader {
            headers: boxed!(headers),
            name: boxed!(name),
        },
        ExprKind::HttpCtxBody { ctx } => ExprKind::HttpCtxBody { ctx: boxed!(ctx) },
        ExprKind::HttpResponseBuilder { status } => ExprKind::HttpResponseBuilder {
            status: boxed!(status),
        },
        ExprKind::HttpRbHeader { rb, name, value } => ExprKind::HttpRbHeader {
            rb: boxed!(rb),
            name: boxed!(name),
            value: boxed!(value),
        },
        ExprKind::HttpRbBody { rb, data } => ExprKind::HttpRbBody {
            rb: boxed!(rb),
            data: boxed!(data),
        },
        ExprKind::HttpRespond { ctx, rb } => ExprKind::HttpRespond {
            ctx: boxed!(ctx),
            rb: boxed!(rb),
        },
        ExprKind::HttpRespondStream { ctx, rb } => ExprKind::HttpRespondStream {
            ctx: boxed!(ctx),
            rb: boxed!(rb),
        },
        ExprKind::HttpStreamSend {
            stream,
            chunk,
            event,
        } => ExprKind::HttpStreamSend {
            stream: boxed!(stream),
            chunk: boxed!(chunk),
            event: *event,
        },
        ExprKind::HttpStreamFinish { stream } => ExprKind::HttpStreamFinish {
            stream: boxed!(stream),
        },
        ExprKind::HttpStreamReject { stream, rb } => ExprKind::HttpStreamReject {
            stream: boxed!(stream),
            rb: boxed!(rb),
        },
        ExprKind::CryptoCtEqual { a, b } => ExprKind::CryptoCtEqual {
            a: boxed!(a),
            b: boxed!(b),
        },
        ExprKind::CryptoRandom { out } => ExprKind::CryptoRandom { out: boxed!(out) },
        ExprKind::CryptoHash { algo, data } => ExprKind::CryptoHash {
            algo: *algo,
            data: boxed!(data),
        },
        ExprKind::CryptoHmac { key, data } => ExprKind::CryptoHmac {
            key: boxed!(key),
            data: boxed!(data),
        },
        ExprKind::CryptoHkdf {
            salt,
            ikm,
            info,
            len,
        } => ExprKind::CryptoHkdf {
            salt: boxed!(salt),
            ikm: boxed!(ikm),
            info: boxed!(info),
            len: boxed!(len),
        },
        ExprKind::CryptoAead {
            cipher,
            dir,
            key,
            nonce,
            input,
            aad,
        } => ExprKind::CryptoAead {
            cipher: *cipher,
            dir: *dir,
            key: boxed!(key),
            nonce: boxed!(nonce),
            input: boxed!(input),
            aad: boxed!(aad),
        },
        ExprKind::CryptoArgon2 {
            password,
            salt,
            params,
        } => ExprKind::CryptoArgon2 {
            password: boxed!(password),
            salt: boxed!(salt),
            params: boxed!(params),
        },
    })
}

enum CloneFrame<'a> {
    Block {
        id: usize,
        source: &'a hir::Block,
        values: Vec<CloneValue>,
    },
    Stmt {
        id: usize,
        source: &'a Stmt,
        values: Vec<CloneValue>,
    },
    Expr {
        id: usize,
        source: &'a hir::Expr,
        values: Vec<CloneValue>,
    },
    MatchArm {
        id: usize,
        source: &'a hir::MatchArm,
        values: Vec<CloneValue>,
    },
    Stage {
        id: usize,
        source: &'a hir::Stage,
        values: Vec<CloneValue>,
    },
    TemplatePart {
        id: usize,
        source: &'a TemplatePart,
        values: Vec<CloneValue>,
    },
}

impl<'a> CloneFrame<'a> {
    fn new(id: usize, record: crate::hir_depth::BodyRecord<'a>) -> Option<Self> {
        let values = Vec::new();
        Some(match record {
            crate::hir_depth::BodyRecord::Block(source) => Self::Block { id, source, values },
            crate::hir_depth::BodyRecord::Stmt(source) => Self::Stmt { id, source, values },
            crate::hir_depth::BodyRecord::Expr(source) => Self::Expr { id, source, values },
            crate::hir_depth::BodyRecord::MatchArm { arm: source, .. } => {
                Self::MatchArm { id, source, values }
            }
            crate::hir_depth::BodyRecord::Stage(source) => Self::Stage { id, source, values },
            crate::hir_depth::BodyRecord::TemplatePart(source) => {
                Self::TemplatePart { id, source, values }
            }
            crate::hir_depth::BodyRecord::BlockExit { .. }
            | crate::hir_depth::BodyRecord::StmtExit { .. }
            | crate::hir_depth::BodyRecord::ExprExit { .. }
            | crate::hir_depth::BodyRecord::MatchArmExit { .. }
            | crate::hir_depth::BodyRecord::StageExit { .. }
            | crate::hir_depth::BodyRecord::TemplatePartExit { .. } => return None,
        })
    }

    fn id(&self) -> usize {
        match self {
            Self::Block { id, .. }
            | Self::Stmt { id, .. }
            | Self::Expr { id, .. }
            | Self::MatchArm { id, .. }
            | Self::Stage { id, .. }
            | Self::TemplatePart { id, .. } => *id,
        }
    }

    fn push(&mut self, value: CloneValue) {
        match self {
            Self::Block { values, .. }
            | Self::Stmt { values, .. }
            | Self::Expr { values, .. }
            | Self::MatchArm { values, .. }
            | Self::Stage { values, .. }
            | Self::TemplatePart { values, .. } => values.push(value),
        }
    }

    fn finish(self) -> Option<CloneValue> {
        match self {
            Self::Block { source, values, .. } => {
                Some(CloneValue::Block(finish_children(values, |children| {
                    clone_block(children, source)
                })?))
            }
            Self::Stmt { source, values, .. } => {
                Some(CloneValue::Stmt(finish_children(values, |children| {
                    clone_stmt(children, source)
                })?))
            }
            Self::Expr { source, values, .. } => {
                Some(CloneValue::Expr(finish_children(values, |children| {
                    Some(hir::Expr {
                        kind: clone_expr_kind(children, &source.kind)?,
                        ty: source.ty,
                        span: source.span,
                    })
                })?))
            }
            Self::MatchArm { source, values, .. } => {
                Some(CloneValue::MatchArm(finish_children(values, |children| {
                    clone_match_arm(children, source)
                })?))
            }
            Self::Stage { source, values, .. } => {
                Some(CloneValue::Stage(finish_children(values, |children| {
                    clone_stage(children, source)
                })?))
            }
            Self::TemplatePart { source, values, .. } => Some(CloneValue::TemplatePart(
                finish_children(values, |children| clone_template_part(children, source))?,
            )),
        }
    }
}

fn clone_function(function: &hir::Fn) -> Option<hir::Fn> {
    let events = crate::hir_depth::clone_events(&function.body)?;
    let mut frames = Vec::new();
    let mut root = None;
    for event in events {
        match event {
            crate::hir_depth::CloneEvent::RecordEnter { id, record } => {
                frames.push(CloneFrame::new(id, record)?);
            }
            crate::hir_depth::CloneEvent::RecordExit { id } => {
                let frame = frames.pop()?;
                if frame.id() != id {
                    return None;
                }
                let value = frame.finish()?;
                if let Some(parent) = frames.last_mut() {
                    parent.push(value);
                } else if root.replace(value).is_some() {
                    return None;
                }
            }
        }
    }
    if !frames.is_empty() {
        return None;
    }
    let body = match root {
        Some(CloneValue::Block(body)) => body,
        Some(CloneValue::Expr(_))
        | Some(CloneValue::Stmt(_))
        | Some(CloneValue::MatchArm(_))
        | Some(CloneValue::Stage(_))
        | Some(CloneValue::TemplatePart(_))
        | None => return None,
    };
    Some(hir::Fn {
        name: function.name.clone(),
        origin: function.origin,
        params: function.params.clone(),
        param_modes: function.param_modes.clone(),
        ret: function.ret,
        return_borrow: function.return_borrow.clone(),
        return_region: function.return_region.clone(),
        return_cleanup: function.return_cleanup,
        locals: function.locals.clone(),
        body,
        span: function.span,
        drop_locals: function.drop_locals.clone(),
        drop_individual_locals: function.drop_individual_locals.clone(),
        drop_individual_exprs: function.drop_individual_exprs.clone(),
    })
}

pub(crate) fn clone_program(program: &hir::Program) -> Option<hir::Program> {
    let mut fns = Vec::with_capacity(program.fns.len());
    for function in &program.fns {
        let Some(function) = clone_function(function) else {
            drop_functions(fns);
            return None;
        };
        fns.push(function);
    }
    Some(hir::Program {
        fns,
        externs: program.externs.clone(),
        link_libs: program.link_libs.clone(),
        structs: program.structs.clone(),
        enums: program.enums.clone(),
        resources: program.resources.clone(),
        tagged_types: program.tagged_types.clone(),
        tuples: program.tuples.clone(),
        fn_types: program.fn_types.clone(),
        imported_fns: program.imported_fns.clone(),
    })
}

enum DropWork {
    Function(hir::Fn),
    Block(hir::Block),
    Stmt(hir::Stmt),
    Expr(hir::Expr),
    MatchArm(hir::MatchArm),
    Stage(hir::Stage),
    TemplatePart(TemplatePart),
}

/// Drop a cloned HIR tree without following its recursive `Box`/`Vec` shape on the native stack.
///
/// This is deliberately separate from `clone_program`: the replay boundary must contain the
/// teardown of a successful replay, a rejected replay, and a replay whose analysis panics. Every
/// child-bearing HIR variant is listed in `drop_expr_kind`; adding one without adding its teardown
/// path is therefore a compile-time review point in the same file as reconstruction.
pub(crate) fn drop_program(program: hir::Program) {
    let hir::Program {
        fns,
        externs,
        link_libs,
        structs,
        enums,
        resources,
        tagged_types,
        tuples,
        fn_types,
        imported_fns,
    } = program;
    drop((
        externs,
        link_libs,
        structs,
        enums,
        resources,
        tagged_types,
        tuples,
        fn_types,
        imported_fns,
    ));

    drop_functions(fns);
}

fn drop_functions(fns: Vec<hir::Fn>) {
    let mut work: Vec<DropWork> = fns.into_iter().map(DropWork::Function).collect();
    while let Some(item) = work.pop() {
        match item {
            DropWork::Function(function) => {
                let hir::Fn {
                    name,
                    origin,
                    params,
                    param_modes,
                    ret,
                    return_borrow,
                    return_region,
                    return_cleanup,
                    locals,
                    body,
                    span,
                    drop_locals,
                    drop_individual_locals,
                    drop_individual_exprs,
                } = function;
                drop((
                    name,
                    origin,
                    params,
                    param_modes,
                    ret,
                    return_borrow,
                    return_region,
                    return_cleanup,
                    locals,
                    span,
                    drop_locals,
                    drop_individual_locals,
                    drop_individual_exprs,
                ));
                work.push(DropWork::Block(body));
            }
            DropWork::Block(block) => {
                let hir::Block { stmts, value } = block;
                work.extend(stmts.into_iter().map(DropWork::Stmt));
                if let Some(value) = value {
                    work.push(DropWork::Expr(*value));
                }
            }
            DropWork::Stmt(stmt) => match stmt {
                Stmt::Let { local: _, init } => {
                    work.push(DropWork::Expr(init));
                }
                Stmt::LetTuple {
                    locals,
                    tuple_id: _,
                    init,
                } => {
                    drop(locals);
                    work.push(DropWork::Expr(init));
                }
                Stmt::Assign {
                    local: _,
                    value,
                    drop_old: _,
                    drop_new: _,
                } => {
                    work.push(DropWork::Expr(value));
                }
                Stmt::AssignIndex {
                    base: _,
                    index,
                    value,
                } => {
                    work.push(DropWork::Expr(index));
                    work.push(DropWork::Expr(value));
                }
                Stmt::AssignVecLane {
                    local: _,
                    lane: _,
                    value,
                } => {
                    work.push(DropWork::Expr(value));
                }
                Stmt::AssignField {
                    root: _,
                    path,
                    value,
                } => {
                    drop(path);
                    work.push(DropWork::Expr(value));
                }
                Stmt::AssignElemField {
                    base: _,
                    index,
                    path,
                    struct_id: _,
                    soa: _,
                    value,
                } => {
                    drop(path);
                    work.push(DropWork::Expr(index));
                    work.push(DropWork::Expr(value));
                }
                Stmt::AssignElem {
                    base: _,
                    index,
                    struct_id: _,
                    soa: _,
                    value,
                } => {
                    work.push(DropWork::Expr(index));
                    work.push(DropWork::Expr(value));
                }
                Stmt::Return(value) | Stmt::Break { value, .. } => {
                    if let Some(value) = value {
                        work.push(DropWork::Expr(value));
                    }
                }
                Stmt::Expr(expr) => work.push(DropWork::Expr(expr)),
            },
            DropWork::Expr(expression) => {
                let hir::Expr {
                    kind,
                    ty: _,
                    span: _,
                } = expression;
                drop_expr_kind(kind, &mut work);
            }
            DropWork::MatchArm(arm) => {
                let hir::MatchArm {
                    variants,
                    bindings,
                    body,
                } = arm;
                drop((variants, bindings));
                work.push(DropWork::Expr(body));
            }
            DropWork::Stage(stage) => {
                let hir::Stage { kind, out_ty: _ } = stage;
                match kind {
                    StageKind::Map { func, captures } | StageKind::Where { func, captures } => {
                        drop(func);
                        work.extend(captures.into_iter().map(DropWork::Expr));
                    }
                    StageKind::WhereField { field: _ } | StageKind::Project { field: _ } => {}
                    StageKind::WhereStrContains { needle } => {
                        work.push(DropWork::Expr(needle));
                    }
                }
            }
            DropWork::TemplatePart(part) => match part {
                TemplatePart::Text(text) => drop(text),
                TemplatePart::Hole(expr) | TemplatePart::JsonStr(expr) => {
                    work.push(DropWork::Expr(expr));
                }
                TemplatePart::OptionField { access, name }
                | TemplatePart::OptionStructField { access, name, .. } => {
                    drop(name);
                    work.push(DropWork::Expr(access));
                }
                TemplatePart::PopComma => {}
                TemplatePart::StructArrayField { access, .. }
                | TemplatePart::ScalarArrayField { access, .. }
                | TemplatePart::UnionValue { access, .. } => {
                    work.push(DropWork::Expr(access));
                }
            },
        }
    }
}

fn drop_expr_kind(kind: ExprKind, work: &mut Vec<DropWork>) {
    macro_rules! one {
        ($expr:expr) => {{
            work.push(DropWork::Expr(*$expr));
        }};
    }
    macro_rules! many {
        ($exprs:expr) => {{
            work.extend($exprs.into_iter().map(DropWork::Expr));
        }};
    }
    macro_rules! block {
        ($block:expr) => {{
            work.push(DropWork::Block($block));
        }};
    }
    macro_rules! stages {
        ($stages:expr) => {{
            work.extend($stages.into_iter().map(DropWork::Stage));
        }};
    }
    macro_rules! arms {
        ($arms:expr) => {{
            work.extend($arms.into_iter().map(DropWork::MatchArm));
        }};
    }
    macro_rules! parts {
        ($parts:expr) => {{
            work.extend($parts.into_iter().map(DropWork::TemplatePart));
        }};
    }
    macro_rules! optional {
        ($expr:expr) => {
            if let Some(expr) = $expr {
                work.push(DropWork::Expr(*expr));
            }
        };
    }

    match kind {
        ExprKind::Unit
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Local(_)
        | ExprKind::FnValue(_)
        | ExprKind::SqliteCallbackDescriptor { .. }
        | ExprKind::Wait
        | ExprKind::Field { .. }
        | ExprKind::SoaColumn { .. }
        | ExprKind::IndexField { .. }
        | ExprKind::OptionNone
        | ExprKind::ArrayGroupAgg { .. }
        | ExprKind::ArrayGroupAggMulti { .. }
        | ExprKind::ArrayDictEncode { .. }
        | ExprKind::ReaderStdin
        | ExprKind::WriterStd { .. }
        | ExprKind::TimeNow
        | ExprKind::TimeInstant
        | ExprKind::ProcessCpuCount
        | ExprKind::ProcessAbort
        | ExprKind::RandSeed
        | ExprKind::RawNull
        | ExprKind::HttpClient => {}
        ExprKind::Unary { expr, .. }
        | ExprKind::Cast(expr)
        | ExprKind::TaskGet(expr)
        | ExprKind::OptionSome(expr)
        | ExprKind::ResultOk(expr)
        | ExprKind::ResultErr(expr)
        | ExprKind::Try(expr)
        | ExprKind::RawAlloc(expr)
        | ExprKind::RawFree(expr)
        | ExprKind::RawIsNull(expr)
        | ExprKind::StaticDescriptorView { ptr: expr, .. }
        | ExprKind::ResourceBorrow { owner: expr, .. }
        | ExprKind::ResourceRaw {
            reference: expr, ..
        }
        | ExprKind::ResourceIntoRaw { owner: expr, .. }
        | ExprKind::HeapNew(expr)
        | ExprKind::BoxGet(expr)
        | ExprKind::BoxClone(expr)
        | ExprKind::StrClone(expr)
        | ExprKind::StrBorrow(expr)
        | ExprKind::BuilderToString(expr)
        | ExprKind::ArrayToSlice(expr)
        | ExprKind::Len(expr)
        | ExprKind::ArrayBuilderBuild(expr) => one!(expr),
        ExprKind::CloneIn { value, region } => {
            one!(value);
            one!(region);
        }
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::IntArith { lhs, rhs, .. }
        | ExprKind::ResultMapErr {
            result: lhs,
            f: rhs,
        }
        | ExprKind::RawLoad {
            ptr: lhs,
            offset: rhs,
            ..
        }
        | ExprKind::RawOffset {
            ptr: lhs,
            offset: rhs,
        }
        | ExprKind::StrPredicate {
            haystack: lhs,
            needle: rhs,
            ..
        }
        | ExprKind::BuilderWrite {
            builder: lhs,
            arg: rhs,
            ..
        }
        | ExprKind::ReaderRead {
            reader: lhs,
            buffer: rhs,
        }
        | ExprKind::ReaderReadLine {
            reader: lhs,
            buffer: rhs,
        }
        | ExprKind::WriterWrite {
            writer: lhs,
            arg: rhs,
            ..
        }
        | ExprKind::IoCopy {
            reader: lhs,
            writer: rhs,
        }
        | ExprKind::BufferPut {
            buffer: lhs,
            value: rhs,
            ..
        }
        | ExprKind::BufferAppend {
            buffer: lhs,
            data: rhs,
        }
        | ExprKind::ArrayBuilderPush {
            builder: lhs,
            value: rhs,
            ..
        }
        | ExprKind::ArrayBuilderAppend {
            builder: lhs,
            data: rhs,
        }
        | ExprKind::FsWriteFile {
            path: lhs,
            data: rhs,
            ..
        }
        | ExprKind::TcpConnect {
            host: lhs,
            port: rhs,
        }
        | ExprKind::TcpReadTimeout { conn: lhs, ns: rhs }
        | ExprKind::TcpWriteTimeout { conn: lhs, ns: rhs }
        | ExprKind::TcpListen {
            host: lhs,
            port: rhs,
        }
        | ExprKind::UdpBind {
            host: lhs,
            port: rhs,
        }
        | ExprKind::UdpRecvFrom {
            sock: lhs,
            buffer: rhs,
        }
        | ExprKind::PathJoin { a: lhs, b: rhs }
        | ExprKind::EnvSet {
            name: lhs,
            value: rhs,
        }
        | ExprKind::ProcessSpawn {
            cmd: lhs,
            args: rhs,
        }
        | ExprKind::ChildKill {
            child: lhs,
            sig: rhs,
        }
        | ExprKind::ProcessExec {
            cmd: lhs,
            args: rhs,
        }
        | ExprKind::ProcessCommand {
            cmd: lhs,
            args: rhs,
        }
        | ExprKind::CommandCwd {
            command: lhs,
            dir: rhs,
        }
        | ExprKind::CommandTimeout {
            command: lhs,
            ns: rhs,
        }
        | ExprKind::Compress {
            data: lhs,
            level: rhs,
            ..
        }
        | ExprKind::RegexIsMatch {
            regex: lhs,
            text: rhs,
        }
        | ExprKind::RegexFindAll {
            regex: lhs,
            text: rhs,
        }
        | ExprKind::RegexSplit {
            regex: lhs,
            text: rhs,
        }
        | ExprKind::RegexCaptures {
            regex: lhs,
            text: rhs,
        }
        | ExprKind::RegexGroupIndex {
            regex: lhs,
            name: rhs,
        }
        | ExprKind::CapturesGroup {
            caps: lhs,
            index: rhs,
        }
        | ExprKind::CliParse {
            cmd: lhs,
            args: rhs,
        }
        | ExprKind::CliGetBool {
            parsed: lhs,
            name: rhs,
        }
        | ExprKind::CliGetI64 {
            parsed: lhs,
            name: rhs,
        }
        | ExprKind::CliGetStr {
            parsed: lhs,
            name: rhs,
        }
        | ExprKind::HttpRequest {
            method: lhs,
            url: rhs,
        }
        | ExprKind::HttpBody {
            req: lhs,
            data: rhs,
        }
        | ExprKind::HttpRequestTimeout { req: lhs, ns: rhs }
        | ExprKind::HttpRespHeader {
            resp: lhs,
            name: rhs,
        }
        | ExprKind::HttpClientTimeout {
            client: lhs,
            ns: rhs,
        }
        | ExprKind::HttpClientGet {
            client: lhs,
            url: rhs,
        }
        | ExprKind::HttpClientRequest {
            client: lhs,
            req: rhs,
        }
        | ExprKind::HttpCtxHeader {
            headers: lhs,
            name: rhs,
        }
        | ExprKind::HttpRbBody { rb: lhs, data: rhs }
        | ExprKind::HttpRespond { ctx: lhs, rb: rhs }
        | ExprKind::HttpRespondStream { ctx: lhs, rb: rhs }
        | ExprKind::HttpStreamSend {
            stream: lhs,
            chunk: rhs,
            ..
        }
        | ExprKind::HttpStreamReject {
            stream: lhs,
            rb: rhs,
        }
        | ExprKind::CryptoCtEqual { a: lhs, b: rhs }
        | ExprKind::CryptoHmac {
            key: lhs,
            data: rhs,
        } => {
            one!(lhs);
            one!(rhs);
        }
        ExprKind::RawStore { ptr, offset, value }
        | ExprKind::Select {
            mask: ptr,
            a: offset,
            b: value,
        }
        | ExprKind::VecStore {
            dst: ptr,
            index: offset,
            value,
            ..
        }
        | ExprKind::RegexReplace {
            regex: ptr,
            text: offset,
            repl: value,
            ..
        }
        | ExprKind::HttpHeader {
            req: ptr,
            name: offset,
            value,
        }
        | ExprKind::HttpClientPost {
            client: ptr,
            url: offset,
            body: value,
        }
        | ExprKind::HttpRbHeader {
            rb: ptr,
            name: offset,
            value,
        } => {
            one!(ptr);
            one!(offset);
            one!(value);
        }
        ExprKind::MathOp { operands, .. }
        | ExprKind::Closure {
            captures: operands, ..
        }
        | ExprKind::EnumValue {
            payload: operands, ..
        }
        | ExprKind::Call { args: operands, .. }
        | ExprKind::StructLit {
            fields: operands, ..
        }
        | ExprKind::Tuple {
            elems: operands, ..
        }
        | ExprKind::ArrayLit {
            elems: operands, ..
        }
        | ExprKind::ConstArray {
            elems: operands, ..
        }
        | ExprKind::ArrayZip {
            sources: operands, ..
        }
        | ExprKind::VecLit {
            elems: operands, ..
        } => many!(operands),
        ExprKind::CallFnValue { callee, args } => {
            one!(callee);
            many!(args);
        }
        ExprKind::RawCall {
            guard,
            callee,
            args,
            ..
        } => {
            if let Some(guard) = guard {
                one!(guard);
            }
            one!(callee);
            many!(args);
        }
        ExprKind::RawPointerLoad { ptr, offset } => {
            one!(ptr);
            one!(offset);
        }
        ExprKind::Spawn { closure, .. } => one!(closure),
        ExprKind::TaskGroup(block)
        | ExprKind::Block(block)
        | ExprKind::Arena(block)
        | ExprKind::NamedArena { block, .. }
        | ExprKind::Unsafe(block) => block!(block),
        ExprKind::Loop { body, .. } => block!(body),
        ExprKind::Match { scrutinee, arms } => {
            one!(scrutinee);
            arms!(arms);
        }
        ExprKind::ElseUnwrap { opt, fallback } => {
            one!(opt);
            one!(fallback);
        }
        ExprKind::If { cond, then, els } => {
            one!(cond);
            block!(then);
            block!(els);
        }
        ExprKind::TupleIndex { recv, .. }
        | ExprKind::StrTrim { recv, .. }
        | ExprKind::ArrayToSoa { source: recv, .. }
        | ExprKind::JsonDecode { input: recv, .. }
        | ExprKind::JsonDecodeArray { input: recv, .. }
        | ExprKind::JsonDecodeScalar { input: recv, .. }
        | ExprKind::JsonDecodeStructArray { input: recv, .. }
        | ExprKind::JsonDecodeSoa { input: recv, .. }
        | ExprKind::JsonDecodeUnion { input: recv, .. }
        | ExprKind::JsonDoc { input: recv }
        | ExprKind::JsonDocKind { doc: recv }
        | ExprKind::JsonDocAsStr { doc: recv }
        | ExprKind::JsonDocAsScalar { doc: recv, .. }
        | ExprKind::JsonDocLen { doc: recv }
        | ExprKind::JsonDocElems { doc: recv }
        | ExprKind::JsonScan { input: recv, .. }
        | ExprKind::FsReadFile { path: recv }
        | ExprKind::ReaderOpen { path: recv }
        | ExprKind::WriterCreate { path: recv }
        | ExprKind::ReaderBuffered { reader: recv }
        | ExprKind::BytesAsStr { bytes: recv }
        | ExprKind::WriterFlush { writer: recv }
        | ExprKind::FileCreateRw { path: recv }
        | ExprKind::FileOpenRw { path: recv }
        | ExprKind::FileLen { file: recv }
        | ExprKind::BufferNew { capacity: recv }
        | ExprKind::BufferBytes { buffer: recv }
        | ExprKind::StrBytes { inner: recv }
        | ExprKind::BufferLen { buffer: recv }
        | ExprKind::FsExists { path: recv }
        | ExprKind::FsRemove { path: recv }
        | ExprKind::FsReadDir { path: recv }
        | ExprKind::DnsResolve { host: recv }
        | ExprKind::ConnReader { conn: recv }
        | ExprKind::ConnWriter { conn: recv }
        | ExprKind::TcpAccept { listener: recv }
        | ExprKind::FsReadFileView { path: recv }
        | ExprKind::FsReadBytesView { path: recv }
        | ExprKind::PathComponent { path: recv, .. }
        | ExprKind::PathNormalize { path: recv }
        | ExprKind::EnvGet { name: recv }
        | ExprKind::TimeSleep { ns: recv }
        | ExprKind::ProcessExit { code: recv }
        | ExprKind::ChildWait { child: recv }
        | ExprKind::CommandEnvClear { command: recv }
        | ExprKind::CommandRun { command: recv }
        | ExprKind::RunOutputCode { out: recv }
        | ExprKind::RunOutputStdout { out: recv }
        | ExprKind::RunOutputStderr { out: recv }
        | ExprKind::EncodingEncode { data: recv, .. }
        | ExprKind::EncodingDecode { input: recv, .. }
        | ExprKind::Utf8Valid { data: recv }
        | ExprKind::Decompress { data: recv, .. }
        | ExprKind::RandSeedWith { seed: recv }
        | ExprKind::RandNext { rng: recv }
        | ExprKind::RegexCompile { pattern: recv }
        | ExprKind::RegexGroupCount { regex: recv }
        | ExprKind::CliCommand { name: recv }
        | ExprKind::CliUsage { cmd: recv }
        | ExprKind::HttpParse { data: recv }
        | ExprKind::HttpRespStatus { resp: recv }
        | ExprKind::HttpRespBody { resp: recv }
        | ExprKind::HttpAccept { server: recv }
        | ExprKind::HttpCtxMethod { ctx: recv }
        | ExprKind::HttpCtxPath { ctx: recv }
        | ExprKind::HttpCtxHeaders { ctx: recv }
        | ExprKind::HttpCtxBody { ctx: recv }
        | ExprKind::HttpResponseBuilder { status: recv }
        | ExprKind::HttpStreamFinish { stream: recv }
        | ExprKind::CryptoRandom { out: recv }
        | ExprKind::CryptoHash { data: recv, .. } => one!(recv),
        ExprKind::Index { recv, index } | ExprKind::ElemField { recv, index, .. } => {
            one!(recv);
            one!(index);
        }
        ExprKind::ArrayBuilderNew { region, .. } => optional!(region),
        ExprKind::BuilderNew { capacity } => optional!(capacity),
        ExprKind::SliceRange { recv, start, end } => {
            one!(recv);
            optional!(start);
            optional!(end);
        }
        ExprKind::RegexFind { regex, text, start } => {
            one!(regex);
            one!(text);
            optional!(start);
        }
        ExprKind::CliFlag {
            cmd, name, default, ..
        } => {
            one!(cmd);
            one!(name);
            optional!(default);
        }
        ExprKind::VecSumWhere { vec, mask } | ExprKind::VecDot { a: vec, b: mask } => {
            one!(vec);
            one!(mask);
        }
        ExprKind::VecMinMax { vec, .. } | ExprKind::VecSum { vec } => one!(vec),
        ExprKind::VecLoad { src, index, .. } => {
            one!(src);
            one!(index);
        }
        ExprKind::ArrayChunks { source, n, .. } => {
            one!(source);
            one!(n);
        }
        ExprKind::ArraySum { source, stages }
        | ExprKind::ArrayCount { source, stages }
        | ExprKind::ArrayMinMax { source, stages, .. }
        | ExprKind::ArraySort { source, stages, .. }
        | ExprKind::ArrayToArray { source, stages, .. } => {
            one!(source);
            stages!(stages);
        }
        ExprKind::ArrayAnyAll {
            source,
            stages,
            captures,
            ..
        }
        | ExprKind::ArraySortBy {
            source,
            stages,
            captures,
            ..
        }
        | ExprKind::ArrayPartition {
            source,
            stages,
            captures,
            ..
        }
        | ExprKind::ArrayParMap {
            source,
            stages,
            captures,
            ..
        } => {
            one!(source);
            stages!(stages);
            many!(captures);
        }
        ExprKind::ArrayReduce {
            source,
            stages,
            captures,
            init,
            ..
        }
        | ExprKind::ArrayScan {
            source,
            stages,
            captures,
            init,
            ..
        } => {
            one!(source);
            stages!(stages);
            many!(captures);
            one!(init);
        }
        ExprKind::ArrayDot { a, b, .. } => {
            one!(a);
            one!(b);
        }
        ExprKind::ArrayMapInto {
            source,
            stages,
            dst,
            ..
        } => {
            one!(source);
            stages!(stages);
            one!(dst);
        }
        ExprKind::Template(parts) => parts!(parts),
        ExprKind::JsonDocGet { doc, key }
        | ExprKind::JsonDocAt { doc, index: key }
        | ExprKind::JsonDocKey { doc, index: key } => {
            one!(doc);
            one!(key);
        }
        ExprKind::FilePread {
            file,
            buffer,
            offset,
        }
        | ExprKind::FilePwrite {
            file,
            data: buffer,
            offset,
        } => {
            one!(file);
            one!(buffer);
            one!(offset);
        }
        ExprKind::BytesRead { bytes, offset, .. } => {
            one!(bytes);
            one!(offset);
        }
        ExprKind::UdpSendTo {
            sock,
            data,
            host,
            port,
        } => {
            one!(sock);
            one!(data);
            one!(host);
            one!(port);
        }
        ExprKind::CommandEnv {
            command,
            name,
            value,
        } => {
            one!(command);
            one!(name);
            one!(value);
        }
        ExprKind::RandRange { rng, lo, hi } => {
            one!(rng);
            one!(lo);
            one!(hi);
        }
        ExprKind::RandShuffle { rng, xs, .. } => {
            one!(rng);
            one!(xs);
        }
        ExprKind::RandSample { rng, xs, k, .. } => {
            one!(rng);
            one!(xs);
            one!(k);
        }
        ExprKind::HttpGetMany {
            client,
            urls,
            max_concurrency,
        } => {
            one!(client);
            one!(urls);
            one!(max_concurrency);
        }
        ExprKind::HttpServe { host, port, .. } => {
            one!(host);
            one!(port);
        }
        ExprKind::CryptoHkdf {
            salt,
            ikm,
            info,
            len,
        } => {
            one!(salt);
            one!(ikm);
            one!(info);
            one!(len);
        }
        ExprKind::CryptoAead {
            key,
            nonce,
            input,
            aad,
            ..
        } => {
            one!(key);
            one!(nonce);
            one!(input);
            one!(aad);
        }
        ExprKind::CryptoArgon2 {
            password,
            salt,
            params,
        } => {
            one!(password);
            one!(salt);
            one!(params);
        }
        ExprKind::ResourceFromRaw { raw, parent, .. } => {
            one!(raw);
            if let Some(parent) = parent {
                one!(parent);
            }
        }
        ExprKind::ResourceViewFromRaw {
            owner, ptr, len, ..
        } => {
            one!(owner);
            one!(ptr);
            one!(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FnEffect, IntTy, Ty};
    use align_ast::UnOp;
    use align_span::Span;

    fn int_ty() -> Ty {
        Ty::Int(IntTy {
            bits: 64,
            signed: true,
        })
    }

    fn leaf(span: Span) -> hir::Expr {
        hir::Expr {
            kind: ExprKind::Int(0),
            ty: int_ty(),
            span,
        }
    }

    fn program_with_body(body: hir::Block) -> hir::Program {
        hir::Program {
            fns: vec![hir::Fn {
                name: "clone_test".to_string(),
                origin: hir::FnOrigin::Source {
                    is_entry: false,
                    is_public: false,
                },
                params: Vec::new(),
                param_modes: Vec::new(),
                ret: int_ty(),
                return_borrow: hir::ReturnBorrowSummary::None,
                return_region: hir::ReturnRegionSummary::None,
                return_cleanup: hir::ReturnCleanupAbi::None,
                locals: Vec::new(),
                body,
                span: Span::new(0, 0, 0),
                drop_locals: Vec::new(),
                drop_individual_locals: Vec::new(),
                drop_individual_exprs: Default::default(),
            }],
            externs: Vec::new(),
            link_libs: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            resources: Vec::new(),
            tagged_types: Vec::new(),
            tuples: Vec::new(),
            fn_types: Vec::new(),
            imported_fns: Vec::new(),
        }
    }

    #[test]
    fn clone_frames_distinguish_repeated_same_span_nodes() {
        let span = Span::new(0, 7, 7);
        let repeated = hir::Expr {
            kind: ExprKind::Unary {
                op: UnOp::Neg,
                expr: Box::new(leaf(span)),
            },
            ty: int_ty(),
            span,
        };
        let program = program_with_body(hir::Block {
            stmts: vec![Stmt::Expr(repeated.clone()), Stmt::Expr(repeated)],
            value: None,
        });
        let events = crate::hir_depth::clone_events(&program.fns[0].body);
        assert!(
            events.is_some(),
            "valid test body must produce clone frames"
        );
        let Some(events) = events else {
            return;
        };
        let mut next_id = 1usize;
        let mut enters = 0usize;
        let mut exits = 0usize;
        for event in events {
            match event {
                crate::hir_depth::CloneEvent::RecordEnter { id, .. } => {
                    assert_eq!(id, next_id);
                    next_id += 1;
                    enters += 1;
                }
                crate::hir_depth::CloneEvent::RecordExit { id } => {
                    assert!(id < next_id);
                    exits += 1;
                }
            }
        }
        assert_eq!(enters, exits);
        let cloned = clone_program(&program);
        assert!(cloned.is_some());
        if let Some(cloned) = cloned {
            drop_program(cloned);
        }
    }

    #[test]
    fn clone_and_drop_are_iterative_for_a_deep_body() {
        let span = Span::new(0, 0, 0);
        let mut expression = leaf(span);
        for _ in 0..4096 {
            expression = hir::Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expression),
                },
                ty: int_ty(),
                span,
            };
        }
        let program = program_with_body(hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expression)),
        });
        let cloned = clone_program(&program);
        assert!(cloned.is_some());
        if let Some(cloned) = cloned {
            drop_program(cloned);
        }
        std::mem::forget(program);
    }

    #[test]
    fn clone_preserves_fn_type_cells_and_assignment_flags() {
        let mut program = program_with_body(hir::Block {
            stmts: vec![Stmt::Assign {
                local: 0,
                value: leaf(Span::new(0, 1, 1)),
                drop_old: std::cell::Cell::new(true),
                drop_new: std::cell::Cell::new(false),
            }],
            value: None,
        });
        program.fn_types.push(hir::FnTy {
            params: Vec::new(),
            ret: int_ty(),
            return_borrow: hir::ReturnBorrowSummary::None,
            return_region: hir::ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
            effect: std::cell::Cell::new(FnEffect::Impure),
        });

        let cloned = clone_program(&program).expect("valid HIR clone");
        assert_eq!(cloned.fn_types.len(), 1);
        assert_eq!(cloned.fn_types[0].effect.get(), FnEffect::Impure);
        match &cloned.fns[0].body.stmts[0] {
            Stmt::Assign {
                drop_old, drop_new, ..
            } => {
                assert!(drop_old.get());
                assert!(!drop_new.get());
            }
            _ => panic!("clone changed assignment statement kind"),
        }
        drop_program(cloned);
    }

    #[test]
    fn finish_children_rejects_missing_and_extra_children() {
        let missing = finish_children::<()>(Vec::new(), |children| {
            children.expr()?;
            Some(())
        });
        assert!(missing.is_none(), "missing child must fail closed");

        let extra = finish_children::<()>(
            vec![CloneValue::Expr(leaf(Span::new(0, 0, 0)))],
            |_children| Some(()),
        );
        assert!(extra.is_none(), "extra child must fail closed");
    }
}
