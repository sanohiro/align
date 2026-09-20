//! Loop facts: the induction structure of a loop, and the bounds checks that follow from it.
//!
//! Plan 69 §§3.2 and 4 (`docs/impl/69-loop-facts-plan.md`), implementing plan 68's G2 movement and
//! G3 counted-loop shape. The
//! guard *fusion* half lives in lowering (`emit_bounds_check` / `emit_range_bounds_check`); this
//! module never deletes a check. It **versions** a loop: an admission test in a new preheader
//! selects between a fast copy whose proved guards are bypassed and the original slow copy, which
//! keeps every guard. A failing run therefore takes the slow copy and traps at the same iteration,
//! with the same arguments, after the same observable prefix (§3.3).
//!
//! The pass is registered immediately after [`crate::byte_ranges::simplify`] in
//! `lower_program_checked_with_catalog` and is fail-closed in the same way: derive, rewrite,
//! re-derive from the rewritten function, and discard the whole rewrite when the re-derived facts
//! disagree. A versioned fast copy with a body-derived exit is also rotated: its zero-trip test is
//! peeled, its stepped trip test is placed at the latch, and a named record carries its recurrence
//! and owned-view extents to codegen. It runs after `annotate_par_map_work` in every lowering entry
//! point (invariant I8), so
//! a `par_map` work weight is derived from the unversioned body and is byte-identical with and
//! without versioning.
//!
//! Every classifier over [`Stmt`] and [`Rvalue`] below is exhaustive with **no wildcard arm**, and
//! [`variant_sweep_tripwire`] pins that inventory a second time: a new IR variant is a build error
//! here rather than a silent "this statement does not mutate anything".

use crate::{
    BlockId, Const, DirectCall, Function, Operand, RuntimeKey, Rvalue, Slot, Stmt, Term, ValueId,
};
use align_ast::{BinOp, ParamMode};
use align_sema::{IntTy, Ty};
use std::collections::{BTreeMap, BTreeSet};

/// The node budget one source loop may spend on versioning, counted in body statements **including
/// terminators** (§3.2.2). A body over budget is not admitted and the refusal is reported, so the
/// code-growth bound is an asserted property rather than a hope. A versioned inner loop contributes
/// both of its copies to the enclosing body's count, because the enclosing loop is considered after
/// it in the innermost-first order.
pub const LOOP_FACTS_VERSION_BUDGET: usize = 256;

/// The largest function `loop_facts` analyses. Dominance is a dense bit matrix, so a pathological
/// machine-generated body is refused rather than paid for quadratically.
const MAX_ANALYSED_BLOCKS: usize = 1024;

fn i64_ty() -> Ty {
    Ty::Int(IntTy {
        bits: 64,
        signed: true,
    })
}

fn u64_ty() -> Ty {
    Ty::Int(IntTy {
        bits: 64,
        signed: false,
    })
}

fn int(value: i128) -> Operand {
    Operand::Const(Const::Int(value, i64_ty()))
}

/// What `loop_facts` decided about one source loop, in innermost-first order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoopDecision {
    /// Versioned: the fast copy bypasses every proved guard, the slow copy keeps all of them.
    Versioned { budget_used: usize },
    /// Not versioned; the loop keeps its in-loop (fused) guards exactly as lowering emitted them.
    KeptChecks(KeptReason),
}

/// Why one loop kept its checks. The **code** is the stable observable surface (§3.2.3); the prose
/// `explain-opt` wraps around it is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeptReason {
    /// The body contains a `Stmt::BorrowedElementReservation`, whose token is function-unique and
    /// whose guard codegen re-derives literally. Lifting this is plan 69 §3.6.
    BorrowedElement,
    /// The candidate index slot is not a single monotone recurrence: more than one statement writes
    /// it (including from a nested loop), the step is not `i = i + <positive constant>`, or the
    /// slot's address is handed to a callee anywhere in the function, which could write it through
    /// a pointer no statement kind names.
    MultipleIndexWrites,
    /// The step can execute before a guarded access on some path, so the value reaching that access
    /// is not the value the header saw (the `shifted_sum` witness).
    StepPrecedesAccess,
    /// The entry value of the index is not a loop-invariant operand proved non-negative.
    EntryUnproved,
    /// The trip-count bound is missing, not loop-invariant, or killed in the body.
    BoundKilled,
    /// A guarded index is not `a*i + b` with `a` a positive constant and `b` loop-invariant.
    AccessNotAffine,
    /// A statement of the named kind writes a root the guard depends on, or an operand deriving
    /// from it. The `drain` witness is `ArrayTruncate`.
    RootKilled(&'static str),
    /// An operand the admission arithmetic needs cannot be rematerialized in the preheader.
    ArithmeticUnproved,
    /// The body exceeds [`LOOP_FACTS_VERSION_BUDGET`].
    OverBudget,
    /// A guard in the body is not in the fused unsigned form — a borrowed-element guard, or a byte
    /// accessor guard, which `byte_ranges` owns and re-derives in its signed form (§3.7).
    GuardNotFused,
    /// A value defined in the body is used after the loop, so a second copy of the body would leave
    /// that use dominated by neither copy (§3.7).
    ValueEscapesLoop,
    /// The body contains a nested loop, so a guarded access in it does not run once per value of
    /// this loop's index (§3.7).
    NestedLoop,
    /// The loop's CFG is not the single-latch reducible shape this pass versions (§3.7).
    LoopShape,
    /// The body, or the code after the loop, contains a statement this pass does not model. The
    /// fail-closed default (§3.7).
    UnmodelledStatement,
    /// The body holds no fused guard to prove, so versioning would only duplicate code (§3.7).
    NoProvableGuard,
    /// The rewrite did not re-derive to the facts it was built from, so it was rolled back (§3.2).
    RederivationFailed,
}

impl KeptReason {
    /// The stable reason code `explain-opt` prints.
    pub fn code(&self) -> String {
        match self {
            KeptReason::BorrowedElement => "borrowed-element".to_string(),
            KeptReason::MultipleIndexWrites => "multiple-index-writes".to_string(),
            KeptReason::StepPrecedesAccess => "step-precedes-access".to_string(),
            KeptReason::EntryUnproved => "entry-unproved".to_string(),
            KeptReason::BoundKilled => "bound-killed".to_string(),
            KeptReason::AccessNotAffine => "access-not-affine".to_string(),
            KeptReason::RootKilled(kind) => format!("root-killed:{kind}"),
            KeptReason::ArithmeticUnproved => "arithmetic-unproved".to_string(),
            KeptReason::OverBudget => "over-budget".to_string(),
            KeptReason::GuardNotFused => "guard-not-fused".to_string(),
            KeptReason::ValueEscapesLoop => "value-escapes-loop".to_string(),
            KeptReason::NestedLoop => "nested-loop".to_string(),
            KeptReason::LoopShape => "loop-shape".to_string(),
            KeptReason::UnmodelledStatement => "unmodelled-statement".to_string(),
            KeptReason::NoProvableGuard => "no-provable-guard".to_string(),
            KeptReason::RederivationFailed => "rederivation-failed".to_string(),
        }
    }
}

/// One function's per-loop decisions, in innermost-first order. Published on [`crate::Program`] for
/// `explain-opt` and deliberately absent from MIR text, so it reaches no artifact identity.
#[derive(Clone, Debug, Default)]
pub struct FunctionDecisions {
    pub function: String,
    pub loops: Vec<LoopDecision>,
    /// Recognized counted loops after rewriting. Codegen consumes only these producer-owned
    /// records when placing the narrow data-buffer extent assumption from plan 69 §4.2.
    pub counted: Vec<CountedLoopFact>,
}

/// The codegen-facing part of one recognized counted loop. `preheader` is reached only after the
/// peeled zero-trip test and exactly once on the rotated path.
#[derive(Clone, Debug)]
pub struct CountedLoopFact {
    pub preheader: BlockId,
    pub header: BlockId,
    pub latch: BlockId,
    pub exit: BlockId,
    pub entry: Operand,
    pub trip_count: Operand,
    pub index: Slot,
    pub step: i64,
    pub relation: BinOp,
    pub bound: Operand,
    pub extents: Vec<DataExtent>,
    stepped: Operand,
    latch_bound: Operand,
}

impl CountedLoopFact {
    /// Keep emission-scoped facts aligned when a preparation pass renumbers the owning function's
    /// blocks. A missing source id invalidates the whole record instead of guessing a placement.
    pub(crate) fn remap_blocks(&mut self, remap: &[BlockId]) -> bool {
        let Some(preheader) = remap.get(self.preheader as usize).copied() else {
            return false;
        };
        let Some(header) = remap.get(self.header as usize).copied() else {
            return false;
        };
        let Some(latch) = remap.get(self.latch as usize).copied() else {
            return false;
        };
        let Some(exit) = remap.get(self.exit as usize).copied() else {
            return false;
        };
        self.preheader = preheader;
        self.header = header;
        self.latch = latch;
        self.exit = exit;
        true
    }
}

/// One Align-owned view whose data buffer is accessible for `len * sizeof(elem)` bytes.
#[derive(Clone, Debug)]
pub struct DataExtent {
    pub view: Operand,
    pub len: Operand,
    pub elem: align_sema::Scalar,
}

impl FunctionDecisions {
    /// The `explain-opt` lines for this function (§3.2.3). One line per source loop, ordinal in
    /// decision order, with the budget use on an admitted loop and the stable code on a refusal.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for (index, decision) in self.loops.iter().enumerate() {
            let ordinal = index + 1;
            match decision {
                LoopDecision::Versioned { budget_used } => {
                    let _ = writeln!(
                        out,
                        "loop facts `{}` #{ordinal}: loop versioned {budget_used}/{LOOP_FACTS_VERSION_BUDGET}",
                        self.function
                    );
                }
                LoopDecision::KeptChecks(reason) => {
                    let _ = writeln!(
                        out,
                        "loop facts `{}` #{ordinal}: loop kept checks: {}",
                        self.function,
                        reason.code()
                    );
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// IR inventory: the exhaustive, wildcard-free classifiers and their tripwire.
// ---------------------------------------------------------------------------------------------

/// The [`Rvalue`] variants this pass does not model. Written once and matched twice so the two
/// classifiers below can never disagree about coverage, and **without a wildcard** so a new variant
/// is a build error in both.
macro_rules! unmodelled_rvalues {
    () => {
        Rvalue::SqliteCallbackDescriptor { .. } | Rvalue::CallWithCleanup { .. } | Rvalue::Closure { .. } |
        Rvalue::RawCall { .. } | Rvalue::CallIndirectWithCleanup { .. } | Rvalue::MakeError { .. } |
        Rvalue::ArenaBegin { .. } | Rvalue::TgBegin { .. } | Rvalue::SpawnTask { .. } |
        Rvalue::TgWaitResult { .. } | Rvalue::HeapAlloc { .. } | Rvalue::RawAlloc { .. } |
        Rvalue::ColumnBatchCreate { .. } | Rvalue::ColumnBatchAppend { .. } | Rvalue::ColumnBatchRow { .. } |
        Rvalue::ColumnBatchSoa { .. } | Rvalue::StaticDescriptorView { .. } | Rvalue::ResourceFromRaw { .. } |
        Rvalue::ResourceBorrow { .. } | Rvalue::ResourceRaw { .. } | Rvalue::ResourceIntoRaw { .. } |
        Rvalue::ResourceViewFromRaw { .. } | Rvalue::BoxClone { .. } | Rvalue::IndexPtr { .. } |
        Rvalue::ArenaAlloc { .. } | Rvalue::HeapAllocBuf { .. } | Rvalue::SoaAlloc { .. } |
        Rvalue::MakeDynArray { .. } | Rvalue::GroupAgg { .. } | Rvalue::GroupAggStrCols { .. } |
        Rvalue::GroupAggStr { .. } | Rvalue::GroupAggMultiStr { .. } | Rvalue::DictEncode { .. } |
        Rvalue::MakeDictEncoded { .. } | Rvalue::DictField { .. } | Rvalue::GatherColumnI64 { .. } |
        Rvalue::DictLookup { .. } | Rvalue::Chunks { .. } | Rvalue::ParMapParallel { .. } |
        Rvalue::ParMapReduce { .. } | Rvalue::StaticData { .. } | Rvalue::StrClone { .. } |
        Rvalue::CloneIn { .. } | Rvalue::StrPredicate { .. } | Rvalue::StrFinderNew { .. } |
        Rvalue::StrFinderFind { .. } | Rvalue::StrTrim { .. } | Rvalue::BuilderNew { .. } |
        Rvalue::BuilderWriteStr { .. } | Rvalue::BuilderWriteInt { .. } | Rvalue::BuilderWriteBool { .. } |
        Rvalue::BuilderWriteChar { .. } | Rvalue::BuilderWriteFloat { .. } | Rvalue::BuilderWriteStrIntStr { .. } |
        Rvalue::BuilderToString { .. } | Rvalue::TemplateHtmlNew { .. } | Rvalue::TemplateHtmlWrite { .. } |
        Rvalue::TemplateHtmlRaw { .. } | Rvalue::TemplateHtmlToString { .. } | Rvalue::Template { .. } |
        Rvalue::JsonEncode { .. } | Rvalue::JsonDecode { .. } | Rvalue::JsonOwnedDecode { .. } |
        Rvalue::JsonDecodeArray { .. } | Rvalue::JsonDecodeScalar { .. } | Rvalue::JsonDecodeStructArray { .. } |
        Rvalue::JsonDecodeSoa { .. } | Rvalue::CsvDecode { .. } | Rvalue::JsonDecodeUnion { .. } |
        Rvalue::JsonDoc { .. } | Rvalue::JsonDocKind { .. } | Rvalue::JsonDocGet { .. } |
        Rvalue::JsonDocAt { .. } | Rvalue::JsonDocAsStr { .. } | Rvalue::JsonDocAsScalar { .. } |
        Rvalue::JsonDocLen { .. } | Rvalue::JsonDocKey { .. } | Rvalue::JsonDocElems { .. } |
        Rvalue::JsonScanNew { .. } | Rvalue::JsonScanNext { .. } | Rvalue::FsReadFile { .. } |
        Rvalue::FsCreatePrivateTempDir { .. } | Rvalue::ReaderOpen { .. } | Rvalue::ReaderOpenBeneath { .. } |
        Rvalue::ReaderOpenBeneathSingleLink { .. } | Rvalue::WriterCreate { .. } | Rvalue::WriterCreateExclusive { .. } |
        Rvalue::WriterCreateExclusiveBeneath { .. } | Rvalue::ReaderStdin { .. } | Rvalue::WriterStd { .. } |
        Rvalue::ReaderRead { .. } | Rvalue::ReaderBuffered { .. } | Rvalue::ReaderReadLine { .. } |
        Rvalue::BytesAsStr { .. } | Rvalue::WriterWrite { .. } | Rvalue::WriterWriteBuilder { .. } |
        Rvalue::WriterFlush { .. } | Rvalue::LogNew { .. } | Rvalue::LogEnabled { .. } |
        Rvalue::LogLine { .. } | Rvalue::LogLineBuilder { .. } | Rvalue::LogFlush { .. } |
        Rvalue::XmlParse { .. } | Rvalue::XmlNext { .. } | Rvalue::XmlName { .. } |
        Rvalue::XmlAttributeCount { .. } | Rvalue::XmlAttributeName { .. } | Rvalue::XmlAttributeValue { .. } |
        Rvalue::XmlText { .. } | Rvalue::CodecOpen { .. } | Rvalue::CodecBatchRows { .. } |
        Rvalue::CodecBatchColumns { .. } | Rvalue::CodecBatchName { .. } | Rvalue::CodecBatchKind { .. } |
        Rvalue::CodecBatchFind { .. } | Rvalue::CodecBatchColumn { .. } | Rvalue::CodecColumnLen { .. } |
        Rvalue::CodecColumnAt { .. } | Rvalue::CodecEncoderNew { .. } | Rvalue::CodecEncoderPut { .. } |
        Rvalue::CodecEncoderFinish { .. } | Rvalue::CryptoDigestNew { .. } | Rvalue::CryptoDigestUpdate { .. } |
        Rvalue::CryptoDigestFinish { .. } | Rvalue::FrameInnerJoin { .. } | Rvalue::IoCopy { .. } |
        Rvalue::FileCreateRw { .. } | Rvalue::FileOpenRw { .. } | Rvalue::FilePread { .. } |
        Rvalue::FilePwrite { .. } | Rvalue::FileLen { .. } | Rvalue::BufferNew { .. } |
        Rvalue::BufferBytes { .. } | Rvalue::BufferLen { .. } | Rvalue::BufferCapacity { .. } |
        Rvalue::BytesRead { .. } | Rvalue::BytesSet { .. } | Rvalue::BytesFill { .. } |
        Rvalue::BytesCopyFrom { .. } | Rvalue::BufferPut { .. } | Rvalue::BufferAppend { .. } |
        Rvalue::BufferAppendFilled { .. } | Rvalue::ArrayBuilderNew { .. } | Rvalue::ArrayBuilderPush { .. } |
        Rvalue::ArrayBuilderPushStr { .. } | Rvalue::ArrayBuilderAppend { .. } | Rvalue::ArrayBuilderBuild { .. } |
        Rvalue::FsWriteFile { .. } | Rvalue::FsWriteFileBuilder { .. } | Rvalue::FsExists { .. } |
        Rvalue::FsRemove { .. } | Rvalue::FsCreateDir { .. } | Rvalue::ProcessLive { .. } |
        Rvalue::FsTree { .. } | Rvalue::FsIsDir { .. } | Rvalue::FsRemoveEmptyDir { .. } |
        Rvalue::RenameNoReplace { .. } | Rvalue::FsReadDir { .. } | Rvalue::DnsResolve { .. } |
        Rvalue::TcpConnect { .. } | Rvalue::ConnReader { .. } | Rvalue::ConnWriter { .. } |
        Rvalue::TcpReadTimeout { .. } | Rvalue::TcpWriteTimeout { .. } | Rvalue::TcpListen { .. } |
        Rvalue::TcpAccept { .. } | Rvalue::UdpBind { .. } | Rvalue::UdpSendTo { .. } |
        Rvalue::UdpRecvFrom { .. } | Rvalue::ProcessSpawn { .. } | Rvalue::ChildWait { .. } |
        Rvalue::ChildKill { .. } | Rvalue::ProcessExec { .. } | Rvalue::FsReadFileView { .. } |
        Rvalue::FsReadBytesView { .. } | Rvalue::PathJoin { .. } | Rvalue::PathComponent { .. } |
        Rvalue::PathNormalize { .. } | Rvalue::EnvGet { .. } | Rvalue::EnvSet { .. } |
        Rvalue::TimeNow { .. } | Rvalue::OsHost { .. } | Rvalue::OsIdentity { .. } |
        Rvalue::ProcessCpuCount { .. } | Rvalue::TimeInstant { .. } | Rvalue::TimeSleep { .. } |
        Rvalue::TimeFormat { .. } | Rvalue::TimeParse { .. } | Rvalue::EncodingEncode { .. } |
        Rvalue::RegexCompile { .. } | Rvalue::RegexIsMatch { .. } | Rvalue::RegexFind { .. } |
        Rvalue::RegexFindAll { .. } | Rvalue::RegexSplit { .. } | Rvalue::RegexReplace { .. } |
        Rvalue::RegexCaptures { .. } | Rvalue::RegexGroupCount { .. } | Rvalue::RegexGroupIndex { .. } |
        Rvalue::CapturesGroup { .. } | Rvalue::EncodingDecode { .. } | Rvalue::CompressCompress { .. } |
        Rvalue::CompressDecompress { .. } | Rvalue::Utf8Valid { .. } | Rvalue::CryptoCtEqual { .. } |
        Rvalue::CryptoRandom { .. } | Rvalue::CryptoHash { .. } | Rvalue::CryptoHmac { .. } |
        Rvalue::CryptoHkdf { .. } | Rvalue::CryptoAead { .. } | Rvalue::CryptoArgon2 { .. } |
        Rvalue::CryptoPrivateKeyFromPem { .. } | Rvalue::CryptoPublicKeyFromPem { .. } | Rvalue::CryptoPublicKeyFromJwk { .. } |
        Rvalue::CryptoSign { .. } | Rvalue::CryptoVerify { .. } | Rvalue::RandSeed { .. } |
        Rvalue::RandNext { .. } | Rvalue::RandRange { .. } | Rvalue::RandShuffle { .. } |
        Rvalue::RandSample { .. } | Rvalue::CliCommand { .. } | Rvalue::CliFlag { .. } |
        Rvalue::CliParse { .. } | Rvalue::CliGetBool { .. } | Rvalue::CliGetI64 { .. } |
        Rvalue::CliGetStr { .. } | Rvalue::CliUsage { .. } | Rvalue::HttpRequest { .. } |
        Rvalue::HttpHeader { .. } | Rvalue::HttpBody { .. } | Rvalue::HttpRequestTimeout { .. } |
        Rvalue::HttpRequestMaxResponseBodyBytes { .. } | Rvalue::HttpClientTimeout { .. } | Rvalue::HttpClientMaxResponseBodyBytes { .. } |
        Rvalue::Command { .. } | Rvalue::CommandCwd { .. } | Rvalue::CommandTimeout { .. } |
        Rvalue::CommandMaxCapture { .. } | Rvalue::CommandEnv { .. } | Rvalue::CommandEnvClear { .. } |
        Rvalue::CommandRun { .. } | Rvalue::CommandRunBytes { .. } | Rvalue::RunOutputView { .. } |
        Rvalue::RunBytesView { .. } | Rvalue::HttpParse { .. } | Rvalue::HttpRespStatus { .. } |
        Rvalue::HttpRespHeader { .. } | Rvalue::HttpRespBody { .. } | Rvalue::HttpClient { .. } |
        Rvalue::HttpClientGet { .. } | Rvalue::HttpClientPost { .. } | Rvalue::HttpClientRequest { .. } |
        Rvalue::HttpClientRequestStream { .. } | Rvalue::HttpReadStreamStatus { .. } | Rvalue::HttpReadStreamHeader { .. } |
        Rvalue::HttpReadStreamRead { .. } | Rvalue::HttpReadStreamSse { .. } | Rvalue::HttpSseStreamLastEventId { .. } |
        Rvalue::HttpSseStreamRetryMs { .. } | Rvalue::HttpSseStreamNext { .. } | Rvalue::HttpGetMany { .. } |
        Rvalue::HttpServe { .. } | Rvalue::HttpAccept { .. } | Rvalue::HttpCtxMethod { .. } |
        Rvalue::HttpCtxPath { .. } | Rvalue::HttpCtxHeader { .. } | Rvalue::HttpHeadersCount { .. } |
        Rvalue::HttpHeadersTokensValid { .. } | Rvalue::HttpHeadersContainsToken { .. } | Rvalue::HttpCtxUpgradeReady { .. } |
        Rvalue::HttpCtxBody { .. } | Rvalue::HttpResponseBuilder { .. } | Rvalue::HttpRbHeader { .. } |
        Rvalue::HttpRbBody { .. } | Rvalue::HttpRespond { .. } | Rvalue::HttpRespondStream { .. } |
        Rvalue::HttpRespondUpgrade { .. } | Rvalue::HttpUpgradeReadExact { .. } | Rvalue::HttpUpgradeWrite { .. } |
        Rvalue::HttpUpgradeDeadline { .. } | Rvalue::HttpUpgradeShutdown { .. } | Rvalue::HttpStreamSend { .. } |
        Rvalue::HttpStreamFinish { .. } | Rvalue::HttpStreamReject { .. }
    };
}

/// Compile-time tripwire over the complete [`Stmt`] inventory, in the manner of
/// `align_sema::variant_sweep_tripwire`. Adding a statement variant fails this match as well as
/// [`statement_facts`], so the build hands you the sweep: decide whether the new statement can
/// write a view root, and whether its operands must be renumbered in a cloned body. Falling into a
/// wildcard would read as "does not mutate", which is the one answer that can be unsound here.
#[allow(dead_code)]
fn variant_sweep_tripwire(stmt: &Stmt) {
    match stmt {
        Stmt::Let { .. }
        | Stmt::Store { .. }
        | Stmt::StoreField { .. }
        | Stmt::StoreIndex { .. }
        | Stmt::StoreConstArray { .. }
        | Stmt::PtrStore { .. }
        | Stmt::PtrStoreNoalias { .. }
        | Stmt::VecStore { .. }
        | Stmt::StoreElemField { .. }
        | Stmt::StoreElemFieldPtr { .. }
        | Stmt::StoreColumn { .. }
        | Stmt::ArenaEnd { .. }
        | Stmt::RawFree { .. }
        | Stmt::ColumnBatchFinish { .. }
        | Stmt::ColumnBatchDrop { .. }
        | Stmt::RawStore { .. }
        | Stmt::TgWait { .. }
        | Stmt::TgEnd { .. }
        | Stmt::DropFlagInit { .. }
        | Stmt::DropFlagMoveOut { .. }
        | Stmt::NullTupleField { .. }
        | Stmt::NullStructField { .. }
        | Stmt::NullElemField { .. }
        | Stmt::Drop { .. }
        | Stmt::DropField { .. }
        | Stmt::DropElem { .. }
        | Stmt::DropElemField { .. }
        | Stmt::BorrowedElementReservation { .. }
        | Stmt::DropValue { .. }
        | Stmt::ArrayTruncate { .. } => {}
    }
}

/// What one [`Rvalue`] does, for both the kill set and the use set.
enum RvalueFacts<'a> {
    /// Writes no memory a view root can reach; the payload is its complete operand-use list.
    Pure(Vec<&'a Operand>),
    /// An opaque call. Kills every root that is not a read-only `borrow` parameter of the enclosing
    /// function, and every root reaching one of its arguments; the payload is its complete use list.
    Call(Vec<&'a Operand>),
    /// Not modelled. May write any memory and may use any value.
    Unknown,
}

/// The complete [`Rvalue`] classification. **Exhaustive, no wildcard arm.** Every whitelisted arm
/// lists *all* of its `Operand` fields explicitly rather than using `..`: an omitted operand would
/// under-report uses and make the escape check unsound.
fn rvalue_facts(rv: &Rvalue) -> RvalueFacts<'_> {
    use RvalueFacts::{Call, Pure, Unknown};
    match rv {
        Rvalue::Use(a) => Pure(vec![a]),
        Rvalue::Load(_) => Pure(Vec::new()),
        Rvalue::Un(_, a) => Pure(vec![a]),
        Rvalue::Cast { operand, from: _, to: _ } => Pure(vec![operand]),
        Rvalue::Bin(_, a, b) => Pure(vec![a, b]),
        Rvalue::FloatBin { op: _, a, b, mode: _ } => Pure(vec![a, b]),
        Rvalue::FloatFma { ty: _, a, b, c, mode: _ } => Pure(vec![a, b, c]),
        Rvalue::IntArith { op: _, mode: _, int_ty: _, a, b } => Pure(vec![a, b]),
        Rvalue::MathOp { fn_: _, ty: _, operands } => Pure(operands.iter().collect()),
        Rvalue::Select { cond, a, b } => Pure(vec![cond, a, b]),
        Rvalue::Field(_, _) => Pure(Vec::new()),
        Rvalue::SliceLen(a) => Pure(vec![a]),
        Rvalue::SlicePtr(a) => Pure(vec![a]),
        Rvalue::SliceIndex(a, b) => Pure(vec![a, b]),
        Rvalue::SliceIndexNoalias { slice, index, scope: _ } => Pure(vec![slice, index]),
        Rvalue::SubSlice { base, start, len, elem: _ } => Pure(vec![base, start, len]),
        Rvalue::Index(_, index) => Pure(vec![index]),
        Rvalue::IndexField(_, index, _) => Pure(vec![index]),
        Rvalue::IndexFieldPtr { base, index, path: _, struct_id: _ } => Pure(vec![base, index]),
        Rvalue::MakeSlice(_, _) => Pure(Vec::new()),
        Rvalue::MakeFieldSlice(_, _, _) => Pure(Vec::new()),
        Rvalue::MakeTuple { tuple_id: _, elems } => Pure(elems.iter().collect()),
        Rvalue::TupleIndex { tuple, index: _ } => Pure(vec![tuple]),
        Rvalue::OptionSome(a) => Pure(vec![a]),
        Rvalue::OptionNone => Pure(Vec::new()),
        Rvalue::OptionIsSome(a) => Pure(vec![a]),
        Rvalue::OptionUnwrap(a) => Pure(vec![a]),
        Rvalue::ResultOk(a) => Pure(vec![a]),
        Rvalue::ResultErr(a) => Pure(vec![a]),
        Rvalue::ResultIsOk(a) => Pure(vec![a]),
        Rvalue::ResultUnwrapOk(a) => Pure(vec![a]),
        Rvalue::ResultUnwrapErr(a) => Pure(vec![a]),
        Rvalue::MakeEnum { enum_id: _, variant: _, payload } => Pure(payload.iter().collect()),
        Rvalue::EnumTagEq { enum_id: _, scrutinee, variant: _ } => Pure(vec![scrutinee]),
        Rvalue::EnumPayload { enum_id: _, variant: _, slot: _, operand } => Pure(vec![operand]),
        Rvalue::StrLit(_) => Pure(Vec::new()),
        Rvalue::ConstArray { elems: _, elem: _ } => Pure(Vec::new()),
        Rvalue::MakeVec { elems, elem: _, n: _ } => Pure(elems.iter().collect()),
        Rvalue::VecExtract { vec, lane: _, elem: _ } => Pure(vec![vec]),
        Rvalue::VecInsert { vec, value, lane: _ } => Pure(vec![vec, value]),
        Rvalue::VecLoad { slice, index, elem: _, n: _, align: _ } => Pure(vec![slice, index]),
        Rvalue::VecSum { vec, elem: _, n: _, mode: _ } => Pure(vec![vec]),
        Rvalue::VecDot { a, b, elem: _, n: _, mode: _ } => Pure(vec![a, b]),
        Rvalue::VecMinMax { vec, elem: _, n: _, max: _ } => Pure(vec![vec]),
        Rvalue::VecSumWhere { vec, mask, elem: _, n: _, mode: _ } => Pure(vec![vec, mask]),
        Rvalue::MaskAny { mask, n: _ } => Pure(vec![mask]),
        Rvalue::FnAddr { target: _, signature: _ } => Pure(Vec::new()),
        Rvalue::RawNull => Pure(Vec::new()),
        Rvalue::RawIsNull(a) => Pure(vec![a]),
        Rvalue::RawOffset { ptr, offset } => Pure(vec![ptr, offset]),
        Rvalue::RawLoad { ptr, offset, scalar: _ } => Pure(vec![ptr, offset]),
        Rvalue::RawPointerLoad { ptr, offset } => Pure(vec![ptr, offset]),
        Rvalue::BoxGet(a) => Pure(vec![a]),
        Rvalue::BytesView { bytes, elem: _ } => Pure(vec![bytes]),
        Rvalue::SliceAsBytes { slice, elem: _ } => Pure(vec![slice]),
        Rvalue::SoaColumn { base: _, struct_id: _, field: _ } => Pure(Vec::new()),
        Rvalue::SoaGather { base, index, struct_id: _ } => Pure(vec![base, index]),
        Rvalue::IndexColumn { base, index, field: _, struct_id: _ } => Pure(vec![base, index]),
        Rvalue::Call(_, args) => Call(args.iter().collect()),
        Rvalue::CallIndirect { callee, args, param_tys: _, ret_ty: _, signature: _ } => {
            Call(std::iter::once(callee).chain(args.iter()).collect())
        }
        unmodelled_rvalues!() => Unknown,
    }
}

/// Drop-state call edges, backed by this module's exhaustive Rvalue classifier. The final arm is
/// not a permissive default: it invokes [`rvalue_facts`], whose wildcard-free inventory makes a
/// newly added Rvalue a compile error until its ownership effect is classified here or there.
pub(crate) enum DropStateCall<'a> {
    Direct(&'a crate::ProgramCall, &'a [Operand]),
    Unknown(&'a [Operand]),
    None,
}

pub(crate) fn drop_state_call(rv: &Rvalue) -> DropStateCall<'_> {
    match rv {
        Rvalue::Call(DirectCall::Program(target), args) => DropStateCall::Direct(target, args),
        Rvalue::Call(DirectCall::Runtime(_), args)
        | Rvalue::CallIndirect { args, .. }
        | Rvalue::RawCall { args, .. } => DropStateCall::Unknown(args),
        Rvalue::CallWithCleanup(call) => DropStateCall::Direct(&call.target, &call.args),
        Rvalue::CallIndirectWithCleanup(call) => DropStateCall::Unknown(&call.args),
        other => {
            let _ = rvalue_facts(other);
            DropStateCall::None
        }
    }
}

/// The mutable twin of [`rvalue_facts`]'s use list, used only by the cloning step to renumber a
/// fast copy's values. **Exhaustive, no wildcard arm**, and it covers exactly the variants
/// [`rvalue_facts`] whitelists — the shared `unmodelled_rvalues!` pattern is what guarantees that.
fn rvalue_operands_mut(rv: &mut Rvalue) -> Option<Vec<&mut Operand>> {
    Some(match rv {
        Rvalue::Use(a) => vec![a],
        Rvalue::Load(_) => Vec::new(),
        Rvalue::Un(_, a) => vec![a],
        Rvalue::Cast { operand, from: _, to: _ } => vec![operand],
        Rvalue::Bin(_, a, b) => vec![a, b],
        Rvalue::FloatBin { op: _, a, b, mode: _ } => vec![a, b],
        Rvalue::FloatFma { ty: _, a, b, c, mode: _ } => vec![a, b, c],
        Rvalue::IntArith { op: _, mode: _, int_ty: _, a, b } => vec![a, b],
        Rvalue::MathOp { fn_: _, ty: _, operands } => operands.iter_mut().collect(),
        Rvalue::Select { cond, a, b } => vec![cond, a, b],
        Rvalue::Field(_, _) => Vec::new(),
        Rvalue::SliceLen(a) => vec![a],
        Rvalue::SlicePtr(a) => vec![a],
        Rvalue::SliceIndex(a, b) => vec![a, b],
        Rvalue::SliceIndexNoalias { slice, index, scope: _ } => vec![slice, index],
        Rvalue::SubSlice { base, start, len, elem: _ } => vec![base, start, len],
        Rvalue::Index(_, index) => vec![index],
        Rvalue::IndexField(_, index, _) => vec![index],
        Rvalue::IndexFieldPtr { base, index, path: _, struct_id: _ } => vec![base, index],
        Rvalue::MakeSlice(_, _) => Vec::new(),
        Rvalue::MakeFieldSlice(_, _, _) => Vec::new(),
        Rvalue::MakeTuple { tuple_id: _, elems } => elems.iter_mut().collect(),
        Rvalue::TupleIndex { tuple, index: _ } => vec![tuple],
        Rvalue::OptionSome(a) => vec![a],
        Rvalue::OptionNone => Vec::new(),
        Rvalue::OptionIsSome(a) => vec![a],
        Rvalue::OptionUnwrap(a) => vec![a],
        Rvalue::ResultOk(a) => vec![a],
        Rvalue::ResultErr(a) => vec![a],
        Rvalue::ResultIsOk(a) => vec![a],
        Rvalue::ResultUnwrapOk(a) => vec![a],
        Rvalue::ResultUnwrapErr(a) => vec![a],
        Rvalue::MakeEnum { enum_id: _, variant: _, payload } => payload.iter_mut().collect(),
        Rvalue::EnumTagEq { enum_id: _, scrutinee, variant: _ } => vec![scrutinee],
        Rvalue::EnumPayload { enum_id: _, variant: _, slot: _, operand } => vec![operand],
        Rvalue::StrLit(_) => Vec::new(),
        Rvalue::ConstArray { elems: _, elem: _ } => Vec::new(),
        Rvalue::MakeVec { elems, elem: _, n: _ } => elems.iter_mut().collect(),
        Rvalue::VecExtract { vec, lane: _, elem: _ } => vec![vec],
        Rvalue::VecInsert { vec, value, lane: _ } => vec![vec, value],
        Rvalue::VecLoad { slice, index, elem: _, n: _, align: _ } => vec![slice, index],
        Rvalue::VecSum { vec, elem: _, n: _, mode: _ } => vec![vec],
        Rvalue::VecDot { a, b, elem: _, n: _, mode: _ } => vec![a, b],
        Rvalue::VecMinMax { vec, elem: _, n: _, max: _ } => vec![vec],
        Rvalue::VecSumWhere { vec, mask, elem: _, n: _, mode: _ } => vec![vec, mask],
        Rvalue::MaskAny { mask, n: _ } => vec![mask],
        Rvalue::FnAddr { target: _, signature: _ } => Vec::new(),
        Rvalue::RawNull => Vec::new(),
        Rvalue::RawIsNull(a) => vec![a],
        Rvalue::RawOffset { ptr, offset } => vec![ptr, offset],
        Rvalue::RawLoad { ptr, offset, scalar: _ } => vec![ptr, offset],
        Rvalue::RawPointerLoad { ptr, offset } => vec![ptr, offset],
        Rvalue::BoxGet(a) => vec![a],
        Rvalue::BytesView { bytes, elem: _ } => vec![bytes],
        Rvalue::SliceAsBytes { slice, elem: _ } => vec![slice],
        Rvalue::SoaColumn { base: _, struct_id: _, field: _ } => Vec::new(),
        Rvalue::SoaGather { base, index, struct_id: _ } => vec![base, index],
        Rvalue::IndexColumn { base, index, field: _, struct_id: _ } => vec![base, index],
        Rvalue::Call(_, args) => args.iter_mut().collect(),
        Rvalue::CallIndirect { callee, args, param_tys: _, ret_ty: _, signature: _ } => {
            std::iter::once(callee).chain(args.iter_mut()).collect()
        }
        unmodelled_rvalues!() => return None,
    })
}

/// What one [`Stmt`] writes, and the operands it uses. **Exhaustive, no wildcard arm** — see
/// [`variant_sweep_tripwire`].
enum StmtFacts<'a> {
    /// Writes nothing reachable from a view root; the payload is its use list.
    Pure(Vec<&'a Operand>),
    /// Writes the named slot's storage; the payload is its use list.
    WritesSlot(Slot, &'static str, Vec<&'a Operand>),
    /// Writes through an operand (a pointer, handle or borrowed base) whose provenance this pass
    /// does not track, so the write kills every root; the payload is the complete use list.
    WritesThrough(&'static str, Vec<&'a Operand>),
    /// Writes **element storage** of a view with a header-free scalar element type. Such a store
    /// changes buffer bytes and no `{ptr,len}` header anywhere, so it kills no root: a guard's
    /// length operand is a property of the header, not of the bytes. The element type is the gate,
    /// exactly as it is for PR 1's `align.elem` tag (I4) — a store of anything that could *hold* a
    /// header is not this case and falls through to the fail-closed default.
    WritesElements(Vec<&'a Operand>),
    /// An opaque call.
    Call(Vec<&'a Operand>),
    /// Writes memory of unknown provenance, or is not modelled: kills every root.
    KillsEverything,
}

/// Whether a stored value's type is a scalar that cannot itself hold a view header, which is what
/// makes an element store harmless to every guard's length operand. Anything else — including an
/// unknown operand type — fails closed.
fn header_free_element(ty: Option<Ty>) -> bool {
    matches!(
        ty,
        Some(
            Ty::Int(_)
                | Ty::Float(_)
                | Ty::Bool
                | Ty::Char
                | Ty::Unit
                | Ty::Vec(_, _)
                | Ty::Mask(_, _)
        )
    )
}

/// The type of an operand, without the panic `Function::operand_ty` can raise on a malformed body.
fn operand_ty(function: &Function, op: &Operand) -> Option<Ty> {
    match op {
        Operand::Const(Const::Int(_, ty)) | Operand::Const(Const::Float(_, ty)) => Some(*ty),
        Operand::Const(Const::Char(_)) => Some(Ty::Char),
        Operand::Const(Const::Bool(_)) => Some(Ty::Bool),
        Operand::Const(Const::Unit) => Some(Ty::Unit),
        Operand::Value(value) => function.value_tys.get(*value as usize).copied(),
        Operand::Arg(index) => function
            .params
            .get(*index as usize)
            .and_then(|slot| function.slots.get(*slot as usize))
            .copied(),
        Operand::BorrowedPlace(_)
        | Operand::BorrowedElementPlace(_)
        | Operand::BorrowedFixedElementPlace(_)
        | Operand::BorrowedCleanupArg(_) => None,
    }
}

fn statement_facts<'a>(function: &Function, stmt: &'a Stmt) -> StmtFacts<'a> {
    use StmtFacts::{Call, KillsEverything, Pure, WritesElements, WritesSlot, WritesThrough};
    // One shared gate for every element-store statement: a header-free scalar element write
    // changes buffer bytes only.
    let element_store = |value: &'a Operand, uses: Vec<&'a Operand>| {
        if header_free_element(operand_ty(function, value)) {
            WritesElements(uses)
        } else {
            KillsEverything
        }
    };
    match stmt {
        Stmt::Let(_, rv) => match rvalue_facts(rv) {
            RvalueFacts::Pure(uses) => Pure(uses),
            RvalueFacts::Call(uses) => Call(uses),
            RvalueFacts::Unknown => KillsEverything,
        },
        Stmt::Store(slot, value) => WritesSlot(*slot, "Store", vec![value]),
        Stmt::StoreField(slot, _, value) => WritesSlot(*slot, "StoreField", vec![value]),
        Stmt::StoreIndex(_, index, value) => element_store(value, vec![index, value]),
        Stmt::StoreConstArray { slot, elems: _, elem: _ } => {
            WritesSlot(*slot, "StoreConstArray", Vec::new())
        }
        Stmt::PtrStore(ptr, index, value) => element_store(value, vec![ptr, index, value]),
        Stmt::PtrStoreNoalias { ptr, index, value, scope: _ } => {
            element_store(value, vec![ptr, index, value])
        }
        Stmt::VecStore { slice, index, value, elem: _, n: _ } => {
            element_store(value, vec![slice, index, value])
        }
        Stmt::StoreElemField(_, index, _, value) => element_store(value, vec![index, value]),
        Stmt::StoreElemFieldPtr { base, index, path: _, struct_id: _, value } => {
            element_store(value, vec![base, index, value])
        }
        Stmt::StoreColumn { base, len, index, field: _, struct_id: _, value } => {
            element_store(value, vec![base, len, index, value])
        }
        // A region release invalidates every allocation made inside it, so it is not modelled per
        // root: it kills everything.
        Stmt::ArenaEnd(handle) => {
            let _ = handle;
            KillsEverything
        }
        Stmt::RawFree(ptr) => {
            let _ = ptr;
            KillsEverything
        }
        Stmt::ColumnBatchFinish { payload, struct_id: _ } => {
            WritesThrough("ColumnBatchFinish", vec![payload])
        }
        Stmt::ColumnBatchDrop { payload, struct_id: _ } => {
            WritesThrough("ColumnBatchDrop", vec![payload])
        }
        // A raw pointer has no Align provenance, so nothing can be proved disjoint from it.
        Stmt::RawStore { ptr, offset, value } => {
            let _ = (ptr, offset, value);
            KillsEverything
        }
        // A task group's members may have written anything the group could reach.
        Stmt::TgWait(handle) => {
            let _ = handle;
            KillsEverything
        }
        Stmt::TgEnd(handle) => {
            let _ = handle;
            KillsEverything
        }
        Stmt::DropFlagInit(slot) => WritesSlot(*slot, "DropFlagInit", Vec::new()),
        Stmt::DropFlagMoveOut { slot, .. } => WritesSlot(*slot, "DropFlagMoveOut", Vec::new()),
        Stmt::NullTupleField(slot, _) => WritesSlot(*slot, "NullTupleField", Vec::new()),
        Stmt::NullStructField(slot, _) => WritesSlot(*slot, "NullStructField", Vec::new()),
        Stmt::NullElemField(slot, index, _) => {
            WritesSlot(*slot, "NullElemField", vec![index])
        }
        Stmt::Drop(slot) => WritesSlot(*slot, "Drop", Vec::new()),
        Stmt::DropField(slot, _) => WritesSlot(*slot, "DropField", Vec::new()),
        Stmt::DropElem(slot, index, _) => WritesSlot(*slot, "DropElem", vec![index]),
        Stmt::DropElemField(slot, index, _) => {
            WritesSlot(*slot, "DropElemField", vec![index])
        }
        // Not a write, but a function-unique token: admission refuses any body holding one.
        Stmt::BorrowedElementReservation { token: _, root: _ } => Pure(Vec::new()),
        Stmt::DropValue(value) => WritesThrough("DropValue", vec![value]),
        // The `drain` witness: this is exactly the statement that changes a published length.
        Stmt::ArrayTruncate { root, path: _, new_len } => {
            WritesSlot(*root, "ArrayTruncate", vec![new_len])
        }
    }
}

/// The mutable operand list of one statement, for the cloning step. `None` is the same fail-closed
/// answer as [`StmtFacts::KillsEverything`]; admission has already refused such a body.
fn stmt_operands_mut(stmt: &mut Stmt) -> Option<Vec<&mut Operand>> {
    Some(match stmt {
        Stmt::Let(_, rv) => return rvalue_operands_mut(rv),
        Stmt::Store(_, value) => vec![value],
        Stmt::StoreField(_, _, value) => vec![value],
        Stmt::StoreIndex(_, index, value) => vec![index, value],
        Stmt::StoreConstArray { slot: _, elems: _, elem: _ } => Vec::new(),
        Stmt::PtrStore(ptr, index, value) => vec![ptr, index, value],
        Stmt::PtrStoreNoalias { ptr, index, value, scope: _ } => vec![ptr, index, value],
        Stmt::VecStore { slice, index, value, elem: _, n: _ } => vec![slice, index, value],
        Stmt::StoreElemField(_, index, _, value) => vec![index, value],
        Stmt::StoreElemFieldPtr { base, index, path: _, struct_id: _, value } => {
            vec![base, index, value]
        }
        Stmt::StoreColumn { base, len, index, field: _, struct_id: _, value } => {
            vec![base, len, index, value]
        }
        Stmt::ArenaEnd(handle) => vec![handle],
        Stmt::RawFree(ptr) => vec![ptr],
        Stmt::ColumnBatchFinish { payload, struct_id: _ } => vec![payload],
        Stmt::ColumnBatchDrop { payload, struct_id: _ } => vec![payload],
        Stmt::RawStore { ptr, offset, value } => vec![ptr, offset, value],
        Stmt::TgWait(handle) => vec![handle],
        Stmt::TgEnd(handle) => vec![handle],
        Stmt::DropFlagInit(_) | Stmt::DropFlagMoveOut { .. } => Vec::new(),
        Stmt::NullTupleField(_, _) => Vec::new(),
        Stmt::NullStructField(_, _) => Vec::new(),
        Stmt::NullElemField(_, index, _) => vec![index],
        Stmt::Drop(_) => Vec::new(),
        Stmt::DropField(_, _) => Vec::new(),
        Stmt::DropElem(_, index, _) => vec![index],
        Stmt::DropElemField(_, index, _) => vec![index],
        Stmt::BorrowedElementReservation { token: _, root: _ } => Vec::new(),
        Stmt::DropValue(value) => vec![value],
        Stmt::ArrayTruncate { root: _, path: _, new_len } => vec![new_len],
    })
}

fn term_operands(term: &Term) -> Vec<&Operand> {
    match term {
        Term::Goto(_) | Term::Unreachable => Vec::new(),
        Term::Branch(cond, _, _) => vec![cond],
        Term::StrMatch { scrutinee, .. } => vec![scrutinee],
        Term::Return(value) => value.iter().collect(),
        Term::ReturnWithCleanup(pair) => vec![&pair.0, &pair.1],
    }
}

fn term_operands_mut(term: &mut Term) -> Vec<&mut Operand> {
    match term {
        Term::Goto(_) | Term::Unreachable => Vec::new(),
        Term::Branch(cond, _, _) => vec![cond],
        Term::StrMatch { scrutinee, .. } => vec![scrutinee],
        Term::Return(value) => value.iter_mut().collect(),
        Term::ReturnWithCleanup(pair) => vec![&mut pair.0, &mut pair.1],
    }
}

/// Structural operand identity. [`Operand`] has no `PartialEq`, and the guard matcher needs to
/// know that the operand a cast was built from is the *same* operand the trap reports. Anything
/// this does not model compares unequal, which refuses the guard rather than fusing two operands
/// that only look alike.
fn same_operand(a: &Operand, b: &Operand) -> bool {
    match (a, b) {
        (Operand::Value(left), Operand::Value(right)) => left == right,
        (Operand::Arg(left), Operand::Arg(right)) => left == right,
        (Operand::Const(Const::Int(left, left_ty)), Operand::Const(Const::Int(right, right_ty))) => {
            left == right && left_ty == right_ty
        }
        _ => false,
    }
}

fn successors(term: &Term) -> Vec<BlockId> {
    match term {
        Term::Goto(target) => vec![*target],
        Term::Branch(_, yes, no) => vec![*yes, *no],
        Term::StrMatch { cases, otherwise, .. } => cases
            .iter()
            .map(|(_, target)| *target)
            .chain(std::iter::once(*otherwise))
            .collect(),
        Term::Return(_) | Term::ReturnWithCleanup(_) | Term::Unreachable => Vec::new(),
    }
}

fn term_targets_mut(term: &mut Term) -> Vec<&mut BlockId> {
    match term {
        Term::Goto(target) => vec![target],
        Term::Branch(_, yes, no) => vec![yes, no],
        Term::StrMatch { cases, otherwise, .. } => cases
            .iter_mut()
            .map(|(_, target)| target)
            .chain(std::iter::once(otherwise))
            .collect(),
        Term::Return(_) | Term::ReturnWithCleanup(_) | Term::Unreachable => Vec::new(),
    }
}

/// The count of emitted bounds/range trap actions. Versioning must leave it **exactly** unchanged:
/// the slow copy keeps every trap block and a fast copy's cloned failure blocks are emptied, which
/// is the mechanical form of §3.3's "call-site count unchanged".
fn trap_call_count(function: &Function) -> usize {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.stmts)
        .filter(|stmt| {
            matches!(
                stmt,
                Stmt::Let(
                    _,
                    Rvalue::Call(
                        DirectCall::Runtime(RuntimeKey::BoundsFail | RuntimeKey::RangeFail),
                        _
                    )
                )
            )
        })
        .count()
}

// ---------------------------------------------------------------------------------------------
// CFG facts.
// ---------------------------------------------------------------------------------------------

struct Cfg {
    blocks: usize,
    preds: Vec<Vec<BlockId>>,
    /// `dom[b]` is the set of blocks that dominate `b`, as a dense bit row.
    dom: Vec<Vec<bool>>,
}

impl Cfg {
    fn build(function: &Function) -> Option<Self> {
        let blocks = function.blocks.len();
        if blocks == 0 || blocks > MAX_ANALYSED_BLOCKS {
            return None;
        }
        for (index, block) in function.blocks.iter().enumerate() {
            if block.id as usize != index {
                return None;
            }
            for target in successors(&block.term) {
                if target as usize >= blocks {
                    return None;
                }
            }
        }
        let entry = function.entry as usize;
        if entry >= blocks {
            return None;
        }
        let mut preds = vec![Vec::new(); blocks];
        for block in &function.blocks {
            for target in successors(&block.term) {
                preds[target as usize].push(block.id);
            }
        }
        // Iterative dominators over a dense bit matrix. Blocks unreachable from the entry keep the
        // full set, which is conservative for every question asked below.
        let mut dom = vec![vec![true; blocks]; blocks];
        dom[entry] = vec![false; blocks];
        dom[entry][entry] = true;
        let mut changed = true;
        while changed {
            changed = false;
            for index in 0..blocks {
                if index == entry {
                    continue;
                }
                let mut next: Option<Vec<bool>> = None;
                for pred in &preds[index] {
                    let row = &dom[*pred as usize];
                    next = Some(match next {
                        None => row.clone(),
                        Some(mut acc) => {
                            for (slot, value) in acc.iter_mut().enumerate() {
                                *value &= row[slot];
                            }
                            acc
                        }
                    });
                }
                let mut next = next.unwrap_or_else(|| vec![false; blocks]);
                next[index] = true;
                if next != dom[index] {
                    dom[index] = next;
                    changed = true;
                }
            }
        }
        Some(Self { blocks, preds, dom })
    }

    fn dominates(&self, before: BlockId, after: BlockId) -> bool {
        self.dom[after as usize][before as usize]
    }

    /// Blocks reachable from `from` without entering any block in `barrier`.
    fn reachable_from(
        &self,
        function: &Function,
        from: BlockId,
        barrier: &BTreeSet<BlockId>,
    ) -> BTreeSet<BlockId> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(block) = stack.pop() {
            if barrier.contains(&block) || !seen.insert(block) {
                continue;
            }
            for target in successors(&function.blocks[block as usize].term) {
                stack.push(target);
            }
        }
        seen
    }
}

/// One natural loop: its header, its body (including the trap blocks it dominates), and its latch.
struct LoopShape {
    header: BlockId,
    body: BTreeSet<BlockId>,
    latch: BlockId,
}

/// Every natural loop of the function, innermost-first (smallest body first, then by header id, so
/// the order is total and deterministic). Returns `None` when the CFG is not analysable.
fn discover_loops(function: &Function, cfg: &Cfg) -> Option<Vec<LoopShape>> {
    let mut back_edges: BTreeMap<BlockId, Vec<BlockId>> = BTreeMap::new();
    for block in &function.blocks {
        for target in successors(&block.term) {
            if cfg.dominates(target, block.id) {
                back_edges.entry(target).or_default().push(block.id);
            }
        }
    }
    let mut loops = Vec::new();
    for (header, latches) in back_edges {
        // One latch only: a multi-latch loop has no single place to read the step from.
        let [latch] = latches.as_slice() else {
            loops.push(LoopShape {
                header,
                body: BTreeSet::new(),
                latch: header,
            });
            continue;
        };
        let mut body = BTreeSet::new();
        body.insert(header);
        let mut stack = vec![*latch];
        while let Some(block) = stack.pop() {
            if !body.insert(block) {
                continue;
            }
            for pred in &cfg.preds[block as usize] {
                stack.push(*pred);
            }
        }
        // Absorb the dead-end blocks the header dominates — the guards' `Unreachable` trap blocks,
        // which never reach the latch and so are not in the natural loop, but are part of the body
        // that must be copied.
        loop {
            let mut grew = false;
            for block in &function.blocks {
                if body.contains(&block.id)
                    || !matches!(block.term, Term::Unreachable)
                    || !cfg.dominates(header, block.id)
                {
                    continue;
                }
                if cfg.preds[block.id as usize]
                    .iter()
                    .all(|pred| body.contains(pred))
                    && !cfg.preds[block.id as usize].is_empty()
                {
                    body.insert(block.id);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        loops.push(LoopShape {
            header,
            body,
            latch: *latch,
        });
    }
    loops.sort_by_key(|shape| (shape.body.len(), shape.header));
    let _ = cfg.blocks;
    Some(loops)
}

// ---------------------------------------------------------------------------------------------
// Guards.
// ---------------------------------------------------------------------------------------------

/// A fused guard recognized inside a loop body.
struct Guard {
    /// The block whose terminator branches on the fused predicate.
    block: BlockId,
    /// Its failure block, which holds exactly one trap action and is `Unreachable`.
    fail: BlockId,
    /// Its success block.
    ok: BlockId,
    /// The index (element guard) or the range start.
    start: Operand,
    /// The range end; `None` for an element guard, whose single index must be `< len`.
    end: Option<Operand>,
    /// The length operand the guard compares against.
    len: Operand,
}

// ---------------------------------------------------------------------------------------------
// The analysis.
// ---------------------------------------------------------------------------------------------

struct Analysis<'a> {
    function: &'a Function,
    cfg: &'a Cfg,
    /// Unique definition of every SSA value: `(block, position, rvalue)`.
    defs: BTreeMap<ValueId, (BlockId, usize, &'a Rvalue)>,
}

impl<'a> Analysis<'a> {
    fn new(function: &'a Function, cfg: &'a Cfg) -> Option<Self> {
        let mut defs = BTreeMap::new();
        for block in &function.blocks {
            for (position, stmt) in block.stmts.iter().enumerate() {
                if let Stmt::Let(value, rv) = stmt
                    && (function.value_tys.get(*value as usize).is_none()
                        || defs.insert(*value, (block.id, position, rv)).is_some())
                {
                    return None;
                }
            }
        }
        Some(Self {
            function,
            cfg,
            defs,
        })
    }

    fn def(&self, op: &Operand) -> Option<(BlockId, usize, &'a Rvalue)> {
        let Operand::Value(value) = op else {
            return None;
        };
        self.defs.get(value).copied()
    }

    fn value_ty(&self, op: &Operand) -> Option<Ty> {
        match op {
            Operand::Const(Const::Int(_, ty)) => Some(*ty),
            Operand::Value(value) => self.function.value_tys.get(*value as usize).copied(),
            _ => None,
        }
    }

    /// The operand a `u64` cast was built from, when `op` is exactly such a cast.
    fn unsigned_cast_source(&self, op: &Operand) -> Option<&'a Operand> {
        match self.def(op)? {
            (
                _,
                _,
                Rvalue::Cast {
                    operand,
                    from,
                    to,
                },
            ) if *from == i64_ty() && *to == u64_ty() => Some(operand),
            _ => None,
        }
    }

    fn binary(&self, op: &Operand, want: BinOp) -> Option<(&'a Operand, &'a Operand)> {
        match self.def(op)? {
            (_, _, Rvalue::Bin(found, a, b)) if *found == want => Some((a, b)),
            _ => None,
        }
    }

    /// Recognize the guard a block branches on, if any. Returns `Err(())` when the block holds a
    /// trap edge that is *not* in the fused form, which is the fail-closed refusal (§3.7).
    fn guard(&self, block: BlockId) -> Result<Option<Guard>, ()> {
        let current = &self.function.blocks[block as usize];
        let Term::Branch(condition, fail, ok) = &current.term else {
            return Ok(None);
        };
        let fail_block = &self.function.blocks[*fail as usize];
        let (key, args) = match fail_block.stmts.as_slice() {
            [Stmt::Let(_, Rvalue::Call(DirectCall::Runtime(key), args))]
                if matches!(key, RuntimeKey::BoundsFail | RuntimeKey::RangeFail) =>
            {
                (*key, args.as_slice())
            }
            _ => return Ok(None),
        };
        if !matches!(fail_block.term, Term::Unreachable) {
            return Err(());
        }
        match (key, args) {
            (RuntimeKey::BoundsFail, [index, len]) => {
                let Some((left, right)) = self.binary(condition, BinOp::Ge) else {
                    return Err(());
                };
                let (Some(left), Some(right)) = (
                    self.unsigned_cast_source(left),
                    self.unsigned_cast_source(right),
                ) else {
                    return Err(());
                };
                if !same_operand(left, index) || !same_operand(right, len) {
                    return Err(());
                }
                Ok(Some(Guard {
                    block,
                    fail: *fail,
                    ok: *ok,
                    start: index.clone(),
                    end: None,
                    len: len.clone(),
                }))
            }
            (RuntimeKey::RangeFail, [start, end, len]) => {
                let Some((inverted, over)) = self.binary(condition, BinOp::Or) else {
                    return Err(());
                };
                let (Some((us, ue)), Some((ue2, ul))) = (
                    self.binary(inverted, BinOp::Gt),
                    self.binary(over, BinOp::Gt),
                ) else {
                    return Err(());
                };
                let (Some(us), Some(ue), Some(ue2), Some(ul)) = (
                    self.unsigned_cast_source(us),
                    self.unsigned_cast_source(ue),
                    self.unsigned_cast_source(ue2),
                    self.unsigned_cast_source(ul),
                ) else {
                    return Err(());
                };
                if !same_operand(us, start)
                    || !same_operand(ue, end)
                    || !same_operand(ue2, end)
                    || !same_operand(ul, len)
                {
                    return Err(());
                }
                Ok(Some(Guard {
                    block,
                    fail: *fail,
                    ok: *ok,
                    start: start.clone(),
                    end: Some(end.clone()),
                    len: len.clone(),
                }))
            }
            _ => Err(()),
        }
    }
}

/// Every SSA value an operand names, including the ones nested inside a borrowed element place.
/// Exhaustive over [`Operand`], because an unexamined nesting would hide a use from the escape
/// check; `hir::BorrowedPathSegment` carries no MIR operand, so a plain borrowed place names none.
fn operand_values(op: &Operand, out: &mut Vec<ValueId>) {
    match op {
        Operand::Const(_) | Operand::Arg(_) | Operand::BorrowedCleanupArg(_) => {}
        Operand::Value(value) => out.push(*value),
        Operand::BorrowedPlace(_) | Operand::BorrowedFixedElementPlace(_) => {}
        Operand::BorrowedElementPlace(place) => {
            operand_values(&place.index, out);
            operand_values(&place.guard.len, out);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The preheader builder.
// ---------------------------------------------------------------------------------------------

/// Statements staged for the new preheader, with the fresh SSA values they define. Values are
/// allocated from a counter rather than by mutating the function, so the whole admission can be
/// computed and then thrown away without touching the IR.
struct Preheader {
    stmts: Vec<Stmt>,
    next_value: ValueId,
    new_tys: Vec<Ty>,
}

impl Preheader {
    /// `None` when the function's value inventory will not fit the ids the plan would mint. Every
    /// id this pass creates is a `u32`, and nothing downstream may see a wrapped one.
    fn new(function: &Function) -> Option<Self> {
        Some(Self {
            stmts: Vec::new(),
            next_value: ValueId::try_from(function.value_tys.len()).ok()?,
            new_tys: Vec::new(),
        })
    }

    fn emit(&mut self, ty: Ty, rv: Rvalue) -> Operand {
        let value = self.next_value;
        self.next_value = self.next_value.saturating_add(1);
        self.new_tys.push(ty);
        self.stmts.push(Stmt::Let(value, rv));
        Operand::Value(value)
    }

    fn bin(&mut self, op: BinOp, ty: Ty, a: Operand, b: Operand) -> Operand {
        self.emit(ty, Rvalue::Bin(op, a, b))
    }

    fn all(&mut self, conjuncts: Vec<Operand>) -> Operand {
        let mut iter = conjuncts.into_iter();
        let mut acc = iter.next().unwrap_or(Operand::Const(Const::Bool(true)));
        for next in iter {
            acc = self.bin(BinOp::And, Ty::Bool, acc, next);
        }
        acc
    }
}

// ---------------------------------------------------------------------------------------------
// Admission.
// ---------------------------------------------------------------------------------------------

/// The rewrite one admitted loop will receive.
struct Plan {
    body: Vec<BlockId>,
    preheader: Preheader,
    admit: Operand,
    /// `(guard block, failure block, success block)` for each proved guard.
    proved: Vec<(BlockId, BlockId, BlockId)>,
    budget_used: usize,
    counted: Option<CountedPlan>,
    extents: Vec<DataExtent>,
}

struct CountedPlan {
    entry: Operand,
    trip_count: Operand,
    index: Slot,
    step: i64,
    relation: BinOp,
    body_arm: BlockId,
    exit_arm: BlockId,
    latch: BlockId,
    stepped: Operand,
    fact_bound: Operand,
    bound: Operand,
}

/// A loop's body facts: what it writes, and whether it calls out.
struct BodyFacts {
    /// Slots written in the body, with the statement kind that wrote each one first.
    killed: BTreeMap<Slot, &'static str>,
    opaque_call: bool,
    /// Present when some statement is not modelled at all.
    unmodelled: bool,
    borrowed_element: bool,
}

fn body_facts(function: &Function, body: &BTreeSet<BlockId>) -> BodyFacts {
    let mut facts = BodyFacts {
        killed: BTreeMap::new(),
        opaque_call: false,
        unmodelled: false,
        borrowed_element: false,
    };
    for block in body {
        for stmt in &function.blocks[*block as usize].stmts {
            if matches!(stmt, Stmt::BorrowedElementReservation { .. }) {
                facts.borrowed_element = true;
            }
            match statement_facts(function, stmt) {
                StmtFacts::Pure(_) => {}
                StmtFacts::WritesSlot(slot, kind, _) => {
                    facts.killed.entry(slot).or_insert(kind);
                }
                // Writing through an operand can reach any root that operand derives from. This
                // pass does not track pointer provenance, so it fails closed: the statement kills
                // every root, exactly as an unmodelled one does.
                StmtFacts::WritesThrough(kind, _) => {
                    facts.killed.insert(Slot::MAX, kind);
                }
                // Buffer bytes only: no header, so no guard's length operand.
                StmtFacts::WritesElements(_) => {}
                StmtFacts::Call(_) => facts.opaque_call = true,
                StmtFacts::KillsEverything => facts.unmodelled = true,
            }
        }
    }
    facts
}

/// Every slot whose *address* is handed out somewhere in the function, so a callee could write it.
/// MIR hands out a slot address only through the borrowed-place operand family. `None` means some
/// statement is not modelled, in which case the caller must treat every slot as escaping.
fn slots_with_escaping_address(function: &Function) -> Option<BTreeSet<Slot>> {
    fn collect(op: &Operand, out: &mut BTreeSet<Slot>) {
        match op {
            Operand::Const(_) | Operand::Value(_) | Operand::Arg(_) => {}
            Operand::BorrowedCleanupArg(_) => {}
            Operand::BorrowedPlace(place) => {
                out.insert(place.slot);
                if let Some(cleanup) = place.cleanup {
                    out.insert(cleanup);
                }
            }
            Operand::BorrowedFixedElementPlace(place) => {
                out.insert(place.base);
                if let Some(cleanup) = place.cleanup {
                    out.insert(cleanup);
                }
            }
            Operand::BorrowedElementPlace(place) => {
                out.insert(place.base.slot);
                if let Some(cleanup) = place.base.cleanup {
                    out.insert(cleanup);
                }
                collect(&place.index, out);
                collect(&place.guard.len, out);
            }
        }
    }
    let mut escaping = BTreeSet::new();
    for block in &function.blocks {
        for stmt in &block.stmts {
            let operands = match statement_facts(function, stmt) {
                StmtFacts::Pure(uses)
                | StmtFacts::Call(uses)
                | StmtFacts::WritesElements(uses)
                | StmtFacts::WritesSlot(_, _, uses)
                | StmtFacts::WritesThrough(_, uses) => uses,
                StmtFacts::KillsEverything => return None,
            };
            for operand in operands {
                collect(operand, &mut escaping);
            }
        }
        for operand in term_operands(&block.term) {
            collect(operand, &mut escaping);
        }
    }
    Some(escaping)
}

/// Whether `slot` holds a value that cannot change across the loop: it is written by no statement
/// in the body, and — when the body calls out opaquely — the callee has no way to write it either.
/// Two disjoint proofs qualify (plan 69 §3.2's kill set, and §3.7's narrowing of its call rule):
/// the slot's address never leaves the function, or it is a read-only `borrow` parameter, whose
/// header a callee is forbidden to write.
fn slot_is_invariant(
    function: &Function,
    facts: &BodyFacts,
    escaping: Option<&BTreeSet<Slot>>,
    slot: Slot,
) -> bool {
    if facts.killed.contains_key(&slot) || facts.killed.contains_key(&Slot::MAX) {
        return false;
    }
    if !facts.opaque_call {
        return true;
    }
    if escaping.is_some_and(|escaping| !escaping.contains(&slot)) {
        return true;
    }
    function
        .params
        .iter()
        .zip(&function.param_modes)
        .any(|(param, mode)| *param == slot && *mode == ParamMode::Borrow)
}

/// Whether `slot` is written somewhere that dominates the loop, so rematerializing a load of it in
/// the preheader cannot read uninitialized storage (and so cannot branch on poison).
fn slot_initialized_before(
    function: &Function,
    cfg: &Cfg,
    body: &BTreeSet<BlockId>,
    header: BlockId,
    slot: Slot,
) -> bool {
    if function.params.contains(&slot) {
        // Every parameter slot is stored from its `Arg` in the entry block.
        return true;
    }
    function.blocks.iter().any(|block| {
        !body.contains(&block.id)
            && cfg.dominates(block.id, header)
            && block.stmts.iter().any(|stmt| {
                matches!(statement_facts(function, stmt), StmtFacts::WritesSlot(written, _, _) if written == slot)
            })
    })
}

/// Rebuild one loop-invariant operand in the preheader.
struct Remat<'a> {
    analysis: &'a Analysis<'a>,
    body: &'a BTreeSet<BlockId>,
    facts: &'a BodyFacts,
    escaping: Option<&'a BTreeSet<Slot>>,
    header: BlockId,
    memo: BTreeMap<ValueId, Operand>,
    /// The first slot whose kill blocked a rematerialization, for the reason code.
    blocked_by: Option<&'static str>,
}

impl Remat<'_> {
    fn get(&mut self, op: &Operand, pre: &mut Preheader) -> Option<Operand> {
        match op {
            Operand::Const(_) | Operand::Arg(_) => return Some(op.clone()),
            Operand::Value(value) => {
                if let Some(found) = self.memo.get(value) {
                    return Some(found.clone());
                }
            }
            _ => return None,
        }
        let Operand::Value(value) = op else {
            return None;
        };
        let (block, _, rv) = self.analysis.def(op)?;
        if !self.body.contains(&block) {
            // Defined outside the loop; usable directly when it dominates the preheader, which sits
            // on every edge into the header.
            if self.analysis.cfg.dominates(block, self.header) {
                self.memo.insert(*value, op.clone());
                return Some(op.clone());
            }
            return None;
        }
        let ty = self.analysis.function.value_tys.get(*value as usize).copied()?;
        let rebuilt = match rv {
            Rvalue::Use(inner) => self.get(inner, pre)?,
            Rvalue::Load(slot) => {
                if !slot_is_invariant(self.analysis.function, self.facts, self.escaping, *slot) {
                    self.blocked_by = self.blocked_by.or_else(|| {
                        self.facts
                            .killed
                            .get(slot)
                            .copied()
                            .or(self.facts.killed.get(&Slot::MAX).copied())
                            .or(Some("Call"))
                    });
                    return None;
                }
                if !slot_initialized_before(
                    self.analysis.function,
                    self.analysis.cfg,
                    self.body,
                    self.header,
                    *slot,
                ) {
                    return None;
                }
                pre.emit(ty, Rvalue::Load(*slot))
            }
            Rvalue::SliceLen(inner) => {
                let inner = self.get(inner, pre)?;
                pre.emit(ty, Rvalue::SliceLen(inner))
            }
            Rvalue::Cast { operand, from, to } => {
                let operand = self.get(operand, pre)?;
                pre.emit(
                    ty,
                    Rvalue::Cast {
                        operand,
                        from: *from,
                        to: *to,
                    },
                )
            }
            Rvalue::Bin(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul), a, b) => {
                let a = self.get(a, pre)?;
                let b = self.get(b, pre)?;
                pre.bin(*op, ty, a, b)
            }
            _ => return None,
        };
        self.memo.insert(*value, rebuilt.clone());
        Some(rebuilt)
    }
}

/// `a * i + b` with `a` a positive `i64` constant and `b` already rebuilt in the preheader.
struct Affine {
    a: i128,
    b: Operand,
}

fn affine(
    remat: &mut Remat<'_>,
    pre: &mut Preheader,
    index_slot: Slot,
    op: &Operand,
) -> Option<Affine> {
    let (block, _, rv) = remat.analysis.def(op)?;
    if !remat.body.contains(&block) {
        return None;
    }
    match rv {
        Rvalue::Use(inner) => affine(remat, pre, index_slot, inner),
        Rvalue::Load(slot) if *slot == index_slot => Some(Affine { a: 1, b: int(0) }),
        Rvalue::Bin(BinOp::Add, a, b) => {
            if let Some(base) = affine(remat, pre, index_slot, a)
                && let Some(offset) = remat.get(b, pre)
            {
                let b = pre.bin(BinOp::Add, i64_ty(), base.b, offset);
                return Some(Affine { a: base.a, b });
            }
            let base = affine(remat, pre, index_slot, b)?;
            let offset = remat.get(a, pre)?;
            let b = pre.bin(BinOp::Add, i64_ty(), base.b, offset);
            Some(Affine { a: base.a, b })
        }
        Rvalue::Bin(BinOp::Sub, a, b) => {
            let base = affine(remat, pre, index_slot, a)?;
            let offset = remat.get(b, pre)?;
            let b = pre.bin(BinOp::Sub, i64_ty(), base.b, offset);
            Some(Affine { a: base.a, b })
        }
        // `i * k` only: scaling a non-zero offset would need its own overflow proof, which §3.2.1
        // states for `a*i + b` and not for `(a*i + b)*k`.
        Rvalue::Bin(BinOp::Mul, a, b) => {
            let scale = |operand: &Operand| match operand {
                Operand::Const(Const::Int(value, ty)) if *ty == i64_ty() && *value > 0 => {
                    Some(*value)
                }
                _ => None,
            };
            let (base, factor) = if let Some(factor) = scale(b) {
                (affine(remat, pre, index_slot, a)?, factor)
            } else {
                (affine(remat, pre, index_slot, b)?, scale(a)?)
            };
            if !matches!(base.b, Operand::Const(Const::Int(0, _))) {
                return None;
            }
            Some(Affine {
                a: base.a.checked_mul(factor)?,
                b: int(0),
            })
        }
        _ => None,
    }
}

fn view_extent_source<'a>(
    analysis: &'a Analysis<'a>,
    len: &'a Operand,
) -> Option<(Slot, &'a Operand, align_sema::Scalar)> {
    let (_, _, Rvalue::SliceLen(view)) = analysis.def(len)? else {
        return None;
    };
    let elem = match analysis.value_ty(view)? {
        Ty::Slice(elem) | Ty::DynArray(elem) => elem,
        _ => return None,
    };
    fn root_slot(analysis: &Analysis<'_>, operand: &Operand) -> Option<Slot> {
        let (_, _, value) = analysis.def(operand)?;
        match value {
            Rvalue::Load(slot) => Some(*slot),
            Rvalue::Use(inner) => root_slot(analysis, inner),
            _ => None,
        }
    }
    Some((root_slot(analysis, view)?, view, elem))
}

/// Decide whether one loop may be versioned, and build the rewrite it would receive.
///
/// `ignore` names blocks that are not part of this decision — the fast copy produced for this same
/// loop, when the caller re-derives the facts from the rewritten function.
fn admit(
    function: &Function,
    cfg: &Cfg,
    analysis: &Analysis<'_>,
    shape: &LoopShape,
    ignore: &BTreeSet<BlockId>,
) -> Result<Plan, KeptReason> {
    if shape.body.is_empty() {
        return Err(KeptReason::LoopShape);
    }
    let header = shape.header;
    let body = &shape.body;
    let facts = body_facts(function, body);
    // Every slot whose value this proof depends on must be provably beyond a callee's reach. This
    // is computed once, over the whole function, because a mutation between the index's
    // initialization and the loop header is as fatal as one inside the body.
    let escaping = slots_with_escaping_address(function);
    if facts.borrowed_element {
        return Err(KeptReason::BorrowedElement);
    }
    // A nested loop inside the body: a guarded access in it does not run once per value of this
    // loop's index, so the affine argument would not describe it.
    for block in body {
        for target in successors(&function.blocks[*block as usize].term) {
            if target != header && body.contains(&target) && cfg.dominates(target, *block) {
                return Err(KeptReason::NestedLoop);
            }
        }
    }
    // Every trap edge in the body must be in the fused form; a signed one is a channel this pass
    // does not own (a byte accessor guard `byte_ranges` re-derives literally, §3.7). This is
    // decided before the unmodelled-statement default so the report names the specific reason
    // rather than the fail-closed one.
    let mut guards = Vec::new();
    for block in body {
        match analysis.guard(*block) {
            Ok(Some(guard)) => guards.push(guard),
            Ok(None) => {}
            Err(()) => return Err(KeptReason::GuardNotFused),
        }
    }
    if facts.unmodelled {
        return Err(KeptReason::UnmodelledStatement);
    }
    let budget_used: usize = body
        .iter()
        .map(|block| function.blocks[*block as usize].stmts.len() + 1)
        .sum();
    if budget_used > LOOP_FACTS_VERSION_BUDGET {
        return Err(KeptReason::OverBudget);
    }
    // The header's trip-count exit: `i >= N` or `i > N`, leaving the loop on the true arm.
    let Term::Branch(condition, exit_arm, body_arm) = &function.blocks[header as usize].term else {
        return Err(KeptReason::LoopShape);
    };
    if body.contains(exit_arm) || !body.contains(body_arm) {
        return Err(KeptReason::LoopShape);
    }
    let (relation, index_operand, bound) = if let Some((index, bound)) =
        analysis.binary(condition, BinOp::Ge)
    {
        (BinOp::Ge, index, bound)
    } else if let Some((index, bound)) = analysis.binary(condition, BinOp::Gt) {
        (BinOp::Gt, index, bound)
    } else {
        return Err(KeptReason::LoopShape);
    };
    let latch_bound = bound.clone();
    let guard_exits: BTreeSet<BlockId> = guards.iter().map(|guard| guard.fail).collect();
    let mut has_early_exit = false;
    for block in body {
        let Some(loop_block) = function.blocks.get(*block as usize) else {
            return Err(KeptReason::LoopShape);
        };
        has_early_exit |= matches!(loop_block.term, Term::Return(_) | Term::ReturnWithCleanup(_))
            || successors(&loop_block.term).into_iter().any(|target| {
                !body.contains(&target)
                    && target != *exit_arm
                    && !guard_exits.contains(&target)
            });
    }
    if guards.is_empty() && !has_early_exit {
        return Err(KeptReason::NoProvableGuard);
    }
    let Some((condition_block, _, _)) = analysis.def(condition) else {
        return Err(KeptReason::LoopShape);
    };
    if condition_block != header {
        return Err(KeptReason::LoopShape);
    }
    // Rotation bypasses the source header after the first iteration, so that block may contain
    // only the pure dependency cone that forms the trip test. An observable call, a potentially
    // failing unrelated expression, or even an unrelated load before the `if` belongs to every
    // source iteration and must not be skipped by the rotated back edge.
    let Some(header_block) = function.blocks.get(header as usize) else {
        return Err(KeptReason::LoopShape);
    };
    let mut pending = Vec::new();
    operand_values(condition, &mut pending);
    let mut condition_stmts = BTreeSet::new();
    while let Some(value) = pending.pop() {
        let operand = Operand::Value(value);
        let Some((block, position, rvalue)) = analysis.def(&operand) else {
            return Err(KeptReason::LoopShape);
        };
        if block != header || !condition_stmts.insert(position) {
            continue;
        }
        let RvalueFacts::Pure(operands) = rvalue_facts(rvalue) else {
            return Err(KeptReason::LoopShape);
        };
        for operand in operands {
            operand_values(operand, &mut pending);
        }
    }
    if condition_stmts.len() != header_block.stmts.len() {
        return Err(KeptReason::LoopShape);
    }
    let Some((_, _, Rvalue::Load(index_slot))) = analysis.def(index_operand) else {
        return Err(KeptReason::LoopShape);
    };
    let index_slot = *index_slot;
    if function.slots.get(index_slot as usize) != Some(&i64_ty())
        || analysis.value_ty(bound) != Some(i64_ty())
    {
        return Err(KeptReason::LoopShape);
    }
    let header_index_values: BTreeSet<ValueId> = header_block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Let(value, Rvalue::Load(slot)) if *slot == index_slot => Some(*value),
            _ => None,
        })
        .collect();
    for block in body.iter().filter(|block| **block != header) {
        let Some(loop_block) = function.blocks.get(*block as usize) else {
            return Err(KeptReason::LoopShape);
        };
        let mut uses = Vec::new();
        for statement in &loop_block.stmts {
            for operand in match statement_facts(function, statement) {
                StmtFacts::Pure(uses)
                | StmtFacts::Call(uses)
                | StmtFacts::WritesElements(uses)
                | StmtFacts::WritesSlot(_, _, uses)
                | StmtFacts::WritesThrough(_, uses) => uses,
                StmtFacts::KillsEverything => return Err(KeptReason::UnmodelledStatement),
            } {
                operand_values(operand, &mut uses);
            }
        }
        for operand in term_operands(&loop_block.term) {
            operand_values(operand, &mut uses);
        }
        if uses.iter().any(|value| header_index_values.contains(value)) {
            return Err(KeptReason::LoopShape);
        }
    }
    // A second exit shaped like another trip-count test is ambiguous: neither comparison is the
    // unique recurrence boundary G3 promises to rotate. A data-derived branch remains admissible.
    for block in body {
        if *block == header {
            continue;
        }
        let Some(loop_block) = function.blocks.get(*block as usize) else {
            return Err(KeptReason::LoopShape);
        };
        let Term::Branch(condition, left, right) = &loop_block.term else {
            continue;
        };
        if body.contains(left) && body.contains(right) {
            continue;
        }
        let trip_operand = analysis
            .binary(condition, BinOp::Ge)
            .or_else(|| analysis.binary(condition, BinOp::Gt))
            .map(|(index, _)| index);
        if matches!(trip_operand.and_then(|index| analysis.def(index)), Some((_, _, Rvalue::Load(slot))) if *slot == index_slot)
        {
            return Err(KeptReason::LoopShape);
        }
    }
    // The statement scan below sees only writes this function performs itself. A call that takes
    // the index as a `borrow mut` argument writes it through a pointer, which no statement kind
    // names — so the recurrence would be proved against a value the callee had already changed.
    // The index slot must therefore never hand its address out, anywhere in the function: not in
    // the body, and not between its initialization and the loop header either.
    if escaping
        .as_ref()
        .is_none_or(|escaping| escaping.contains(&index_slot))
    {
        return Err(KeptReason::MultipleIndexWrites);
    }

    // Exactly one statement writes the index slot in the body, and it is `i = i + step`.
    let mut writes = Vec::new();
    for block in body {
        for (position, stmt) in function.blocks[*block as usize].stmts.iter().enumerate() {
            if matches!(statement_facts(function, stmt), StmtFacts::WritesSlot(slot, _, _) if slot == index_slot)
            {
                writes.push((*block, position, stmt));
            }
        }
    }
    let [(step_block, step_position, step_stmt)] = writes.as_slice() else {
        return Err(KeptReason::MultipleIndexWrites);
    };
    if !cfg.dominates(*step_block, shape.latch)
        || !matches!(function.blocks.get(shape.latch as usize).map(|block| &block.term), Some(Term::Goto(target)) if *target == header)
    {
        return Err(KeptReason::LoopShape);
    }
    let Stmt::Store(_, stepped) = step_stmt else {
        return Err(KeptReason::MultipleIndexWrites);
    };
    let Some((previous, Operand::Const(Const::Int(step, step_ty)))) =
        analysis.binary(stepped, BinOp::Add)
    else {
        return Err(KeptReason::MultipleIndexWrites);
    };
    if *step_ty != i64_ty() || *step <= 0 || *step > i128::from(i64::MAX) {
        return Err(KeptReason::MultipleIndexWrites);
    }
    if !matches!(analysis.def(previous), Some((_, _, Rvalue::Load(slot))) if *slot == index_slot) {
        return Err(KeptReason::MultipleIndexWrites);
    }
    let step = *step;

    // Every read of the index in the body must see the value the header saw: no read may be
    // reachable from the step without passing the header again (the `shifted_sum` witness).
    let mut barrier = BTreeSet::new();
    barrier.insert(header);
    let mut after_step = BTreeSet::new();
    for target in successors(&function.blocks[*step_block as usize].term) {
        after_step.extend(cfg.reachable_from(function, target, &barrier));
    }
    for block in body {
        for (position, stmt) in function.blocks[*block as usize].stmts.iter().enumerate() {
            let Stmt::Let(_, Rvalue::Load(slot)) = stmt else {
                continue;
            };
            if *slot != index_slot {
                continue;
            }
            let after = if *block == *step_block {
                position > *step_position
            } else {
                after_step.contains(block)
            };
            if after {
                return Err(KeptReason::StepPrecedesAccess);
            }
        }
    }

    // The entry value: exactly one write to the index slot outside the loop, dominating the header.
    // Dominance proves only that this initializer runs before the *first* entry, not before every
    // one: an enclosing (unversioned) loop can re-enter this header without re-executing it, and a
    // previous slow-copy run may have left the slot wrapped (defined two's-complement wrap). So the
    // initializer is evidence only that the slot is initialized and statically non-negative here;
    // the admission arithmetic itself reads the slot's *live* value at the preheader (§3.2.1).
    let mut entries = Vec::new();
    for block in &function.blocks {
        if body.contains(&block.id) || ignore.contains(&block.id) {
            continue;
        }
        for stmt in &block.stmts {
            if matches!(statement_facts(function, stmt), StmtFacts::WritesSlot(slot, _, _) if slot == index_slot)
            {
                entries.push((block.id, stmt));
            }
        }
    }
    let [(entry_block, Stmt::Store(_, entry_value))] = entries.as_slice() else {
        return Err(KeptReason::EntryUnproved);
    };
    if !cfg.dominates(*entry_block, header) {
        return Err(KeptReason::EntryUnproved);
    }
    let entry_non_negative = match entry_value {
        Operand::Const(Const::Int(value, ty)) => *ty == i64_ty() && *value >= 0,
        other => matches!(analysis.def(other), Some((_, _, Rvalue::SliceLen(_)))),
    };
    if !entry_non_negative {
        return Err(KeptReason::EntryUnproved);
    }

    // No value defined in the body may be read after the loop: a second copy would leave such a use
    // dominated by neither.
    let defined: BTreeSet<ValueId> = body
        .iter()
        .flat_map(|block| &function.blocks[*block as usize].stmts)
        .filter_map(|stmt| match stmt {
            Stmt::Let(value, _) => Some(*value),
            _ => None,
        })
        .collect();
    let after_loop: BTreeSet<BlockId> = cfg
        .reachable_from(function, header, &BTreeSet::new())
        .into_iter()
        .filter(|block| !body.contains(block) && !ignore.contains(block))
        .collect();
    for block in &after_loop {
        let block = &function.blocks[*block as usize];
        let mut uses = Vec::new();
        for stmt in &block.stmts {
            let operands = match statement_facts(function, stmt) {
                StmtFacts::Pure(uses)
                | StmtFacts::Call(uses)
                | StmtFacts::WritesElements(uses)
                | StmtFacts::WritesSlot(_, _, uses)
                | StmtFacts::WritesThrough(_, uses) => uses,
                StmtFacts::KillsEverything => return Err(KeptReason::UnmodelledStatement),
            };
            for operand in operands {
                operand_values(operand, &mut uses);
            }
        }
        for operand in term_operands(&block.term) {
            operand_values(operand, &mut uses);
        }
        if uses.iter().any(|value| defined.contains(value)) {
            return Err(KeptReason::ValueEscapesLoop);
        }
    }

    // The admission arithmetic (§3.2.1), all in `i64`, all in the preheader.
    let Some(mut pre) = Preheader::new(function) else {
        return Err(KeptReason::ArithmeticUnproved);
    };
    let mut remat = Remat {
        analysis,
        body,
        facts: &facts,
        escaping: escaping.as_ref(),
        header,
        memo: BTreeMap::new(),
        blocked_by: None,
    };
    let root_killed = |remat: &Remat<'_>| {
        KeptReason::RootKilled(remat.blocked_by.unwrap_or("Call"))
    };
    // The initializer proves the slot's first value. An enclosing loop can re-enter this header
    // without re-executing it, and a slow-copy run may have wrapped the index, so the admission
    // must read the value the header will actually see. The preheader sits on every edge into the
    // header (`apply`) and the slot's address never leaves the function (checked above), so this
    // load is that value.
    let entry = pre.emit(i64_ty(), Rvalue::Load(index_slot));
    let Some(bound) = remat.get(bound, &mut pre) else {
        return Err(if remat.blocked_by.is_some() {
            root_killed(&remat)
        } else {
            KeptReason::BoundKilled
        });
    };

    let zero_trip = pre.bin(relation, Ty::Bool, entry.clone(), bound.clone());
    let fact_bound = bound.clone();
    let last_bound = if relation == BinOp::Ge {
        pre.bin(BinOp::Sub, i64_ty(), bound.clone(), int(1))
    } else {
        bound.clone()
    };
    let span = pre.bin(BinOp::Sub, i64_ty(), last_bound, entry.clone());
    let quotient = pre.bin(BinOp::Div, i64_ty(), span, int(step));
    let trip_count = pre.bin(BinOp::Add, i64_ty(), quotient.clone(), int(1));
    let scaled = pre.bin(BinOp::Mul, i64_ty(), int(step), quotient);
    let imax = pre.bin(BinOp::Add, i64_ty(), entry.clone(), scaled);
    // The loop performs one final step after the last body iteration. Constrain that exact last
    // body index rather than `bound`: for `0 .. i64::MAX` with step 1, `imax` is
    // `i64::MAX - 1` and the final step to the bound is valid. Constraining the bound itself would
    // needlessly retain a slow copy which then prevents LLVM's early-exit vectorizer from using
    // the otherwise canonical fast loop.
    let induction = pre.bin(
        BinOp::Le,
        Ty::Bool,
        imax.clone(),
        int(i128::from(i64::MAX) - step),
    );

    // The initializer only proved non-negativity of the *first* entry (§3.2); the live entry read
    // above needs its own runtime check, exactly as every other admission operand does.
    let entry_ok = pre.bin(BinOp::Ge, Ty::Bool, entry.clone(), int(0));
    let mut conjuncts = vec![entry_ok, induction];
    let mut proved = Vec::new();
    let mut extent_slots = BTreeSet::new();
    let mut extents = Vec::new();
    for guard in &guards {
        let Some(len) = remat.get(&guard.len, &mut pre) else {
            return Err(if remat.blocked_by.is_some() {
                root_killed(&remat)
            } else {
                KeptReason::ArithmeticUnproved
            });
        };
        if has_early_exit
            && let Some((slot, view, elem)) = view_extent_source(analysis, &guard.len)
            && extent_slots.insert(slot)
            && let Some(view) = remat.get(view, &mut pre)
        {
            extents.push(DataExtent {
                view,
                len: len.clone(),
                elem,
            });
        }
        let Some(low) = affine(&mut remat, &mut pre, index_slot, &guard.start) else {
            return Err(KeptReason::AccessNotAffine);
        };
        let high = match &guard.end {
            None => Affine {
                a: low.a,
                b: low.b.clone(),
            },
            Some(end) => {
                let high = affine(&mut remat, &mut pre, index_slot, end)
                    .ok_or(KeptReason::AccessNotAffine)?;
                if high.a != low.a {
                    return Err(KeptReason::AccessNotAffine);
                }
                // `start <= end` for every iteration reduces to the offsets, because both share `a`.
                let ordered = pre.bin(BinOp::Le, Ty::Bool, low.b.clone(), high.b.clone());
                conjuncts.push(ordered);
                high
            }
        };
        if low.a <= 0 || low.a > i128::from(i64::MAX) {
            return Err(KeptReason::AccessNotAffine);
        }
        // `a * imax` is overflow-free exactly when `imax <= i64::MAX / a`; `a` is a positive
        // compile-time constant, so the division is folded here and never emitted.
        let scale_ok = pre.bin(
            BinOp::Le,
            Ty::Bool,
            imax.clone(),
            int(i128::from(i64::MAX) / low.a),
        );
        let top = pre.bin(BinOp::Mul, i64_ty(), int(low.a), imax.clone());
        // `amax + b` overflows only for a non-negative `b`; the negative case is selected by the
        // `Or`, and the wrapped `i64::MAX - b` it also computes is defined, not undefined.
        let offset_negative = pre.bin(BinOp::Lt, Ty::Bool, high.b.clone(), int(0));
        let room = pre.bin(
            BinOp::Sub,
            i64_ty(),
            int(i128::from(i64::MAX)),
            high.b.clone(),
        );
        let fits = pre.bin(BinOp::Le, Ty::Bool, top.clone(), room);
        let sum_ok = pre.bin(BinOp::Or, Ty::Bool, offset_negative, fits);
        let highest = pre.bin(BinOp::Add, i64_ty(), top, high.b.clone());
        let base = pre.bin(BinOp::Mul, i64_ty(), int(low.a), entry.clone());
        let lowest = pre.bin(BinOp::Add, i64_ty(), base, low.b.clone());
        let low_ok = pre.bin(BinOp::Ge, Ty::Bool, lowest, int(0));
        // An element index must be `< len`; a range end must be `<= len`.
        let high_ok = pre.bin(
            if guard.end.is_some() {
                BinOp::Le
            } else {
                BinOp::Lt
            },
            Ty::Bool,
            highest,
            len,
        );
        conjuncts.extend([scale_ok, sum_ok, low_ok, high_ok]);
        proved.push((guard.block, guard.fail, guard.ok));
    }

    let all = pre.all(conjuncts);
    let admit = pre.bin(BinOp::Or, Ty::Bool, zero_trip, all);
    let mut body: Vec<BlockId> = body.iter().copied().collect();
    body.sort_unstable();
    Ok(Plan {
        body,
        preheader: pre,
        admit,
        proved,
        budget_used,
        counted: has_early_exit.then_some(CountedPlan {
            entry,
            trip_count,
            index: index_slot,
            step: i64::try_from(step).ok().ok_or(KeptReason::ArithmeticUnproved)?,
            relation,
            body_arm: *body_arm,
            exit_arm: *exit_arm,
            latch: shape.latch,
            stepped: stepped.clone(),
            fact_bound,
            bound: latch_bound,
        }),
        extents,
    })
}

// ---------------------------------------------------------------------------------------------
// The rewrite.
// ---------------------------------------------------------------------------------------------

/// Apply one admitted plan: append the fast copy, insert the preheader, and redirect every edge
/// that entered the loop from outside. Returns the blocks the clone created.
struct AppliedPlan {
    blocks: BTreeSet<BlockId>,
    counted: Option<CountedLoopFact>,
}

fn apply(function: &mut Function, header: BlockId, plan: Plan) -> Option<AppliedPlan> {
    // Every id minted here is a `u32`. A function large enough to wrap one is refused rather than
    // rewritten with a colliding block or value id.
    let block_base = BlockId::try_from(function.blocks.len()).ok()?;
    let mut block_map: BTreeMap<BlockId, BlockId> = BTreeMap::new();
    for (offset, block) in plan.body.iter().enumerate() {
        block_map.insert(*block, block_base.checked_add(BlockId::try_from(offset).ok()?)?);
    }
    let preheader_id = block_base.checked_add(BlockId::try_from(plan.body.len()).ok()?)?;
    let rotate_entry_id = if plan.counted.is_some() {
        Some(preheader_id.checked_add(1)?)
    } else {
        None
    };

    // Fresh SSA values for every definition in the clone, allocated after the preheader's own.
    let mut value_map: BTreeMap<ValueId, ValueId> = BTreeMap::new();
    let mut cloned_tys: Vec<Ty> = Vec::new();
    let value_base = plan.preheader.next_value;
    for block in &plan.body {
        for stmt in &function.blocks[*block as usize].stmts {
            if let Stmt::Let(value, _) = stmt {
                let ty = *function.value_tys.get(*value as usize)?;
                value_map.insert(
                    *value,
                    value_base.checked_add(ValueId::try_from(cloned_tys.len()).ok()?)?,
                );
                cloned_tys.push(ty);
            }
        }
    }

    let mut clones = Vec::new();
    for block in &plan.body {
        let mut cloned = function.blocks[*block as usize].clone();
        cloned.id = *block_map.get(block)?;
        for stmt in &mut cloned.stmts {
            if let Stmt::Let(value, _) = stmt {
                *value = *value_map.get(value)?;
            }
            for operand in stmt_operands_mut(stmt)? {
                remap_operand(operand, &value_map);
            }
        }
        for operand in term_operands_mut(&mut cloned.term) {
            remap_operand(operand, &value_map);
        }
        for target in term_targets_mut(&mut cloned.term) {
            if let Some(mapped) = block_map.get(target) {
                *target = *mapped;
            }
        }
        clones.push(cloned);
    }

    // A counted loop with a body-derived exit receives the G3 rotation. A loop with only its trip
    // exit keeps PR 2's existing versioned shape: rotation buys it nothing and the G3 contract
    // explicitly leaves that shape unchanged.
    let mut rotated_body = None;
    let mut rotated_operands = None;
    if let Some(counted) = &plan.counted {
        let rotate_entry_id = rotate_entry_id?;
        let header_index = plan.body.iter().position(|block| *block == header)?;
        let latch_index = plan.body.iter().position(|block| *block == counted.latch)?;
        let expected_exit = counted.exit_arm;
        let expected_body = *block_map.get(&counted.body_arm)?;
        let header_clone = clones.get_mut(header_index)?;
        let Term::Branch(zero_trip, found_exit, found_body) = header_clone.term.clone()
        else {
            return None;
        };
        if found_exit != expected_exit || found_body != expected_body {
            return None;
        }
        header_clone.term = Term::Branch(zero_trip, found_exit, rotate_entry_id);

        let mut stepped = counted.stepped.clone();
        remap_operand(&mut stepped, &value_map);
        let mut latch_bound = counted.bound.clone();
        remap_operand(&mut latch_bound, &value_map);
        let latch_clone = clones.get_mut(latch_index)?;
        if !matches!(latch_clone.term, Term::Goto(target) if target == *block_map.get(&header)?)
        {
            return None;
        }
        let compare = value_base.checked_add(ValueId::try_from(cloned_tys.len()).ok()?)?;
        cloned_tys.push(Ty::Bool);
        latch_clone.stmts.push(Stmt::Let(
            compare,
            Rvalue::Bin(counted.relation, stepped.clone(), latch_bound.clone()),
        ));
        latch_clone.stmt_lines.push((0, 0));
        latch_clone.term = Term::Branch(
            Operand::Value(compare),
            counted.exit_arm,
            expected_body,
        );
        rotated_body = Some(expected_body);
        rotated_operands = Some((stepped, latch_bound));
    }

    // Bypass each proved guard in the fast copy, and empty its failure block so the function's trap
    // call-site count is exactly what it was (§3.3).
    let index_of = |block: BlockId| plan.body.iter().position(|found| *found == block);
    for (guard, fail, ok) in &plan.proved {
        let guard_index = index_of(*guard)?;
        let fail_index = index_of(*fail)?;
        clones[guard_index].term = Term::Goto(*block_map.get(ok)?);
        clones[fail_index].stmts.clear();
        clones[fail_index].stmt_lines.clear();
        clones[fail_index].term = Term::Unreachable;
    }

    // Exceptional edges are producer-owned CFG facts. Clone every still-live exceptional branch
    // with the loop body, but do not retain a record for a proved guard that the fast copy just
    // replaced with `Goto`.
    let cloned_exceptional_edges = function
        .exceptional_edges
        .iter()
        .filter_map(|edge| {
            let cloned_block = *block_map.get(&edge.block)?;
            let clone = clones.get(index_of(edge.block)?)?;
            matches!(clone.term, Term::Branch(..)).then_some(crate::ExceptionalEdge {
                block: cloned_block,
                unlikely: edge.unlikely,
                kind: edge.kind,
            })
        })
        .collect::<Vec<_>>();

    // Every edge that entered the header from outside the loop now enters the preheader instead.
    let body_set: BTreeSet<BlockId> = plan.body.iter().copied().collect();
    for block in &mut function.blocks {
        if body_set.contains(&block.id) {
            continue;
        }
        for target in term_targets_mut(&mut block.term) {
            if *target == header {
                *target = preheader_id;
            }
        }
    }

    let preheader = crate::Block {
        id: preheader_id,
        stmt_lines: Vec::new(),
        stmts: plan.preheader.stmts,
        term: Term::Branch(plan.admit, *block_map.get(&header)?, header),
    };
    function.value_tys.extend(plan.preheader.new_tys);
    function.value_tys.extend(cloned_tys);
    function.blocks.extend(clones);
    function.exceptional_edges.extend(cloned_exceptional_edges);
    function.blocks.push(preheader);
    if let (Some(rotate_entry_id), Some(expected_body)) = (rotate_entry_id, rotated_body) {
        function.blocks.push(crate::Block {
            id: rotate_entry_id,
            stmt_lines: Vec::new(),
            stmts: Vec::new(),
            term: Term::Goto(expected_body),
        });
    }
    let counted = match plan.counted {
        Some(counted) => {
            let (stepped, latch_bound) = rotated_operands?;
            Some(CountedLoopFact {
                preheader: rotate_entry_id?,
                header: rotated_body?,
                latch: *block_map.get(&counted.latch)?,
                exit: counted.exit_arm,
                entry: counted.entry,
                trip_count: counted.trip_count,
                index: counted.index,
                step: counted.step,
                relation: counted.relation,
                bound: counted.fact_bound,
                extents: plan.extents,
                stepped,
                latch_bound,
            })
        }
        None => None,
    };
    let mut blocks: BTreeSet<_> = block_map
        .values()
        .copied()
        .chain([preheader_id])
        .collect();
    blocks.extend(rotate_entry_id);
    Some(AppliedPlan {
        blocks,
        counted,
    })
}

fn remap_operand(operand: &mut Operand, value_map: &BTreeMap<ValueId, ValueId>) {
    match operand {
        Operand::Value(value) => {
            if let Some(mapped) = value_map.get(value) {
                *value = *mapped;
            }
        }
        Operand::BorrowedElementPlace(place) => {
            remap_operand(&mut place.index, value_map);
            remap_operand(&mut place.guard.len, value_map);
        }
        Operand::Const(_)
        | Operand::Arg(_)
        | Operand::BorrowedCleanupArg(_)
        | Operand::BorrowedPlace(_)
        | Operand::BorrowedFixedElementPlace(_) => {}
    }
}

/// One versioned loop's record: its header, the slow copy's blocks, and the `(guard, failure,
/// success)` triples whose branch the fast copy bypasses.
type VersionedLoop = (BlockId, BTreeSet<BlockId>, Vec<(BlockId, BlockId, BlockId)>);

/// Structural re-validation of the rewritten function: block ids index their own slot, every
/// successor exists, and every SSA value is defined exactly once and has a type.
fn structurally_valid(function: &Function) -> bool {
    let mut defined = BTreeSet::new();
    for (index, block) in function.blocks.iter().enumerate() {
        if block.id as usize != index {
            return false;
        }
        for target in successors(&block.term) {
            if target as usize >= function.blocks.len() {
                return false;
            }
        }
        for stmt in &block.stmts {
            if let Stmt::Let(value, _) = stmt
                && (!defined.insert(*value) || function.value_tys.get(*value as usize).is_none())
            {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------------------------
// The pass.
// ---------------------------------------------------------------------------------------------

/// Version every admissible loop of one function, innermost-first, and report one decision per
/// source loop. Fail-closed throughout: any disagreement between the facts the rewrite was built
/// from and the facts the rewritten function re-derives discards the whole rewrite.
pub fn version_loops(function: &mut Function) -> (Vec<LoopDecision>, Vec<CountedLoopFact>) {
    let original = function.clone();
    let original_traps = trap_call_count(&original);
    let mut decided: BTreeSet<BlockId> = BTreeSet::new();
    let mut cloned: BTreeSet<BlockId> = BTreeSet::new();
    let mut versioned: Vec<VersionedLoop> = Vec::new();
    let mut decisions: Vec<LoopDecision> = Vec::new();
    let mut counted: Vec<CountedLoopFact> = Vec::new();

    while let Some(cfg) = Cfg::build(function) {
        let Some(loops) = discover_loops(function, &cfg) else {
            break;
        };
        let Some(shape) = loops
            .into_iter()
            .find(|shape| !decided.contains(&shape.header) && !cloned.contains(&shape.header))
        else {
            break;
        };
        decided.insert(shape.header);
        let Some(analysis) = Analysis::new(function, &cfg) else {
            decisions.push(LoopDecision::KeptChecks(KeptReason::LoopShape));
            continue;
        };
        match admit(function, &cfg, &analysis, &shape, &cloned) {
            Err(reason) => decisions.push(LoopDecision::KeptChecks(reason)),
            Ok(plan) => {
                let budget_used = plan.budget_used;
                let proved = plan.proved.clone();
                let body = shape.body.clone();
                drop(analysis);
                match apply(function, shape.header, plan) {
                    Some(applied) => {
                        cloned.extend(applied.blocks);
                        counted.extend(applied.counted);
                        versioned.push((shape.header, body, proved));
                        decisions.push(LoopDecision::Versioned { budget_used });
                    }
                    None => {
                        decisions.push(LoopDecision::KeptChecks(KeptReason::ArithmeticUnproved))
                    }
                }
            }
        }
    }

    if versioned.is_empty() {
        return (decisions, counted);
    }
    if !rederives(
        function,
        &original,
        original_traps,
        &versioned,
        &cloned,
        &counted,
    ) {
        *function = original;
        return (
            decisions
            .into_iter()
            .map(|decision| match decision {
                LoopDecision::Versioned { .. } => {
                    LoopDecision::KeptChecks(KeptReason::RederivationFailed)
                }
                other => other,
            })
            .collect(),
            Vec::new(),
        );
    }
    (decisions, counted)
}

/// Re-derive the facts the rewrite was built from, **from the rewritten function**, and check them.
fn rederives(
    function: &Function,
    original: &Function,
    original_traps: usize,
    versioned: &[VersionedLoop],
    cloned: &BTreeSet<BlockId>,
    counted: &[CountedLoopFact],
) -> bool {
    if !structurally_valid(function) || trap_call_count(function) != original_traps {
        return false;
    }
    // Every slow copy is byte-identical to the body it was derived from: versioning moves no
    // statement out of it and deletes no guard from it.
    for (_, body, _) in versioned {
        for block in body {
            let Some(before) = original.blocks.get(*block as usize) else {
                return false;
            };
            let Some(after) = function.blocks.get(*block as usize) else {
                return false;
            };
            if crate::print::block_text(before) != crate::print::block_text(after) {
                return false;
            }
        }
    }
    let Some(cfg) = Cfg::build(function) else {
        return false;
    };
    let Some(loops) = discover_loops(function, &cfg) else {
        return false;
    };
    let Some(analysis) = Analysis::new(function, &cfg) else {
        return false;
    };
    for fact in counted {
        if fact.step <= 0
            || function.slots.get(fact.index as usize) != Some(&i64_ty())
            || !matches!(function.blocks.get(fact.preheader as usize).map(|block| &block.term), Some(Term::Goto(target)) if *target == fact.header)
        {
            return false;
        }
        let Some(shape) = loops.iter().find(|shape| shape.header == fact.header) else {
            return false;
        };
        if shape.latch != fact.latch {
            return false;
        }
        let Some(latch) = function.blocks.get(fact.latch as usize) else {
            return false;
        };
        let Term::Branch(condition, exit, body) = &latch.term else {
            return false;
        };
        if *exit != fact.exit || *body != fact.header {
            return false;
        }
        let Some((stepped, bound)) = analysis.binary(condition, fact.relation) else {
            return false;
        };
        if !same_operand(stepped, &fact.stepped) || !same_operand(bound, &fact.latch_bound) {
            return false;
        }
        let Some((step_block, _, Rvalue::Bin(BinOp::Add, previous, delta))) =
            analysis.def(stepped)
        else {
            return false;
        };
        if !cfg.dominates(step_block, fact.latch)
            || !matches!(delta, Operand::Const(Const::Int(step, ty)) if *ty == i64_ty() && *step == i128::from(fact.step))
            || !matches!(analysis.def(previous), Some((_, _, Rvalue::Load(slot))) if *slot == fact.index)
            || !function
                .blocks
                .get(step_block as usize)
                .is_some_and(|block| {
                    block.stmts.iter().any(|statement| {
                        matches!(statement, Stmt::Store(slot, value) if *slot == fact.index && same_operand(value, stepped))
                    })
                })
        {
            return false;
        }
        let Some((quotient, one)) = analysis.binary(&fact.trip_count, BinOp::Add) else {
            return false;
        };
        let Some((span, divisor)) = analysis.binary(quotient, BinOp::Div) else {
            return false;
        };
        let Some((last, entry)) = analysis.binary(span, BinOp::Sub) else {
            return false;
        };
        if !same_operand(one, &int(1))
            || !same_operand(divisor, &int(i128::from(fact.step)))
            || !same_operand(entry, &fact.entry)
        {
            return false;
        }
        match fact.relation {
            BinOp::Ge => {
                let Some((bound, one)) = analysis.binary(last, BinOp::Sub) else {
                    return false;
                };
                if !same_operand(bound, &fact.bound) || !same_operand(one, &int(1)) {
                    return false;
                }
            }
            BinOp::Gt if same_operand(last, &fact.bound) => {}
            _ => return false,
        }
        let mut extent_slots = BTreeSet::new();
        for extent in &fact.extents {
            let Some((slot, view, elem)) = view_extent_source(&analysis, &extent.len) else {
                return false;
            };
            if !same_operand(view, &extent.view)
                || elem != extent.elem
                || !extent_slots.insert(slot)
            {
                return false;
            }
        }
        for operand in [
            &fact.entry,
            &fact.trip_count,
            &fact.bound,
        ]
        .into_iter()
        .chain(fact.extents.iter().flat_map(|extent| [&extent.view, &extent.len]))
        {
            if matches!(operand, Operand::Value(_)) {
                let Some((block, _, _)) = analysis.def(operand) else {
                    return false;
                };
                if !cfg.dominates(block, fact.preheader) {
                    return false;
                }
            }
        }
    }
    // Each versioned loop still re-admits from the rewritten function, with the fast copy ignored:
    // the induction structure the fast copy was built on must still be the one the slow copy has.
    for (header, _, proved) in versioned {
        let Some(shape) = loops.iter().find(|shape| shape.header == *header) else {
            return false;
        };
        match admit(function, &cfg, &analysis, shape, cloned) {
            Ok(plan) => {
                if plan.proved != *proved {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Block, ProgramCall};

    /// A body with one `i64` slot, one `i64` value and one `slice<i64>` slot, enough to build every
    /// statement shape the kill-set owner classifies.
    fn scratch() -> Function {
        Function {
            // `from_validated` is the crate-internal infallible constructor, so this owner adds no
            // panic source: the compiler must diagnose, never panic, and a test is no exception.
            name: ProgramCall::from_validated("scratch"),
            params: Vec::new(),
            param_modes: Vec::new(),
            borrow_mut_cleanup_slots: Vec::new(),
            ret: Ty::Unit,
            return_borrow: align_sema::hir::ReturnBorrowSummary::None,
            return_region: align_sema::hir::ReturnRegionSummary::None,
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
            slots: vec![i64_ty(), Ty::Slice(align_sema::Scalar::Int(IntTy { bits: 64, signed: true }))],
            slot_align: vec![None, None],
            value_tys: vec![i64_ty(), Ty::Str],
            blocks: vec![Block {
                id: 0,
                stmts: Vec::new(),
                stmt_lines: Vec::new(),
                term: Term::Unreachable,
            }],
            entry: 0,
            exceptional_edges: Vec::new(),
            cold: false,
            exportable: false,
            available_externally: false,
        }
    }

    /// The kill set, closed once for the whole `Stmt` inventory rather than by one Align fixture per
    /// variant — most of these statements have no source spelling that could put them inside a
    /// versionable loop, and the ones that do (`ArrayTruncate`, `Store`, a call) already have their
    /// executable owners in `loop_facts.rs`. What has to hold for *every* variant is the
    /// classification itself: a statement that can write a view header must never read as "writes
    /// nothing", and a new variant must be a build error rather than a silent omission — which
    /// [`variant_sweep_tripwire`] and the wildcard-free matches enforce at compile time.
    #[test]
    fn every_statement_kind_is_classified_and_none_reads_as_harmless() {
        let function = scratch();
        let scalar = Operand::Const(Const::Int(0, i64_ty()));
        let header = Operand::Value(1); // a `str` value: storage that can hold a view header
        let value = Operand::Value(0);
        let cases: Vec<(&str, Stmt, bool)> = vec![
            // (kind, statement, may this statement leave every root invariant?)
            ("Store", Stmt::Store(1, value.clone()), false),
            ("StoreField", Stmt::StoreField(1, vec![0], value.clone()), false),
            ("StoreIndex", Stmt::StoreIndex(1, scalar.clone(), scalar.clone()), true),
            (
                "StoreIndex(header element)",
                Stmt::StoreIndex(1, scalar.clone(), header.clone()),
                false,
            ),
            (
                "StoreConstArray",
                Stmt::StoreConstArray { slot: 1, elems: Vec::new(), elem: i64_ty() },
                false,
            ),
            ("PtrStore", Stmt::PtrStore(value.clone(), scalar.clone(), scalar.clone()), true),
            (
                "PtrStore(header element)",
                Stmt::PtrStore(value.clone(), scalar.clone(), header.clone()),
                false,
            ),
            (
                "PtrStoreNoalias",
                Stmt::PtrStoreNoalias {
                    ptr: value.clone(),
                    index: scalar.clone(),
                    value: scalar.clone(),
                    scope: 0,
                },
                true,
            ),
            (
                "VecStore",
                Stmt::VecStore {
                    slice: value.clone(),
                    index: scalar.clone(),
                    value: scalar.clone(),
                    elem: i64_ty(),
                    n: 2,
                },
                true,
            ),
            (
                "StoreElemField",
                Stmt::StoreElemField(1, scalar.clone(), vec![0], header.clone()),
                false,
            ),
            (
                "StoreElemFieldPtr",
                Stmt::StoreElemFieldPtr {
                    base: value.clone(),
                    index: scalar.clone(),
                    path: vec![0],
                    struct_id: 0,
                    value: header.clone(),
                },
                false,
            ),
            (
                "StoreColumn",
                Stmt::StoreColumn {
                    base: value.clone(),
                    len: scalar.clone(),
                    index: scalar.clone(),
                    field: 0,
                    struct_id: 0,
                    value: header.clone(),
                },
                false,
            ),
            ("ArenaEnd", Stmt::ArenaEnd(value.clone()), false),
            ("RawFree", Stmt::RawFree(value.clone()), false),
            (
                "ColumnBatchFinish",
                Stmt::ColumnBatchFinish { payload: value.clone(), struct_id: 0 },
                false,
            ),
            (
                "ColumnBatchDrop",
                Stmt::ColumnBatchDrop { payload: value.clone(), struct_id: 0 },
                false,
            ),
            (
                "RawStore",
                Stmt::RawStore {
                    ptr: value.clone(),
                    offset: scalar.clone(),
                    value: scalar.clone(),
                },
                false,
            ),
            ("TgWait", Stmt::TgWait(value.clone()), false),
            ("TgEnd", Stmt::TgEnd(value.clone()), false),
            ("DropFlagInit", Stmt::DropFlagInit(1), false),
            (
                "DropFlagMoveOut",
                Stmt::DropFlagMoveOut { slot: 1, flag: 2 },
                false,
            ),
            ("NullTupleField", Stmt::NullTupleField(1, 0), false),
            ("NullStructField", Stmt::NullStructField(1, 0), false),
            ("NullElemField", Stmt::NullElemField(1, scalar.clone(), vec![0]), false),
            ("Drop", Stmt::Drop(1), false),
            ("DropField", Stmt::DropField(1, vec![0]), false),
            ("DropElem", Stmt::DropElem(1, scalar.clone(), 0), false),
            ("DropElemField", Stmt::DropElemField(1, scalar.clone(), vec![0]), false),
            (
                "BorrowedElementReservation",
                Stmt::BorrowedElementReservation { token: 0, root: 1 },
                true, // not a write; admission refuses the body for the token itself
            ),
            ("DropValue", Stmt::DropValue(value.clone()), false),
            (
                "ArrayTruncate",
                Stmt::ArrayTruncate { root: 1, path: Vec::new(), new_len: scalar.clone() },
                false,
            ),
            ("Let(pure)", Stmt::Let(0, Rvalue::Load(0)), true),
            (
                "Let(call)",
                Stmt::Let(0, Rvalue::Call(DirectCall::Runtime(RuntimeKey::BoundsFail), Vec::new())),
                true, // a call is opaque, not harmless: `slot_is_invariant` applies its own rule
            ),
            ("Let(unmodelled)", Stmt::Let(0, Rvalue::StrClone(value.clone())), false),
        ];
        for (kind, stmt, harmless) in cases {
            let mut body = BTreeSet::new();
            body.insert(0u32);
            let mut host = function.clone();
            host.blocks[0].stmts = vec![stmt];
            let facts = body_facts(&host, &body);
            let invariant = !facts.unmodelled
                && !facts.killed.contains_key(&1)
                && !facts.killed.contains_key(&Slot::MAX);
            assert_eq!(
                invariant, harmless,
                "[{kind}] a statement that can write a view header must kill the root"
            );
        }
    }

    /// The tripwire itself is a compile-time construct; this only keeps it referenced so a future
    /// `Stmt` variant fails the build here as well as in the classifiers.
    #[test]
    fn the_variant_sweep_tripwire_is_live() {
        variant_sweep_tripwire(&Stmt::Drop(0));
    }
}
