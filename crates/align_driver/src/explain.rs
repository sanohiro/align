//! `alignc explain-opt` — translate LLVM's optimization remarks into Align's diagnostic voice
//! (`docs/impl/09-explain-opt.md`, Slice 3b).
//!
//! Pipeline: front end → located MIR → codegen(+debug locations) → `default<O2>` with remark
//! capture → parse each flat `"<file>:<line>:<col>: <message>"` string → classify into a
//! [`Vec<OptRecord>`] (build first) → render the human report (render second). The default view is
//! the missed/actionable records plus a one-line success summary plus a bucket count; `--verbose`
//! adds the passed records, the untranslated remarks as raw `[llvm …]` passthrough, and the
//! suppressed internal-location remarks.
//!
//! Keying reality (empirically pinned against LLVM 19, `docs/impl/09-explain-opt.md`): the C API
//! yields only a flat message string, so the table matches on message *patterns*, re-verified at the
//! LLVM upgrade. Each row carries our own stable [`ReasonCode`] so the human text (and a future JSON
//! view) never depends on LLVM prose. Honesty rule: a record asserts only what the remark justifies
//! — a cost-model decline says only that; no fabricated aliasing story, no invented suggestions.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use align_span::SourceMap;

use crate::{check, collect_opt_remarks, format_diagnostics, lower_to_mir_located, BuildTarget, DebugInfo};

/// What LLVM did to a construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Vectorized,
    NotVectorized,
    Inlined,
    NotInlined,
    Other,
}

/// Whether the optimizer succeeded, declined, or merely analyzed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Passed,
    Missed,
    Analysis,
}

/// A stable, LLVM-prose-independent reason for a verdict. The human text and any future JSON key off
/// this, not the raw remark string. (`FpReorder` extends the `09-explain-opt.md` list: the
/// FP-reassociation decline is a distinct, mappable cause with a concrete remedy — recorded as a
/// deliberate deviation.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasonCode {
    /// The optimizer succeeded — no decline reason.
    None,
    MayAlias,
    CostModel,
    CallNoVectorForm,
    ReductionNotRecognized,
    FpReorder,
    UnknownTripCount,
    Unspecified,
}

/// One optimization event on a user-source construct, built from a raw remark. The report printer
/// consumes a `Vec<OptRecord>`; a future `--format json` / optimization score / CI count-gate are
/// second consumers of the same vector (`docs/impl/09-explain-opt.md`, deferrals).
#[derive(Clone, Debug)]
pub struct OptRecord {
    pub kind: Kind,
    pub verdict: Verdict,
    /// The LLVM pass that produced the remark (`loop-vectorize`, `inline`, `slp-vectorize`, …).
    pub pass: String,
    pub reason: ReasonCode,
    pub file: String,
    pub line: u32,
    pub col: u32,
    /// The rendered Align-voice sentence (empty for a summary-only record).
    pub message: String,
    /// The raw LLVM remark text (`--verbose` passthrough).
    pub llvm_detail: String,
}

/// A remark we do not translate into a record — counted, and shown raw under `--verbose`.
/// (Compiler-internal `<unknown>:0:0` remarks are handled before classification, so they are not a
/// bucket here.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bucket {
    /// A remark about runtime/std code (`align_rt_*`). Counted separately; raw under `--verbose`.
    LibraryRuntime,
    /// Everything else LLVM emitted that v1 does not translate (unroll, GVN, interleave cost, SLP
    /// analysis, the redundant bare `loop not vectorized`). Raw under `--verbose`.
    Other,
}

/// The classification of a raw remark: either a translated record or a bucket.
enum Class {
    Record(OptRecord),
    /// A passed record that only feeds the summary (SLP stores, inline successes) — not shown as an
    /// itemized line in the default view, but counted.
    Summary(SummaryHit),
    Bucketed(Bucket),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryHit {
    Inlined,
    SlpStores,
}

/// A raw remark split into its location and message.
struct RawRemark {
    file: String,
    line: u32,
    col: u32,
    message: String,
    raw: String,
}

impl RawRemark {
    /// Parse `"<file>:<line>:<col>: <message>"`. The location has no spaces (colon-joined), so the
    /// first `": "` (colon-space) separates it from the message. Returns `None` if the shape does
    /// not match (then the whole line is kept as raw, bucketed Other).
    fn parse(s: &str) -> Option<RawRemark> {
        let (loc, message) = s.split_once(": ")?;
        // loc = file:line:col — split the two trailing numeric fields off the right.
        let mut it = loc.rsplitn(3, ':');
        let col = it.next()?.parse::<u32>().ok()?;
        let line = it.next()?.parse::<u32>().ok()?;
        let file = it.next()?.to_string();
        if file.is_empty() {
            return None;
        }
        Some(RawRemark { file, line, col, message: message.to_string(), raw: s.to_string() })
    }
}

/// The built report: records + bucket counts, ready to render.
pub struct Report {
    pub records: Vec<OptRecord>,
    inlined: u32,
    slp_stores: u32,
    internal: Vec<String>,
    library_runtime: Vec<String>,
    other: Vec<String>,
}

impl Report {
    /// Build the report from LLVM's raw remark strings (build first, render second).
    pub fn build(remarks: &[String]) -> Report {
        let mut records = Vec::new();
        let mut inlined = 0u32;
        let mut slp_stores = 0u32;
        let mut internal = Vec::new();
        let mut library_runtime = Vec::new();
        let mut other = Vec::new();

        for r in remarks {
            let Some(raw) = RawRemark::parse(r) else {
                other.push(r.clone());
                continue;
            };
            // Anchoring policy: a compiler-generated construct with no user span is suppressed
            // (never fabricated), regardless of its message.
            if raw.file == "<unknown>" || raw.line == 0 {
                internal.push(raw.raw);
                continue;
            }
            match classify(&raw) {
                Class::Record(rec) => records.push(rec),
                Class::Summary(SummaryHit::Inlined) => inlined += 1,
                Class::Summary(SummaryHit::SlpStores) => slp_stores += 1,
                Class::Bucketed(Bucket::LibraryRuntime) => library_runtime.push(raw.raw),
                Class::Bucketed(Bucket::Other) => other.push(raw.raw),
            }
        }
        Report { records, inlined, slp_stores, internal, library_runtime, other }
    }

    fn vectorized(&self) -> usize {
        self.records.iter().filter(|r| r.kind == Kind::Vectorized && r.verdict == Verdict::Passed).count()
    }

    fn not_vectorized(&self) -> usize {
        self.records.iter().filter(|r| r.kind == Kind::NotVectorized && r.verdict == Verdict::Missed).count()
    }

    /// Total non-record remarks folded into the buckets.
    fn bucket_total(&self) -> usize {
        self.internal.len() + self.library_runtime.len() + self.other.len()
    }

    /// Render the human report. `verbose` adds the passed records, raw untranslated passthrough, and
    /// the suppressed internal remarks.
    pub fn render(&self, verbose: bool) -> String {
        let mut out = String::new();

        // 1. Missed / actionable records — the default focus, one line each, in the diagnostic voice.
        for rec in self.records.iter().filter(|r| r.verdict == Verdict::Missed) {
            let _ = writeln!(out, "{}:{}:{}: {}", rec.file, rec.line, rec.col, rec.message);
        }

        // 2. Passed records — itemized only under --verbose.
        if verbose {
            for rec in self.records.iter().filter(|r| r.verdict == Verdict::Passed) {
                let _ = writeln!(out, "{}:{}:{}: {}", rec.file, rec.line, rec.col, rec.message);
            }
        }

        // 3. One-line success summary.
        let _ = writeln!(out, "{}", self.summary_line());

        // 4. Bucket count (default) / raw passthrough (verbose).
        if verbose {
            if !self.library_runtime.is_empty() {
                let _ = writeln!(out, "\n{} remark(s) in library/runtime code:", self.library_runtime.len());
                for r in &self.library_runtime {
                    let _ = writeln!(out, "  [llvm] {r}");
                }
            }
            if !self.other.is_empty() {
                let _ = writeln!(out, "\n{} other LLVM remark(s):", self.other.len());
                for r in &self.other {
                    let _ = writeln!(out, "  [llvm] {r}");
                }
            }
            if !self.internal.is_empty() {
                let _ = writeln!(out, "\n{} remark(s) in compiler-internal code (no user source):", self.internal.len());
                for r in &self.internal {
                    let _ = writeln!(out, "  [llvm] {r}");
                }
            }
        } else {
            let total = self.bucket_total();
            if total > 0 {
                let lib = self.library_runtime.len();
                if lib > 0 {
                    let _ = writeln!(out, "+ {total} other LLVM remarks ({lib} in library/runtime code) (see --verbose)");
                } else {
                    let _ = writeln!(out, "+ {total} other LLVM remarks (see --verbose)");
                }
            }
        }
        out
    }

    /// A single honest success line assembled from the counts we actually have.
    fn summary_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let vec_pass = self.vectorized();
        let vec_miss = self.not_vectorized();
        if vec_pass + vec_miss > 0 {
            if vec_miss > 0 {
                parts.push(format!("{vec_pass} loop(s) vectorized ({vec_miss} not)"));
            } else {
                parts.push(format!("{vec_pass} loop(s) vectorized"));
            }
        }
        if self.inlined > 0 {
            parts.push(format!("{} call(s) inlined", self.inlined));
        }
        if self.slp_stores > 0 {
            parts.push(format!("{} store group(s) SLP-vectorized", self.slp_stores));
        }
        if parts.is_empty() {
            "no vectorization or inlining opportunities were reported".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// Translate one located raw remark into a record, a summary hit, or a bucket. Pattern-keyed against
/// real LLVM 19 strings; the honesty rule is enforced here (a cost-model decline never becomes an
/// aliasing story). Runtime callees (`align_rt_*`) route to the library/runtime bucket.
fn classify(raw: &RawRemark) -> Class {
    let m = raw.message.as_str();

    // loop-vectorize, PASSED.
    if m.starts_with("vectorized loop") {
        return Class::Record(OptRecord {
            kind: Kind::Vectorized,
            verdict: Verdict::Passed,
            pass: "loop-vectorize".to_string(),
            reason: ReasonCode::None,
            file: raw.file.clone(),
            line: raw.line,
            col: raw.col,
            message: "vectorized — this pipeline runs several elements per instruction".to_string(),
            llvm_detail: raw.raw.clone(),
        });
    }

    // loop-vectorize, MISSED. The specific analysis remark (`loop not vectorized: <reason>`) carries
    // the actionable cause; the bare `loop not vectorized` decline is a redundant duplicate → bucket.
    if let Some(reason_text) = m.strip_prefix("loop not vectorized: ") {
        let (reason, message) = vectorize_miss_reason(reason_text);
        return Class::Record(OptRecord {
            kind: Kind::NotVectorized,
            verdict: Verdict::Missed,
            pass: "loop-vectorize".to_string(),
            reason,
            file: raw.file.clone(),
            line: raw.line,
            col: raw.col,
            message,
            llvm_detail: raw.raw.clone(),
        });
    }
    if m == "loop not vectorized" {
        // Redundant with the specific-reason remark LLVM also emits; do not double-report.
        return Class::Bucketed(Bucket::Other);
    }

    // inline. Success feeds the summary; a miss on a runtime callee is library/runtime noise. (A
    // pipeline-lambda inline-miss → actionable record is deferred: not observed in practice — every
    // Align pipeline lambda inlines — so v1 buckets inline misses rather than key on an unverified
    // pattern, `docs/impl/09-explain-opt.md`.)
    // Order matters: "will not be inlined into …" contains the substring "inlined into", so the
    // decline must be checked before the success.
    if m.contains("will not be inlined") {
        return Class::Bucketed(if is_runtime(m) { Bucket::LibraryRuntime } else { Bucket::Other });
    }
    if m.contains("inlined into") {
        return Class::Summary(SummaryHit::Inlined);
    }

    // slp-vectorize successes feed the summary; the analysis ("not beneficial") is deferred → bucket.
    if m.starts_with("Stores SLP vectorized") {
        return Class::Summary(SummaryHit::SlpStores);
    }

    Class::Bucketed(if is_runtime(m) { Bucket::LibraryRuntime } else { Bucket::Other })
}

/// Whether a remark message is about Align's runtime/std layer (an `align_rt_*` symbol).
fn is_runtime(message: &str) -> bool {
    message.contains("align_rt_")
}

/// Map a loop-vectorize miss reason (the text after `loop not vectorized: `) to a stable reason code
/// and the Align-voice sentence. Honesty rule: only assert what the remark justifies; a vague
/// cost-model decline says only that. Unknown reasons stay `Unspecified` with a neutral message
/// (no fabricated cause).
fn vectorize_miss_reason(reason: &str) -> (ReasonCode, String) {
    // FP reassociation (an ordered floating-point reduction). Empirically observed.
    if reason.contains("reorder floating-point") {
        return (
            ReasonCode::FpReorder,
            "not vectorized — reordering floating-point operations could change the result, so the \
             compiler won't parallelize this reduction by default. Use an integer reduction, or \
             accept the scalar loop here."
                .to_string(),
        );
    }
    // A loop-carried value that is not a recognized reduction (e.g. a running `scan`). Observed.
    if reason.contains("could not be identified as reduction") || reason.contains("reduction") {
        return (
            ReasonCode::ReductionNotRecognized,
            "not vectorized — a value carried between iterations wasn't recognized as a reduction, \
             so the iterations can't run in parallel. If it's a sum/product/min/max, use `reduce` \
             with a recognized combiner (`+`, `*`, `min`, `max`); a genuine running scan can't be \
             widened."
                .to_string(),
        );
    }
    // Aliasing — the vectorizer can't prove two pointers don't overlap.
    if reason.contains("cannot identify array bounds")
        || reason.contains("do not alias")
        || reason.contains("memory operations")
        || reason.contains("Unknown data dependence")
    {
        return (
            ReasonCode::MayAlias,
            "not vectorized — the compiler can't prove the pipeline's source and destination don't \
             overlap. Write the result into a distinct `out` array."
                .to_string(),
        );
    }
    // A call with no vector form (e.g. a non-vectorizable math function).
    if reason.contains("call") {
        return (
            ReasonCode::CallNoVectorForm,
            "not vectorized — the pipeline calls a function with no vector form, so the loop can't \
             be widened. Use a vectorizable operation, or accept the scalar loop here."
                .to_string(),
        );
    }
    // Unknown trip count.
    if reason.contains("number of loop iterations") || reason.contains("loop count") {
        return (
            ReasonCode::UnknownTripCount,
            "not vectorized — the compiler couldn't determine the iteration count, so it couldn't \
             widen the loop."
                .to_string(),
        );
    }
    // Cost model — vague by nature; say only that (never upgrade it into a concrete cause).
    if reason.contains("not beneficial") || reason.contains("cost") {
        return (
            ReasonCode::CostModel,
            "not vectorized — the compiler judged it not worthwhile here (short pipeline or cheap \
             per-element work); nothing to change in the source."
                .to_string(),
        );
    }
    // Anything else: report the decline honestly without inventing a cause.
    (
        ReasonCode::Unspecified,
        format!("not vectorized — the compiler declined to widen this loop ({reason})."),
    )
}

/// `alignc explain-opt <file> [--verbose]` — compile, capture remarks, and print the report. Exit
/// code: `0` = compiled + report produced (missed optimizations are not errors); `1` = compile
/// error / bad args (`docs/impl/09-explain-opt.md`).
pub fn run_explain_opt(path: &str, verbose: bool, target: BuildTarget) -> ExitCode {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("alignc: cannot read '{path}': {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, path, &src);
    if !checked.diags.is_empty() {
        eprint!("{}", format_diagnostics(&sm, &checked.diags));
    }
    if checked.diags.has_errors() {
        return ExitCode::FAILURE;
    }
    let mir = lower_to_mir_located(&checked.hir, &sm);

    // The DIFile that names the module: base the shown file on the input path (its basename is what
    // the remark strings — and thus the report — carry), the directory on its parent.
    let p = PathBuf::from(path);
    let file = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    let directory = p.parent().map(|d| d.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| ".".to_string());
    let debug = DebugInfo { file, directory };

    let remarks = match collect_opt_remarks(&mir, target, &debug) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("alignc: {e}");
            return ExitCode::FAILURE;
        }
    };
    let report = Report::build(&remarks);
    print!("{}", report.render(verbose));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real LLVM remark strings captured from the probe kernels (`docs/impl/09-explain-opt.md`).
    // The translation table is keyed on these; breaking a pattern must drop the actionable line
    // (the mutation-check below). Re-verified at the LLVM 19 → 22 upgrade (2026-07-12): the
    // vectorize/reduction/inline/SLP message patterns are unchanged, and the `explain_opt.rs`
    // integration tests (which drive the REAL LLVM 22 remark stream) pass.

    #[test]
    fn parses_location_and_message() {
        let r = RawRemark::parse("probe.align:2:33: vectorized loop (vectorization width: 4, interleaved count: 1)").unwrap();
        assert_eq!(r.file, "probe.align");
        assert_eq!((r.line, r.col), (2, 33));
        assert!(r.message.starts_with("vectorized loop"));
        // A message that itself contains ": " still splits only at the first colon-space.
        let r2 = RawRemark::parse("f.align:1:1: loop not vectorized: cannot prove it is safe to reorder floating-point operations").unwrap();
        assert_eq!(r2.message, "loop not vectorized: cannot prove it is safe to reorder floating-point operations");
    }

    #[test]
    fn unknown_location_is_suppressed_as_internal() {
        let rep = Report::build(&["<unknown>:0:0: 'align_main' inlined into 'main' with (cost=-14825, threshold=225)".to_string()]);
        assert!(rep.records.is_empty());
        assert_eq!(rep.internal.len(), 1);
    }

    #[test]
    fn vectorized_loop_is_a_passed_record() {
        let rep = Report::build(&["probe.align:2:33: vectorized loop (vectorization width: 4, interleaved count: 1)".to_string()]);
        assert_eq!(rep.vectorized(), 1);
        let rec = &rep.records[0];
        assert_eq!(rec.kind, Kind::Vectorized);
        assert_eq!(rec.verdict, Verdict::Passed);
        assert_eq!(rec.pass, "loop-vectorize");
    }

    #[test]
    fn fp_reorder_miss_is_actionable_with_its_own_reason() {
        let rep = Report::build(&[
            "probe.align:1:33: loop not vectorized: cannot prove it is safe to reorder floating-point operations".to_string(),
        ]);
        assert_eq!(rep.not_vectorized(), 1);
        let rec = &rep.records[0];
        assert_eq!(rec.reason, ReasonCode::FpReorder);
        // Honesty: the FP message must not fabricate an aliasing story.
        assert!(!rec.message.contains("overlap"));
        assert!(rec.message.contains("floating-point"));
        // The default view surfaces it as a `file:line:col:` line.
        let out = rep.render(false);
        assert!(out.contains("probe.align:1:33:"), "actionable line missing:\n{out}");
    }

    #[test]
    fn reduction_miss_maps_to_reduction_reason() {
        let rep = Report::build(&[
            "probe.align:2:40: loop not vectorized: value that could not be identified as reduction is used outside the loop".to_string(),
        ]);
        let rec = &rep.records[0];
        assert_eq!(rec.reason, ReasonCode::ReductionNotRecognized);
        assert!(rec.message.contains("reduce"));
    }

    #[test]
    fn bare_loop_not_vectorized_is_deduped_to_bucket() {
        let rep = Report::build(&["probe.align:2:33: loop not vectorized".to_string()]);
        assert!(rep.records.is_empty(), "bare decline must not add a second actionable line");
        assert_eq!(rep.other.len(), 1);
    }

    #[test]
    fn runtime_inline_miss_goes_to_library_bucket() {
        let rep = Report::build(&[
            "probe.align:6:3: align_rt_print_i64 will not be inlined into align_main because its definition is unavailable".to_string(),
        ]);
        assert!(rep.records.is_empty());
        assert_eq!(rep.library_runtime.len(), 1);
    }

    #[test]
    fn inline_success_and_slp_feed_the_summary_only() {
        let rep = Report::build(&[
            "probe.align:2:33: 'dbl' inlined into 'run' with (cost=-15030, threshold=337) at callsite run:0:33;".to_string(),
            "probe.align:4:8: Stores SLP vectorized with cost -2 and with tree size 2".to_string(),
        ]);
        assert!(rep.records.is_empty());
        assert_eq!(rep.inlined, 1);
        assert_eq!(rep.slp_stores, 1);
        let s = rep.summary_line();
        assert!(s.contains("inlined") && s.contains("SLP"), "summary: {s}");
    }

    /// The full map+sum kernel's real remark set: a vectorized loop + inlines + SLP + unroll noise.
    /// The default report shows the success summary and buckets the rest; no spurious miss line.
    #[test]
    fn map_sum_full_remark_set_renders_a_clean_success() {
        let remarks: Vec<String> = [
            "probe.align:2:33: 'dbl' inlined into 'run' with (cost=-15030, threshold=337) at callsite run:0:33;",
            "probe.align:5:21: align_rt_range_fail will not be inlined into align_main because its definition is unavailable",
            "probe.align:6:9: 'run' inlined into 'align_main' with (cost=-14995, threshold=225) at callsite align_main:2:9;",
            "<unknown>:0:0: 'align_main' inlined into 'main' with (cost=-14825, threshold=225)",
            "probe.align:2:33: the cost-model indicates that interleaving is not beneficial",
            "probe.align:2:33: vectorized loop (vectorization width: 4, interleaved count: 1)",
            "probe.align:4:8: Stores SLP vectorized with cost -2 and with tree size 2",
            "probe.align:2:33: completely unrolled loop with 2 iterations",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let rep = Report::build(&remarks);
        assert_eq!(rep.vectorized(), 1);
        assert_eq!(rep.not_vectorized(), 0);
        let out = rep.render(false);
        // A success line, no "not vectorized" actionable line.
        assert!(out.contains("vectorized"), "{out}");
        assert!(!out.contains("not vectorized"), "no miss expected:\n{out}");
        // The interleave/unroll/GVN noise is bucketed, not dressed as diagnostics.
        assert!(out.contains("other LLVM remarks"), "{out}");
    }
}
