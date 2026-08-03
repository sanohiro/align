//! Stack-bounded cloning for checked-HIR body replay.
//!
//! The derived `Clone` implementations for HIR are correct for ordinary compiler inputs, but
//! they recursively clone every boxed expression. The replay boundary accepts producer-valid
//! bodies up to `MAX_CHECKED_HIR_DEPTH` on a 2 MiB owner stack, so replay must rebuild the tree from
//! a child-first explicit worklist instead of calling those derived implementations.

use std::collections::HashMap;

use crate::hir::{self, ExprKind, StageKind, Stmt, TemplatePart};

struct ClonedExprs {
    values: HashMap<usize, hir::Expr>,
    failed: bool,
}

impl ClonedExprs {
    fn new() -> Self {
        Self {
            values: HashMap::new(),
            failed: false,
        }
    }

    fn insert(&mut self, key: usize, value: hir::Expr) -> Option<hir::Expr> {
        self.values.insert(key, value)
    }

    fn remove(&mut self, key: &usize) -> Option<hir::Expr> {
        self.values.remove(key)
    }

    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

fn expr_key(expr: &hir::Expr) -> usize {
    std::ptr::from_ref(expr) as usize
}

fn take_expr(clones: &mut ClonedExprs, original: &hir::Expr) -> hir::Expr {
    match clones.remove(&expr_key(original)) {
        Some(expr) => expr,
        None => {
            // A validated body cannot reach this branch. Keep reconstruction total for direct
            // malformed-HIR callers; the enclosing clone returns None after recording failure.
            clones.failed = true;
            hir::Expr {
                kind: ExprKind::Unit,
                ty: original.ty,
                span: original.span,
            }
        }
    }
}

fn take_boxed_expr(clones: &mut ClonedExprs, original: &hir::Expr) -> Box<hir::Expr> {
    Box::new(take_expr(clones, original))
}

fn take_optional_boxed_expr(
    clones: &mut ClonedExprs,
    original: Option<&hir::Expr>,
) -> Option<Box<hir::Expr>> {
    original.map(|expr| take_boxed_expr(clones, expr))
}

fn take_exprs(clones: &mut ClonedExprs, originals: &[hir::Expr]) -> Vec<hir::Expr> {
    originals
        .iter()
        .map(|expr| take_expr(clones, expr))
        .collect()
}

fn clone_match_arm(clones: &mut ClonedExprs, arm: &hir::MatchArm) -> hir::MatchArm {
    hir::MatchArm {
        variants: arm.variants.clone(),
        bindings: arm.bindings.clone(),
        body: take_expr(clones, &arm.body),
    }
}

fn clone_stage(clones: &mut ClonedExprs, stage: &hir::Stage) -> hir::Stage {
    let kind = match &stage.kind {
        StageKind::Map { func, captures } => StageKind::Map {
            func: func.clone(),
            captures: take_exprs(clones, captures),
        },
        StageKind::Where { func, captures } => StageKind::Where {
            func: func.clone(),
            captures: take_exprs(clones, captures),
        },
        StageKind::WhereField { field } => StageKind::WhereField { field: *field },
        StageKind::WhereStrContains { needle } => StageKind::WhereStrContains {
            needle: take_expr(clones, needle),
        },
        StageKind::Project { field } => StageKind::Project { field: *field },
    };
    hir::Stage {
        kind,
        out_ty: stage.out_ty,
    }
}

fn clone_template_part(clones: &mut ClonedExprs, part: &TemplatePart) -> TemplatePart {
    match part {
        TemplatePart::Text(text) => TemplatePart::Text(text.clone()),
        TemplatePart::Hole(expr) => TemplatePart::Hole(take_expr(clones, expr)),
        TemplatePart::JsonStr(expr) => TemplatePart::JsonStr(take_expr(clones, expr)),
        TemplatePart::OptionField { access, name } => TemplatePart::OptionField {
            access: take_expr(clones, access),
            name: name.clone(),
        },
        TemplatePart::OptionStructField {
            access,
            name,
            struct_id,
        } => TemplatePart::OptionStructField {
            access: take_expr(clones, access),
            name: name.clone(),
            struct_id: *struct_id,
        },
        TemplatePart::PopComma => TemplatePart::PopComma,
        TemplatePart::StructArrayField { access, struct_id } => TemplatePart::StructArrayField {
            access: take_expr(clones, access),
            struct_id: *struct_id,
        },
        TemplatePart::ScalarArrayField { access, elem } => TemplatePart::ScalarArrayField {
            access: take_expr(clones, access),
            elem: *elem,
        },
        TemplatePart::UnionValue { access, enum_id } => TemplatePart::UnionValue {
            access: take_expr(clones, access),
            enum_id: *enum_id,
        },
    }
}

fn clone_block(clones: &mut ClonedExprs, block: &hir::Block) -> hir::Block {
    hir::Block {
        stmts: block
            .stmts
            .iter()
            .map(|stmt| clone_stmt(clones, stmt))
            .collect(),
        value: block
            .value
            .as_deref()
            .map(|expr| take_boxed_expr(clones, expr)),
    }
}

fn clone_stmt(clones: &mut ClonedExprs, stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::Let { local, init } => Stmt::Let {
            local: *local,
            init: take_expr(clones, init),
        },
        Stmt::LetTuple {
            locals,
            tuple_id,
            init,
        } => Stmt::LetTuple {
            locals: locals.clone(),
            tuple_id: *tuple_id,
            init: take_expr(clones, init),
        },
        Stmt::Assign {
            local,
            value,
            drop_old,
            drop_new,
        } => Stmt::Assign {
            local: *local,
            value: take_expr(clones, value),
            drop_old: std::cell::Cell::new(drop_old.get()),
            drop_new: std::cell::Cell::new(drop_new.get()),
        },
        Stmt::AssignIndex { base, index, value } => Stmt::AssignIndex {
            base: *base,
            index: take_expr(clones, index),
            value: take_expr(clones, value),
        },
        Stmt::AssignVecLane { local, lane, value } => Stmt::AssignVecLane {
            local: *local,
            lane: *lane,
            value: take_expr(clones, value),
        },
        Stmt::AssignField { root, path, value } => Stmt::AssignField {
            root: *root,
            path: path.clone(),
            value: take_expr(clones, value),
        },
        Stmt::AssignElemField {
            base,
            index,
            path,
            struct_id,
            soa,
            value,
        } => Stmt::AssignElemField {
            base: *base,
            index: take_expr(clones, index),
            path: path.clone(),
            struct_id: *struct_id,
            soa: *soa,
            value: take_expr(clones, value),
        },
        Stmt::AssignElem {
            base,
            index,
            struct_id,
            soa,
            value,
        } => Stmt::AssignElem {
            base: *base,
            index: take_expr(clones, index),
            struct_id: *struct_id,
            soa: *soa,
            value: take_expr(clones, value),
        },
        Stmt::Return(value) => Stmt::Return(value.as_ref().map(|expr| take_expr(clones, expr))),
        Stmt::Break { value, accepted } => Stmt::Break {
            value: value.as_ref().map(|expr| take_expr(clones, expr)),
            accepted: *accepted,
        },
        Stmt::Expr(expr) => Stmt::Expr(take_expr(clones, expr)),
    }
}

fn clone_expr_kind(clones: &mut ClonedExprs, kind: &ExprKind) -> ExprKind {
    macro_rules! boxed {
        ($expr:expr) => {
            take_boxed_expr(clones, $expr)
        };
    }

    match kind {
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
        | ExprKind::ArrayBuilderNew { .. }
        | ExprKind::TimeNow
        | ExprKind::TimeInstant
        | ExprKind::ProcessCpuCount
        | ExprKind::ProcessAbort
        | ExprKind::RandSeed
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
            operands: take_exprs(clones, operands),
        },
        ExprKind::Closure { lifted, captures } => ExprKind::Closure {
            lifted: lifted.clone(),
            captures: take_exprs(clones, captures),
        },
        ExprKind::CallFnValue { callee, args } => ExprKind::CallFnValue {
            callee: boxed!(callee),
            args: take_exprs(clones, args),
        },
        ExprKind::TaskGroup(block) => ExprKind::TaskGroup(clone_block(clones, block)),
        ExprKind::EnumValue {
            enum_id,
            variant,
            payload,
        } => ExprKind::EnumValue {
            enum_id: *enum_id,
            variant: *variant,
            payload: take_exprs(clones, payload),
        },
        ExprKind::Match { scrutinee, arms } => ExprKind::Match {
            scrutinee: boxed!(scrutinee),
            arms: arms
                .iter()
                .map(|arm| clone_match_arm(clones, arm))
                .collect(),
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
            args: take_exprs(clones, args),
            type_args: type_args.clone(),
        },
        ExprKind::If { cond, then, els } => ExprKind::If {
            cond: boxed!(cond),
            then: clone_block(clones, then),
            els: clone_block(clones, els),
        },
        ExprKind::StructLit { struct_id, fields } => ExprKind::StructLit {
            struct_id: *struct_id,
            fields: take_exprs(clones, fields),
        },
        ExprKind::Tuple { tuple_id, elems } => ExprKind::Tuple {
            tuple_id: *tuple_id,
            elems: take_exprs(clones, elems),
        },
        ExprKind::TupleIndex { recv, index } => ExprKind::TupleIndex {
            recv: boxed!(recv),
            index: *index,
        },
        ExprKind::Block(block) => ExprKind::Block(clone_block(clones, block)),
        ExprKind::OptionSome(expr) => ExprKind::OptionSome(boxed!(expr)),
        ExprKind::ElseUnwrap { opt, fallback } => ExprKind::ElseUnwrap {
            opt: boxed!(opt),
            fallback: boxed!(fallback),
        },
        ExprKind::ResultOk(expr) => ExprKind::ResultOk(boxed!(expr)),
        ExprKind::ResultErr(expr) => ExprKind::ResultErr(boxed!(expr)),
        ExprKind::Try(expr) => ExprKind::Try(boxed!(expr)),
        ExprKind::Loop {
            body,
            diverges,
            body_locals,
        } => ExprKind::Loop {
            body: clone_block(clones, body),
            diverges: *diverges,
            body_locals: body_locals.clone(),
        },
        ExprKind::Arena(block) => ExprKind::Arena(clone_block(clones, block)),
        ExprKind::Unsafe(block) => ExprKind::Unsafe(clone_block(clones, block)),
        ExprKind::RawAlloc(expr) => ExprKind::RawAlloc(boxed!(expr)),
        ExprKind::RawFree(expr) => ExprKind::RawFree(boxed!(expr)),
        ExprKind::RawLoad {
            ptr,
            offset,
            scalar,
        } => ExprKind::RawLoad {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
            scalar: *scalar,
        },
        ExprKind::RawStore { ptr, offset, value } => ExprKind::RawStore {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
            value: boxed!(value),
        },
        ExprKind::RawOffset { ptr, offset } => ExprKind::RawOffset {
            ptr: boxed!(ptr),
            offset: boxed!(offset),
        },
        ExprKind::HeapNew(expr) => ExprKind::HeapNew(boxed!(expr)),
        ExprKind::BoxGet(expr) => ExprKind::BoxGet(boxed!(expr)),
        ExprKind::BoxClone(expr) => ExprKind::BoxClone(boxed!(expr)),
        ExprKind::StrClone(expr) => ExprKind::StrClone(boxed!(expr)),
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
        ExprKind::BuilderNew { capacity } => ExprKind::BuilderNew {
            capacity: take_optional_boxed_expr(clones, capacity.as_deref()),
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
            elems: take_exprs(clones, elems),
            elem: *elem,
            pooled: *pooled,
        },
        ExprKind::ConstArray { elems, elem, len } => ExprKind::ConstArray {
            elems: take_exprs(clones, elems),
            elem: *elem,
            len: *len,
        },
        ExprKind::ArrayZip { sources, tuple_id } => ExprKind::ArrayZip {
            sources: take_exprs(clones, sources),
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
            elems: take_exprs(clones, elems),
            elem: *elem,
        },
        ExprKind::ArraySum { source, stages } => ExprKind::ArraySum {
            source: boxed!(source),
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
        },
        ExprKind::ArrayCount { source, stages } => ExprKind::ArrayCount {
            source: boxed!(source),
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
        },
        ExprKind::ArrayAnyAll {
            source,
            stages,
            func,
            captures,
            all,
        } => ExprKind::ArrayAnyAll {
            source: boxed!(source),
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
            func: func.clone(),
            captures: take_exprs(clones, captures),
            all: *all,
        },
        ExprKind::ArrayMinMax {
            source,
            stages,
            is_max,
        } => ExprKind::ArrayMinMax {
            source: boxed!(source),
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
            func: func.clone(),
            captures: take_exprs(clones, captures),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
            func: func.clone(),
            captures: take_exprs(clones, captures),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
            key_func: key_func.clone(),
            captures: take_exprs(clones, captures),
            key_ty: *key_ty,
            elem: *elem,
        },
        ExprKind::ArrayToArray {
            source,
            stages,
            elem,
        } => ExprKind::ArrayToArray {
            source: boxed!(source),
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
            func: func.clone(),
            captures: take_exprs(clones, captures),
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
            stages: stages
                .iter()
                .map(|stage| clone_stage(clones, stage))
                .collect(),
            func: func.clone(),
            captures: take_exprs(clones, captures),
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
            start: take_optional_boxed_expr(clones, start.as_deref()),
            end: take_optional_boxed_expr(clones, end.as_deref()),
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
        ExprKind::Template(parts) => ExprKind::Template(
            parts
                .iter()
                .map(|part| clone_template_part(clones, part))
                .collect(),
        ),
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
            start: take_optional_boxed_expr(clones, start.as_deref()),
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
            default: take_optional_boxed_expr(clones, default.as_deref()),
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
    }
}

fn clone_function(function: &hir::Fn) -> Option<hir::Fn> {
    let mut clones = ClonedExprs::new();
    for event in crate::hir_depth::body_events(&function.body) {
        if let crate::hir_depth::BodyEvent::ExprExit { expression, .. } = event {
            let cloned_kind = clone_expr_kind(&mut clones, &expression.kind);
            let previous = clones.insert(
                expr_key(expression),
                hir::Expr {
                    kind: cloned_kind,
                    ty: expression.ty,
                    span: expression.span,
                },
            );
            if previous.is_some() {
                clones.failed = true;
            }
        }
    }
    let body = clone_block(&mut clones, &function.body);
    if clones.failed || !clones.is_empty() {
        return None;
    }
    Some(hir::Fn {
        name: function.name.clone(),
        origin: function.origin,
        params: function.params.clone(),
        param_modes: function.param_modes.clone(),
        ret: function.ret,
        return_borrow: function.return_borrow.clone(),
        return_region: function.return_region.clone(),
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
        fns.push(clone_function(function)?);
    }
    Some(hir::Program {
        fns,
        externs: program.externs.clone(),
        link_libs: program.link_libs.clone(),
        structs: program.structs.clone(),
        enums: program.enums.clone(),
        tagged_types: program.tagged_types.clone(),
        tuples: program.tuples.clone(),
        fn_types: program.fn_types.clone(),
        imported_fns: program.imported_fns.clone(),
    })
}
