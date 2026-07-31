use crate::hir::{self, Block, Expr, ExprKind, MatchArm, Stage, StageKind, Stmt, TemplatePart};

/// Conservative producer ceiling for one checked-HIR function body:
/// `2 * (parser MAX_EXPR_DEPTH + 1) + 1 == 259`.
pub const MAX_CHECKED_HIR_DEPTH: usize = 259;

#[derive(Clone, Copy)]
enum BodyRecord<'a> {
    Block(&'a Block),
    Stmt(&'a Stmt),
    StmtExit(&'a Stmt),
    Expr(&'a Expr),
    ExprExit {
        expression: &'a Expr,
        children_completed: bool,
    },
    MatchArm {
        scrutinee: &'a Expr,
        arm: &'a MatchArm,
    },
    Stage(&'a Stage),
    TemplatePart(&'a TemplatePart),
}

#[derive(Clone, Copy)]
pub(crate) enum BodyEvent<'a> {
    StmtEnter(&'a Stmt),
    StmtExit(&'a Stmt),
    ExprEnter(&'a Expr),
    ExprExit {
        expression: &'a Expr,
        children_completed: bool,
    },
    MatchArmEnter {
        scrutinee: &'a Expr,
        arm: &'a MatchArm,
    },
}

/// Check every stored function body before a recursive checked-HIR consumer can run.
///
/// Every body is an independent root at depth one. The match over [`ExprKind`] is deliberately
/// exhaustive: adding a record-bearing expression must update the depth proof in the same change.
pub fn checked_hir_body_depth_is_valid(program: &hir::Program) -> bool {
    for function in &program.fns {
        if !walk_body_records(
            BodyRecord::Block(&function.body),
            MAX_CHECKED_HIR_DEPTH,
            None,
            false,
        ) {
            return false;
        }
    }
    true
}

fn walk_body_records<'a>(
    root: BodyRecord<'a>,
    max_depth: usize,
    mut events: Option<&mut Vec<BodyEvent<'a>>>,
    reachable_only: bool,
) -> bool {
    let mut work = vec![(root, 1usize)];
    while let Some((record, depth)) = work.pop() {
        if depth > max_depth {
            return false;
        }
        let exit_expr = match record {
            BodyRecord::Expr(expr) => Some(expr),
            _ => None,
        };
        if let Some(events) = events.as_mut() {
            match record {
                BodyRecord::Stmt(stmt) => events.push(BodyEvent::StmtEnter(stmt)),
                BodyRecord::StmtExit(stmt) => events.push(BodyEvent::StmtExit(stmt)),
                BodyRecord::Expr(expr) => events.push(BodyEvent::ExprEnter(expr)),
                BodyRecord::ExprExit {
                    expression,
                    children_completed,
                } => events.push(BodyEvent::ExprExit {
                    expression,
                    children_completed,
                }),
                BodyRecord::MatchArm { scrutinee, arm } => {
                    events.push(BodyEvent::MatchArmEnter { scrutinee, arm });
                }
                BodyRecord::Block(_)
                | BodyRecord::Stage(_)
                | BodyRecord::TemplatePart(_) => {}
            }
        }
        let child_depth = depth + 1;
        let child_start = work.len();
        let exit_stmt = match record {
            BodyRecord::Stmt(stmt) => Some(stmt),
            _ => None,
        };
        match record {
                BodyRecord::StmtExit(_) | BodyRecord::ExprExit { .. } => {}
                BodyRecord::Block(block) => {
                    work.extend(
                        block
                            .stmts
                            .iter()
                            .map(|stmt| (BodyRecord::Stmt(stmt), child_depth)),
                    );
                    if let Some(value) = block.value.as_deref() {
                        work.push((BodyRecord::Expr(value), child_depth));
                    }
                }
                BodyRecord::Stmt(stmt) => match stmt {
                    Stmt::Let { init, .. } | Stmt::LetTuple { init, .. } => {
                        work.push((BodyRecord::Expr(init), child_depth));
                    }
                    Stmt::Assign { value, .. }
                    | Stmt::AssignVecLane { value, .. }
                    | Stmt::AssignField { value, .. } => {
                        work.push((BodyRecord::Expr(value), child_depth));
                    }
                    Stmt::AssignIndex { index, value, .. }
                    | Stmt::AssignElemField { index, value, .. }
                    | Stmt::AssignElem { index, value, .. } => {
                        work.push((BodyRecord::Expr(index), child_depth));
                        work.push((BodyRecord::Expr(value), child_depth));
                    }
                    Stmt::Return(value) | Stmt::Break { value, .. } => {
                        if let Some(value) = value.as_ref() {
                            work.push((BodyRecord::Expr(value), child_depth));
                        }
                    }
                    Stmt::Expr(expr) => work.push((BodyRecord::Expr(expr), child_depth)),
                },
                BodyRecord::MatchArm { arm, .. } => {
                    work.push((BodyRecord::Expr(&arm.body), child_depth));
                }
                BodyRecord::Stage(stage) => match &stage.kind {
                    StageKind::Map { captures, .. } | StageKind::Where { captures, .. } => {
                        work.extend(
                            captures
                                .iter()
                                .map(|capture| (BodyRecord::Expr(capture), child_depth)),
                        );
                    }
                    StageKind::WhereStrContains { needle } => {
                        work.push((BodyRecord::Expr(needle), child_depth));
                    }
                    StageKind::WhereField { .. } | StageKind::Project { .. } => {}
                },
                BodyRecord::TemplatePart(part) => match part {
                    TemplatePart::Hole(expr) | TemplatePart::JsonStr(expr) => {
                        work.push((BodyRecord::Expr(expr), child_depth));
                    }
                    TemplatePart::OptionField { access, .. }
                    | TemplatePart::OptionStructField { access, .. }
                    | TemplatePart::StructArrayField { access, .. }
                    | TemplatePart::ScalarArrayField { access, .. }
                    | TemplatePart::UnionValue { access, .. } => {
                        work.push((BodyRecord::Expr(access), child_depth));
                    }
                    TemplatePart::Text(_) | TemplatePart::PopComma => {}
                },
                BodyRecord::Expr(expr) => match &expr.kind {
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
                    | ExprKind::HeapNew(expr)
                    | ExprKind::BoxGet(expr)
                    | ExprKind::BoxClone(expr)
                    | ExprKind::StrClone(expr)
                    | ExprKind::StrBorrow(expr)
                    | ExprKind::BuilderToString(expr)
                    | ExprKind::ArrayToSlice(expr)
                    | ExprKind::Len(expr)
                    | ExprKind::ArrayBuilderBuild(expr) => {
                        work.push((BodyRecord::Expr(expr), child_depth));
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
                    | ExprKind::ArrayDot { a: lhs, b: rhs, .. }
                    | ExprKind::ArrayChunks {
                        source: lhs,
                        n: rhs,
                        ..
                    }
                    | ExprKind::Index {
                        recv: lhs,
                        index: rhs,
                    }
                    | ExprKind::ElemField {
                        recv: lhs,
                        index: rhs,
                        ..
                    }
                    | ExprKind::JsonDocGet { doc: lhs, key: rhs }
                    | ExprKind::JsonDocAt {
                        doc: lhs,
                        index: rhs,
                    }
                    | ExprKind::JsonDocKey {
                        doc: lhs,
                        index: rhs,
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
                        work.push((BodyRecord::Expr(lhs), child_depth));
                        work.push((BodyRecord::Expr(rhs), child_depth));
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
                    } => {
                        work.extend(
                            operands
                                .iter()
                                .map(|operand| (BodyRecord::Expr(operand), child_depth)),
                        );
                    }
                    ExprKind::CallFnValue { callee, args } => {
                        work.push((BodyRecord::Expr(callee), child_depth));
                        work.extend(args.iter().map(|arg| (BodyRecord::Expr(arg), child_depth)));
                    }
                    ExprKind::TaskGroup(block)
                    | ExprKind::Block(block)
                    | ExprKind::Arena(block)
                    | ExprKind::Unsafe(block) => {
                        work.push((BodyRecord::Block(block), child_depth));
                    }
                    ExprKind::Loop { body, .. } => {
                        work.push((BodyRecord::Block(body), child_depth));
                    }
                    ExprKind::Match { scrutinee, arms } => {
                        work.push((BodyRecord::Expr(scrutinee), child_depth));
                        work.extend(arms.iter().map(|arm| {
                            (
                                BodyRecord::MatchArm { scrutinee, arm },
                                child_depth,
                            )
                        }));
                    }
                    ExprKind::Spawn { closure, .. } => {
                        work.push((BodyRecord::Expr(closure), child_depth));
                    }
                    ExprKind::If { cond, then, els } => {
                        work.push((BodyRecord::Expr(cond), child_depth));
                        work.push((BodyRecord::Block(then), child_depth));
                        work.push((BodyRecord::Block(els), child_depth));
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
                    | ExprKind::CryptoHash { data: recv, .. } => {
                        work.push((BodyRecord::Expr(recv), child_depth));
                    }
                    ExprKind::ElseUnwrap { opt, fallback } => {
                        work.push((BodyRecord::Expr(opt), child_depth));
                        work.push((BodyRecord::Expr(fallback), child_depth));
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
                        work.push((BodyRecord::Expr(ptr), child_depth));
                        work.push((BodyRecord::Expr(offset), child_depth));
                        work.push((BodyRecord::Expr(value), child_depth));
                    }
                    ExprKind::BuilderNew { capacity } => {
                        if let Some(value) = capacity.as_deref() {
                            work.push((BodyRecord::Expr(value), child_depth));
                        }
                    }
                    ExprKind::RegexFind { regex, text, start } => {
                        work.push((BodyRecord::Expr(regex), child_depth));
                        work.push((BodyRecord::Expr(text), child_depth));
                        if let Some(start) = start.as_deref() {
                            work.push((BodyRecord::Expr(start), child_depth));
                        }
                    }
                    ExprKind::CliFlag {
                        cmd, name, default, ..
                    } => {
                        work.push((BodyRecord::Expr(cmd), child_depth));
                        work.push((BodyRecord::Expr(name), child_depth));
                        if let Some(default) = default.as_deref() {
                            work.push((BodyRecord::Expr(default), child_depth));
                        }
                    }
                    ExprKind::SliceRange { recv, start, end } => {
                        work.push((BodyRecord::Expr(recv), child_depth));
                        if let Some(start) = start.as_deref() {
                            work.push((BodyRecord::Expr(start), child_depth));
                        }
                        if let Some(end) = end.as_deref() {
                            work.push((BodyRecord::Expr(end), child_depth));
                        }
                    }
                    ExprKind::VecSumWhere { vec, mask } => {
                        work.push((BodyRecord::Expr(vec), child_depth));
                        work.push((BodyRecord::Expr(mask), child_depth));
                    }
                    ExprKind::VecDot { a, b } => {
                        work.push((BodyRecord::Expr(a), child_depth));
                        work.push((BodyRecord::Expr(b), child_depth));
                    }
                    ExprKind::VecMinMax { vec, .. } | ExprKind::VecSum { vec } => {
                        work.push((BodyRecord::Expr(vec), child_depth));
                    }
                    ExprKind::VecLoad { src, index, .. } => {
                        work.push((BodyRecord::Expr(src), child_depth));
                        work.push((BodyRecord::Expr(index), child_depth));
                    }
                    ExprKind::ArraySum { source, stages }
                    | ExprKind::ArrayCount { source, stages }
                    | ExprKind::ArrayMinMax { source, stages, .. }
                    | ExprKind::ArraySort { source, stages, .. }
                    | ExprKind::ArrayToArray { source, stages, .. } => {
                        work.push((BodyRecord::Expr(source), child_depth));
                        work.extend(
                            stages
                                .iter()
                                .map(|stage| (BodyRecord::Stage(stage), child_depth)),
                        );
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
                        work.push((BodyRecord::Expr(source), child_depth));
                        work.extend(
                            stages
                                .iter()
                                .map(|stage| (BodyRecord::Stage(stage), child_depth)),
                        );
                        work.extend(
                            captures
                                .iter()
                                .map(|capture| (BodyRecord::Expr(capture), child_depth)),
                        );
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
                        work.push((BodyRecord::Expr(source), child_depth));
                        work.extend(
                            stages
                                .iter()
                                .map(|stage| (BodyRecord::Stage(stage), child_depth)),
                        );
                        work.extend(
                            captures
                                .iter()
                                .map(|capture| (BodyRecord::Expr(capture), child_depth)),
                        );
                        work.push((BodyRecord::Expr(init), child_depth));
                    }
                    ExprKind::ArrayMapInto {
                        source,
                        stages,
                        dst,
                        ..
                    } => {
                        work.push((BodyRecord::Expr(source), child_depth));
                        work.extend(
                            stages
                                .iter()
                                .map(|stage| (BodyRecord::Stage(stage), child_depth)),
                        );
                        work.push((BodyRecord::Expr(dst), child_depth));
                    }
                    ExprKind::Template(parts) => {
                        work.extend(
                            parts
                                .iter()
                                .map(|part| (BodyRecord::TemplatePart(part), child_depth)),
                        );
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
                        work.push((BodyRecord::Expr(file), child_depth));
                        work.push((BodyRecord::Expr(buffer), child_depth));
                        work.push((BodyRecord::Expr(offset), child_depth));
                    }
                    ExprKind::BytesRead { bytes, offset, .. } => {
                        work.push((BodyRecord::Expr(bytes), child_depth));
                        work.push((BodyRecord::Expr(offset), child_depth));
                    }
                    ExprKind::UdpSendTo {
                        sock,
                        data,
                        host,
                        port,
                    } => {
                        work.push((BodyRecord::Expr(sock), child_depth));
                        work.push((BodyRecord::Expr(data), child_depth));
                        work.push((BodyRecord::Expr(host), child_depth));
                        work.push((BodyRecord::Expr(port), child_depth));
                    }
                    ExprKind::CommandEnv {
                        command,
                        name,
                        value,
                    } => {
                        work.push((BodyRecord::Expr(command), child_depth));
                        work.push((BodyRecord::Expr(name), child_depth));
                        work.push((BodyRecord::Expr(value), child_depth));
                    }
                    ExprKind::RandRange { rng, lo, hi } => {
                        work.push((BodyRecord::Expr(rng), child_depth));
                        work.push((BodyRecord::Expr(lo), child_depth));
                        work.push((BodyRecord::Expr(hi), child_depth));
                    }
                    ExprKind::RandShuffle { rng, xs, .. } => {
                        work.push((BodyRecord::Expr(rng), child_depth));
                        work.push((BodyRecord::Expr(xs), child_depth));
                    }
                    ExprKind::RandSample { rng, xs, k, .. } => {
                        work.push((BodyRecord::Expr(rng), child_depth));
                        work.push((BodyRecord::Expr(xs), child_depth));
                        work.push((BodyRecord::Expr(k), child_depth));
                    }
                    ExprKind::HttpGetMany {
                        client,
                        urls,
                        max_concurrency,
                    } => {
                        work.push((BodyRecord::Expr(client), child_depth));
                        work.push((BodyRecord::Expr(urls), child_depth));
                        work.push((BodyRecord::Expr(max_concurrency), child_depth));
                    }
                    ExprKind::HttpServe { host, port, .. } => {
                        work.push((BodyRecord::Expr(host), child_depth));
                        work.push((BodyRecord::Expr(port), child_depth));
                    }
                    ExprKind::CryptoHkdf {
                        salt,
                        ikm,
                        info,
                        len,
                    } => {
                        work.push((BodyRecord::Expr(salt), child_depth));
                        work.push((BodyRecord::Expr(ikm), child_depth));
                        work.push((BodyRecord::Expr(info), child_depth));
                        work.push((BodyRecord::Expr(len), child_depth));
                    }
                    ExprKind::CryptoAead {
                        key,
                        nonce,
                        input,
                        aad,
                        ..
                    } => {
                        work.push((BodyRecord::Expr(key), child_depth));
                        work.push((BodyRecord::Expr(nonce), child_depth));
                        work.push((BodyRecord::Expr(input), child_depth));
                        work.push((BodyRecord::Expr(aad), child_depth));
                    }
                    ExprKind::CryptoArgon2 {
                        password,
                        salt,
                        params,
                    } => {
                        work.push((BodyRecord::Expr(password), child_depth));
                        work.push((BodyRecord::Expr(salt), child_depth));
                        work.push((BodyRecord::Expr(params), child_depth));
                    }
                },
            }
            let mut expression_children_completed = true;
            if reachable_only {
                let first_diverging = work[child_start..]
                    .iter()
                    .position(|(child, _)| match child {
                        BodyRecord::Stmt(statement) => {
                            crate::hir_stmt_diverges(statement)
                        }
                        BodyRecord::Expr(expression) => {
                            crate::hir_expr_diverges(expression)
                        }
                        BodyRecord::Block(_)
                        | BodyRecord::StmtExit(_)
                        | BodyRecord::ExprExit { .. }
                        | BodyRecord::MatchArm { .. }
                        | BodyRecord::Stage(_)
                        | BodyRecord::TemplatePart(_) => false,
                    });
                if let Some(index) = first_diverging {
                    expression_children_completed = !matches!(
                        work[child_start + index].0,
                        BodyRecord::Expr(_)
                    );
                    work.truncate(child_start + index + 1);
                }
            }
            if let Some(stmt) = exit_stmt {
                work.push((BodyRecord::StmtExit(stmt), depth));
            }
            if let Some(expr) = exit_expr {
                work.push((
                    BodyRecord::ExprExit {
                        expression: expr,
                        children_completed: expression_children_completed,
                    },
                    depth,
                ));
            }
            // Children were appended in producer order. Reverse only this record's suffix so the
            // LIFO worklist completes the first child before starting the next one.
            work[child_start..].reverse();
        }
    true
}

/// Stable child-first expression order for one mutable HIR root.
///
/// Collection itself holds only shared references. The returned pointers are used after the walk
/// completes; finalization changes types and metadata but never replaces an `Expr`, child vector, or
/// block, so their addresses stay valid for the duration of that pass.
pub(crate) fn expr_postorder_mut(root: &mut Expr) -> Vec<*mut Expr> {
    let mut events = Vec::new();
    let root = &*root;
    let valid = walk_body_records(
        BodyRecord::Expr(root),
        usize::MAX,
        Some(&mut events),
        false,
    );
    debug_assert!(valid);
    events
        .into_iter()
        .filter_map(|event| match event {
            BodyEvent::ExprExit { expression, .. } => {
                Some(expression as *const Expr as *mut Expr)
            }
            BodyEvent::StmtEnter(_)
            | BodyEvent::StmtExit(_)
            | BodyEvent::ExprEnter(_)
            | BodyEvent::MatchArmEnter { .. } => None,
        })
        .collect()
}

/// Stable child-first expression order for a shared HIR root.
pub(crate) fn expr_postorder(root: &Expr) -> Vec<&Expr> {
    let mut events = Vec::new();
    let valid = walk_body_records(
        BodyRecord::Expr(root),
        usize::MAX,
        Some(&mut events),
        false,
    );
    debug_assert!(valid);
    events
        .into_iter()
        .filter_map(|event| match event {
            BodyEvent::ExprExit { expression, .. } => Some(expression),
            BodyEvent::StmtEnter(_)
            | BodyEvent::StmtExit(_)
            | BodyEvent::ExprEnter(_)
            | BodyEvent::MatchArmEnter { .. } => None,
        })
        .collect()
}

pub(crate) fn body_events(root: &Block) -> Vec<BodyEvent<'_>> {
    let mut events = Vec::new();
    let valid = walk_body_records(
        BodyRecord::Block(root),
        usize::MAX,
        Some(&mut events),
        false,
    );
    debug_assert!(valid);
    events
}

/// Source-order enter/exit events with unreachable siblings removed after a diverging child.
///
/// Alternative `if`/`match` branches remain present because their block/arm records are not
/// sequential expression children. Consumers can therefore replay all runtime possibilities while
/// skipping statements, arguments, captures, and tails that cannot be reached.
pub(crate) fn reachable_body_events(root: &Block) -> Vec<BodyEvent<'_>> {
    let mut events = Vec::new();
    let valid = walk_body_records(
        BodyRecord::Block(root),
        usize::MAX,
        Some(&mut events),
        true,
    );
    debug_assert!(valid);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IntTy, Ty};
    use align_ast::UnOp;
    use align_span::Span;

    #[derive(Clone, Copy, Debug)]
    enum Shape {
        BlockStmt,
        MatchArm,
        Stage,
        TemplatePart,
    }

    #[derive(Clone, Copy, Debug)]
    enum MoveControlShape {
        ShortCircuit,
        ElseUnwrap,
        If,
        LoopBreak,
    }

    fn int_ty() -> Ty {
        Ty::Int(IntTy {
            bits: 64,
            signed: true,
        })
    }

    fn leaf() -> Expr {
        Expr {
            kind: ExprKind::Int(0),
            ty: int_ty(),
            span: Span::new(0, 0, 0),
        }
    }

    fn wrap(shape: Shape, child: Expr) -> (Expr, usize) {
        let span = Span::new(0, 0, 0);
        let (kind, added_depth) = match shape {
            Shape::BlockStmt => (
                ExprKind::Block(Block {
                    stmts: vec![Stmt::Expr(child)],
                    value: None,
                }),
                3,
            ),
            Shape::MatchArm => (
                ExprKind::Match {
                    scrutinee: Box::new(leaf()),
                    arms: vec![MatchArm {
                        variants: Vec::new(),
                        bindings: Vec::new(),
                        body: child,
                    }],
                },
                2,
            ),
            Shape::Stage => (
                ExprKind::ArraySum {
                    source: Box::new(leaf()),
                    stages: vec![Stage {
                        kind: StageKind::Map {
                            func: "f".to_string(),
                            captures: vec![child],
                        },
                        out_ty: int_ty(),
                    }],
                },
                2,
            ),
            Shape::TemplatePart => (ExprKind::Template(vec![TemplatePart::Hole(child)]), 2),
        };
        (
            Expr {
                kind,
                ty: int_ty(),
                span,
            },
            added_depth,
        )
    }

    fn program_with_depth(shape: Shape, body_depth: usize) -> hir::Program {
        assert!(body_depth >= 2);
        let target_expr_depth = body_depth - 1;
        let delta = match shape {
            Shape::BlockStmt => 3,
            Shape::MatchArm | Shape::Stage | Shape::TemplatePart => 2,
        };
        let mut expr = leaf();
        let mut expr_depth = 1;
        while expr_depth + delta <= target_expr_depth {
            (expr, _) = wrap(shape, expr);
            expr_depth += delta;
        }
        while expr_depth < target_expr_depth {
            expr = Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expr),
                },
                ty: int_ty(),
                span: Span::new(0, 0, 0),
            };
            expr_depth += 1;
        }

        hir::Program {
            fns: vec![hir::Fn {
                name: "deep".to_string(),
                lifted_capture_count: None,
                params: Vec::new(),
                param_modes: Vec::new(),
                ret: int_ty(),
                return_borrow: hir::ReturnBorrowSummary::None,
                return_region: hir::ReturnRegionSummary::None,
                locals: Vec::new(),
                body: Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expr)),
                },
                span: Span::new(0, 0, 0),
                drop_locals: Vec::new(),
                drop_individual_locals: Vec::new(),
                drop_individual_exprs: Default::default(),
                exportable: false,
            }],
            externs: Vec::new(),
            link_libs: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            tagged_types: Vec::new(),
            tuples: Vec::new(),
            fn_types: Vec::new(),
            imported_fns: Vec::new(),
        }
    }

    fn move_program_at_boundary() -> hir::Program {
        let span = Span::new(0, 0, 0);
        let result_ty = Ty::Option(crate::Scalar::String);
        let mut expression = Expr {
            kind: ExprKind::OptionSome(Box::new(Expr {
                kind: ExprKind::Local(0),
                ty: Ty::String,
                span,
            })),
            ty: result_ty,
            span,
        };
        let mut expression_depth = 2;
        while expression_depth < MAX_CHECKED_HIR_DEPTH - 1 {
            expression = Expr {
                kind: ExprKind::Block(Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expression)),
                }),
                ty: result_ty,
                span,
            };
            expression_depth += 2;
        }
        assert_eq!(expression_depth, MAX_CHECKED_HIR_DEPTH - 1);
        hir::Program {
            fns: vec![hir::Fn {
                name: "deep_move".to_string(),
                lifted_capture_count: None,
                params: vec![0],
                param_modes: vec![align_ast::ParamMode::ByValue],
                ret: result_ty,
                return_borrow: hir::ReturnBorrowSummary::None,
                return_region: hir::ReturnRegionSummary::None,
                locals: vec![hir::Local {
                    id: 0,
                    name: "value".to_string(),
                    ty: Ty::String,
                    is_mut: false,
                    is_param: true,
                    align: None,
                }],
                body: Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expression)),
                },
                span,
                drop_locals: Vec::new(),
                drop_individual_locals: Vec::new(),
                drop_individual_exprs: Default::default(),
                exportable: false,
            }],
            externs: Vec::new(),
            link_libs: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            tagged_types: Vec::new(),
            tuples: Vec::new(),
            fn_types: Vec::new(),
            imported_fns: Vec::new(),
        }
    }

    fn move_call_program_at_boundary() -> hir::Program {
        let span = Span::new(0, 0, 0);
        let mut expression = Expr {
            kind: ExprKind::Local(0),
            ty: Ty::String,
            span,
        };
        let mut expression_depth = 1;
        while expression_depth < MAX_CHECKED_HIR_DEPTH - 1 {
            expression = Expr {
                kind: ExprKind::Call {
                    func: "consume".to_string(),
                    args: vec![
                        Expr {
                            kind: ExprKind::Int(0),
                            ty: int_ty(),
                            span,
                        },
                        expression,
                    ],
                    type_args: Vec::new(),
                },
                ty: Ty::String,
                span,
            };
            expression_depth += 1;
        }
        assert_eq!(expression_depth, MAX_CHECKED_HIR_DEPTH - 1);
        hir::Program {
            fns: vec![hir::Fn {
                name: "deep_move_call".to_string(),
                lifted_capture_count: None,
                params: vec![0],
                param_modes: vec![align_ast::ParamMode::ByValue],
                ret: Ty::String,
                return_borrow: hir::ReturnBorrowSummary::None,
                return_region: hir::ReturnRegionSummary::None,
                locals: vec![hir::Local {
                    id: 0,
                    name: "value".to_string(),
                    ty: Ty::String,
                    is_mut: false,
                    is_param: true,
                    align: None,
                }],
                body: Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expression)),
                },
                span,
                drop_locals: Vec::new(),
                drop_individual_locals: Vec::new(),
                drop_individual_exprs: Default::default(),
                exportable: false,
            }],
            externs: Vec::new(),
            link_libs: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            tagged_types: Vec::new(),
            tuples: Vec::new(),
            fn_types: Vec::new(),
            imported_fns: Vec::new(),
        }
    }

    fn move_shape_program_at_boundary(shape: Shape) -> hir::Program {
        let span = Span::new(0, 0, 0);
        let target_expression_depth = MAX_CHECKED_HIR_DEPTH - 1;
        let delta = match shape {
            Shape::BlockStmt => 3,
            Shape::MatchArm | Shape::Stage | Shape::TemplatePart => 2,
        };
        let mut expression = Expr {
            kind: ExprKind::Local(0),
            ty: Ty::String,
            span,
        };
        let mut expression_depth = 1;
        while expression_depth + delta <= target_expression_depth {
            (expression, _) = wrap(shape, expression);
            expression_depth += delta;
        }
        while expression_depth < target_expression_depth {
            expression = Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expression),
                },
                ty: int_ty(),
                span,
            };
            expression_depth += 1;
        }
        assert_eq!(expression_depth, target_expression_depth);
        let ret = expression.ty;
        hir::Program {
            fns: vec![hir::Fn {
                name: "deep_move_shape".to_string(),
                lifted_capture_count: None,
                params: vec![0],
                param_modes: vec![align_ast::ParamMode::ByValue],
                ret,
                return_borrow: hir::ReturnBorrowSummary::None,
                return_region: hir::ReturnRegionSummary::None,
                locals: vec![hir::Local {
                    id: 0,
                    name: "value".to_string(),
                    ty: Ty::String,
                    is_mut: false,
                    is_param: true,
                    align: None,
                }],
                body: Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expression)),
                },
                span,
                drop_locals: Vec::new(),
                drop_individual_locals: Vec::new(),
                drop_individual_exprs: Default::default(),
                exportable: false,
            }],
            externs: Vec::new(),
            link_libs: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            tagged_types: Vec::new(),
            tuples: Vec::new(),
            fn_types: Vec::new(),
            imported_fns: Vec::new(),
        }
    }

    fn move_control_program_at_boundary(
        shape: MoveControlShape,
    ) -> hir::Program {
        let span = Span::new(0, 0, 0);
        let target_expression_depth = MAX_CHECKED_HIR_DEPTH - 1;
        let delta = match shape {
            MoveControlShape::ShortCircuit
            | MoveControlShape::ElseUnwrap => 1,
            MoveControlShape::If => 2,
            MoveControlShape::LoopBreak => 3,
        };
        let mut expression = Expr {
            kind: ExprKind::StrBorrow(Box::new(Expr {
                kind: ExprKind::Local(0),
                ty: Ty::String,
                span,
            })),
            ty: Ty::Str,
            span,
        };
        let mut expression_depth = 2;
        while expression_depth + delta <= target_expression_depth {
            let kind = match shape {
                MoveControlShape::ShortCircuit => ExprKind::Binary {
                    op: crate::BinOp::And,
                    lhs: Box::new(Expr {
                        kind: ExprKind::Bool(true),
                        ty: Ty::Bool,
                        span,
                    }),
                    rhs: Box::new(expression),
                },
                MoveControlShape::ElseUnwrap => ExprKind::ElseUnwrap {
                    opt: Box::new(Expr {
                        kind: ExprKind::OptionNone,
                        ty: Ty::Option(crate::Scalar::Int(IntTy {
                            bits: 64,
                            signed: true,
                        })),
                        span,
                    }),
                    fallback: Box::new(expression),
                },
                MoveControlShape::If => ExprKind::If {
                    cond: Box::new(Expr {
                        kind: ExprKind::Bool(true),
                        ty: Ty::Bool,
                        span,
                    }),
                    then: Block {
                        stmts: Vec::new(),
                        value: Some(Box::new(expression)),
                    },
                    els: Block {
                        stmts: Vec::new(),
                        value: Some(Box::new(leaf())),
                    },
                },
                MoveControlShape::LoopBreak => ExprKind::Loop {
                    body: Block {
                        stmts: vec![Stmt::Break {
                            value: Some(expression),
                            accepted: true,
                        }],
                        value: None,
                    },
                    diverges: false,
                    body_locals: 0..0,
                },
            };
            expression = Expr {
                kind,
                ty: match shape {
                    MoveControlShape::ShortCircuit => Ty::Bool,
                    MoveControlShape::ElseUnwrap
                    | MoveControlShape::If
                    | MoveControlShape::LoopBreak => int_ty(),
                },
                span,
            };
            expression_depth += delta;
        }
        while expression_depth < target_expression_depth {
            expression = Expr {
                kind: ExprKind::Unary {
                    op: UnOp::Neg,
                    expr: Box::new(expression),
                },
                ty: int_ty(),
                span,
            };
            expression_depth += 1;
        }
        assert_eq!(expression_depth, target_expression_depth);
        let ret = expression.ty;
        hir::Program {
            fns: vec![hir::Fn {
                name: "deep_move_control".to_string(),
                lifted_capture_count: None,
                params: vec![0],
                param_modes: vec![align_ast::ParamMode::ByValue],
                ret,
                return_borrow: hir::ReturnBorrowSummary::None,
                return_region: hir::ReturnRegionSummary::None,
                locals: vec![hir::Local {
                    id: 0,
                    name: "value".to_string(),
                    ty: Ty::String,
                    is_mut: false,
                    is_param: true,
                    align: None,
                }],
                body: Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expression)),
                },
                span,
                drop_locals: Vec::new(),
                drop_individual_locals: Vec::new(),
                drop_individual_exprs: Default::default(),
                exportable: false,
            }],
            externs: Vec::new(),
            link_libs: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            tagged_types: Vec::new(),
            tuples: Vec::new(),
            fn_types: Vec::new(),
            imported_fns: Vec::new(),
        }
    }

    #[test]
    fn checked_hir_depth_closure_matrix() {
        for shape in [
            Shape::BlockStmt,
            Shape::MatchArm,
            Shape::Stage,
            Shape::TemplatePart,
        ] {
            for depth in [MAX_CHECKED_HIR_DEPTH - 1, MAX_CHECKED_HIR_DEPTH] {
                assert!(
                    checked_hir_body_depth_is_valid(&program_with_depth(shape, depth)),
                    "{shape:?} depth {depth} was rejected"
                );
            }
            let depth = MAX_CHECKED_HIR_DEPTH + 1;
            assert!(
                !checked_hir_body_depth_is_valid(&program_with_depth(shape, depth)),
                "{shape:?} depth {depth} was accepted"
            );
        }

        let program = program_with_depth(Shape::BlockStmt, MAX_CHECKED_HIR_DEPTH);
        std::thread::Builder::new()
            .name("checked-hir-divergence-depth".to_string())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let root = program.fns[0]
                    .body
                    .value
                    .as_deref()
                    .expect("deep body value");
                assert!(
                    !crate::hir_expr_diverges(root),
                    "nested expression statements do not make the body diverge"
                );
                let effects = crate::fn_effects(
                    &program,
                    &std::collections::HashMap::new(),
                );
                assert_eq!(
                    effects.get("deep"),
                    Some(&crate::FnEffect::Pure),
                    "the accepted checked-HIR boundary must scan effects without process-stack recursion"
                );

                let mut move_programs = vec![
                    move_program_at_boundary(),
                    move_call_program_at_boundary(),
                ];
                move_programs.extend([
                    move_shape_program_at_boundary(Shape::BlockStmt),
                    move_shape_program_at_boundary(Shape::MatchArm),
                    move_shape_program_at_boundary(Shape::Stage),
                    move_shape_program_at_boundary(Shape::TemplatePart),
                    move_control_program_at_boundary(
                        MoveControlShape::ShortCircuit,
                    ),
                    move_control_program_at_boundary(
                        MoveControlShape::ElseUnwrap,
                    ),
                    move_control_program_at_boundary(
                        MoveControlShape::If,
                    ),
                    move_control_program_at_boundary(
                        MoveControlShape::LoopBreak,
                    ),
                ]);
                for move_program in move_programs {
                    assert!(checked_hir_body_depth_is_valid(&move_program));
                    let mut diagnostics = crate::Diagnostics::new();
                    let named_return_borrow = std::collections::HashMap::new();
                    crate::MoveCheck {
                        f: &move_program.fns[0],
                        diags: &mut diagnostics,
                        named_return_borrow: &named_return_borrow,
                        summary_dependencies: None,
                        tuples: &move_program.tuples,
                        structs: &move_program.structs,
                        enums: &move_program.enums,
                        tagged_types: &move_program.tagged_types,
                        loop_breaks: Vec::new(),
                        borrows: crate::BorrowState::default(),
                        next_pipeline_snapshot: 0,
                        loop_borrow_breaks: Vec::new(),
                        loop_value_breaks: Vec::new(),
                        loop_value_facts: std::collections::HashMap::new(),
                        control_value_facts: std::collections::HashMap::new(),
                        walked_value_facts: std::collections::HashMap::new(),
                        value_snapshot_frames: Vec::new(),
                        reported_invalid_value_actions: std::collections::HashSet::new(),
                        loop_iter_drops: Vec::new(),
                        arena_depth: 0,
                        return_roots: crate::BorrowRoots::new(),
                        non_fallthrough: std::collections::HashSet::new(),
                        borrow_fact_cache: std::cell::RefCell::new(None),
                        collecting_move_children: false,
                        move_children: Vec::new(),
                    }
                    .check();
                    assert!(
                        !diagnostics.has_errors(),
                        "the accepted checked-HIR boundary must check moves and borrows without process-stack recursion"
                    );
                }

                let mut diagnostics = crate::Diagnostics::new();
                let named_return_region = std::collections::HashMap::new();
                {
                    let function = &program.fns[0];
                    let mut escape = crate::EscapeCheck {
                        f: function,
                        diags: &mut diagnostics,
                        named_return_region: &named_return_region,
                        tuples: &program.tuples,
                        structs: &program.structs,
                        enums: &program.enums,
                        tagged_types: &program.tagged_types,
                        state: crate::EscapeState::default(),
                        drop_region: std::collections::HashMap::new(),
                        drop_individual: std::collections::HashMap::new(),
                        drop_individual_exprs: std::collections::HashMap::new(),
                        decl_depth: std::collections::HashMap::new(),
                        task_group_regions: Vec::new(),
                        allocation_regions: Vec::new(),
                        allocation_region_by_expr: std::collections::HashMap::new(),
                        flow: crate::EscapeFlowCfg::new(),
                        flow_current: 0,
                        loop_exit_blocks: Vec::new(),
                        collecting_walk_children: false,
                        walk_children: Vec::new(),
                    };
                    escape.check();
                }
                assert!(
                    !diagnostics.has_errors(),
                    "the accepted checked-HIR boundary must build and solve escape flow without process-stack recursion"
                );
            })
            .expect("spawn checked-HIR divergence owner")
            .join()
            .expect("checked-HIR divergence owner");
    }
}
