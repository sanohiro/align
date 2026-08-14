//! `align-repl` — an AOT REPL for Align (`docs/impl/22-repl-plan.md`).
//!
//! The REPL is an editor for one growing Align program. Every entry is spliced into the session,
//! the whole session is recompiled through the same `align_driver` calls `alignc build` uses, and
//! the resulting native binary is executed as a subprocess. There is no interpreter, no JIT, and no
//! residual runtime state: earlier effects are reproduced by re-executing them, which is what keeps
//! REPL behavior byte-identical to a production compile.

mod build;
pub mod cmd;
mod echo;
mod entry;

use align_driver::{ArtifactStage, BuildTarget, Profile};
pub use build::RunOutput;
pub use echo::Echo;
pub use entry::{Entry, EntryKind, Region};

/// The duplicate/shadowing diagnostic classes §3.5.1 recognises.
///
/// Derived from the set of creatable entities in the four regions, then verified against the
/// compiler. Two facts differ from a naive expectation and are load-bearing:
///
///   * a `resource` redeclaration reports `duplicate type declaration`, the same class as a struct
///     or a sum — resources do not have their own message;
///   * a duplicate `extern "C"` block is **not** an error at all, so no message can carry it. That
///     is why declaration regions resolve replacement from their own AST names (§3.5) instead of
///     waiting for a diagnostic; only region 4, whose names come from the HIR, needs §3.5.1.
const DUPLICATE_CLASSES: [&str; 5] = [
    "is already bound in this scope chain",
    "duplicate function",
    "duplicate type declaration",
    "duplicate constant",
    "duplicate import",
];

/// The message on a real Align diagnostic error-header line.
///
/// `format_diagnostics` uses `path:line:column: error: message`; unit probes also use the compact
/// `error: message` form. Source excerpts carry a ` | ` gutter and must not become diagnostics just
/// because the user's text contains `error:`.
fn diagnostic_error_message(line: &str) -> Option<&str> {
    if let Some(message) = line.strip_prefix("error:") {
        return Some(message.trim_start());
    }
    if line.contains(" | ") {
        return None;
    }
    let (location, message) = line.rsplit_once(": error:")?;
    let mut parts = location.rsplitn(3, ':');
    let column = parts.next()?;
    let row = parts.next()?;
    let path = parts.next()?;
    if path.is_empty() || row.parse::<u32>().is_err() || column.parse::<u32>().is_err() {
        return None;
    }
    Some(message.trim_start())
}

/// Whether a rendered diagnostic block contains at least one duplicate-class error.
///
/// §3.5.1 attempt 1 asks "is a rebinding what happened here?", where a duplicate plus downstream
/// `undefined name` noise is the normal shape — so **one** duplicate is enough. This is
/// deliberately a different threshold from [`all_errors_are_duplicates`]; see there.
fn has_duplicate_error(rendered: &str) -> bool {
    rendered
        .lines()
        .filter_map(diagnostic_error_message)
        .any(|message| DUPLICATE_CLASSES.iter().any(|class| message.contains(class)))
}

/// Whether **every** error in a rendered block is duplicate-class.
///
/// §3.3 step 4 is choosing between two *readings of the text*, and a single non-duplicate error
/// proves that reading is wrong — `T { a: i64 }`'s statement reading fails with
/// `undefined name: 'i64'`, which is decisive. Collapsing this and [`has_duplicate_error`] into one
/// predicate breaks type redeclaration in one direction and rebinding in the other.
fn all_errors_are_duplicates(rendered: &str) -> bool {
    let mut saw = false;
    for message in rendered.lines().filter_map(diagnostic_error_message) {
        saw = true;
        if !DUPLICATE_CLASSES.iter().any(|class| message.contains(class)) {
            return false;
        }
    }
    saw
}

/// Session configuration. Every field has a default that matches `alignc`'s own.
#[derive(Clone, Debug)]
pub struct Config {
    pub profile: Profile,
    pub rt_lto: bool,
    pub target: BuildTarget,
    pub jobs: usize,
    pub memo_budget_bytes: u64,
    pub time_default_n: u32,
    pub output_cap_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        let jobs = match std::env::var_os("ALIGNC_JOBS") {
            Some(value) => value
                .to_string_lossy()
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|jobs| *jobs >= 1)
                .unwrap_or(0),
            None if cfg!(target_arch = "aarch64") => 1,
            None => std::thread::available_parallelism().map_or(1, |n| n.get()),
        };
        Config {
            profile: Profile::Release,
            // `alignc`'s own default for the optimizing profiles. Measured: disabling it does not
            // make a REPL entry faster, because the uncacheable link step dominates.
            rt_lto: align_driver::default_rt_lto(Profile::Release),
            target: BuildTarget::Baseline,
            jobs,
            memo_budget_bytes: 256 << 20,
            time_default_n: 10,
            output_cap_bytes: 8 << 20,
        }
    }
}

#[derive(Debug)]
pub enum StartupError {
    BackendUnavailable,
    InvalidJobs(String),
    /// `libalign_runtime.a is stale: … run cargo build` — a real failure a library consumer hits
    /// exactly as `alignc` does. Reported once at startup with the driver's own message rather
    /// than once per entry.
    RuntimeArchiveStale(String),
    Stage(std::io::Error),
    FloorBuild(String),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupError::BackendUnavailable => {
                write!(f, "the LLVM backend is unavailable; align-repl cannot compile anything")
            }
            StartupError::InvalidJobs(value) => {
                write!(f, "invalid ALIGNC_JOBS '{value}' (expected a positive integer)")
            }
            StartupError::RuntimeArchiveStale(m) => write!(f, "{m}"),
            StartupError::Stage(e) => write!(f, "cannot create the session staging directory: {e}"),
            StartupError::FloorBuild(m) => write!(f, "cannot build the startup probe program: {m}"),
        }
    }
}

/// What one submitted entry did.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Stage 0: no significant token. The program is neither rebuilt nor re-run, so holding Enter
    /// does not replay every side effect in the session.
    NoOp,
    Applied {
        ordinals: Vec<u32>,
        replaced: Vec<u32>,
        echo: Echo,
        out: RunOutput,
    },
    CompileFailed {
        rendered: String,
        replacing: Vec<u32>,
    },
    /// A region-4 binding collided with a region-2 constant. Nothing is removed: deleting a
    /// `:const` because a `main` binding reused its name would destroy an entry the user never
    /// referred to.
    RegionConflict {
        name: String,
        ordinal: u32,
    },
    RanAndFailed {
        status: std::process::ExitStatus,
        out: RunOutput,
    },
    Command(cmd::CmdResult),
}

/// Whether the REPL is mid-entry (§9.1) or has a complete one.
pub enum Feed {
    NeedMore,
    Ready(Outcome),
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub n: u32,
    pub clamped_from: Option<u32>,
    pub min_ms: f64,
    pub median_ms: f64,
    pub max_ms: f64,
    pub floor_ms: f64,
}

#[derive(Debug)]
pub enum TimeRefusal {
    NoBinary,
    Projected { secs: f64 },
}

#[derive(Debug)]
pub enum SaveError {
    Exists,
    ParentMissing,
    Io(std::io::Error),
}

pub struct Session {
    cfg: Config,
    stage: ArtifactStage,
    exe: std::path::PathBuf,
    entries: Vec<Entry>,
    next_ordinal: u32,
    next_paste: u32,
    baseline: build::OutputBaseline,
    floor_ms: f64,
    have_binary: bool,
    last_run_ms: f64,
    pending: String,
    undo: Vec<UndoStep>,
    hir: Option<Box<align_sema::Program>>,
}

#[derive(Clone)]
struct UndoStep {
    added: Vec<Entry>,
    removed: Vec<(usize, Entry)>,
}

impl UndoStep {
    fn between(before: &[Entry], after: &[Entry]) -> Self {
        let added = after
            .iter()
            .filter(|entry| !before.iter().any(|old| old == *entry))
            .cloned()
            .collect();
        let removed = before
            .iter()
            .enumerate()
            .filter(|(_, entry)| !after.iter().any(|new| new == *entry))
            .map(|(index, entry)| (index, entry.clone()))
            .collect();
        Self { added, removed }
    }

    fn is_active(&self, entries: &[Entry]) -> bool {
        self.added
            .iter()
            .any(|added| entries.iter().any(|entry| entry == added))
    }

    fn reverse(&self, entries: &[Entry]) -> Vec<Entry> {
        let mut restored = entries
            .iter()
            .filter(|entry| !self.added.iter().any(|added| *entry == added))
            .cloned()
            .collect::<Vec<_>>();
        for (index, entry) in &self.removed {
            restored.insert((*index).min(restored.len()), entry.clone());
        }
        restored
    }
}

impl Session {
    pub fn new(cfg: Config) -> Result<Session, StartupError> {
        if cfg.jobs == 0 {
            return Err(StartupError::InvalidJobs(
                std::env::var("ALIGNC_JOBS").unwrap_or_else(|_| "0".to_string()),
            ));
        }
        if !align_driver::backend_available() {
            return Err(StartupError::BackendUnavailable);
        }
        align_driver::memo::set_budget(cfg.memo_budget_bytes);
        let stage = ArtifactStage::temp("align-repl").map_err(StartupError::Stage)?;
        let exe = stage.path().join("session");
        let mut s = Session {
            cfg,
            stage,
            exe,
            entries: Vec::new(),
            next_ordinal: 1,
            next_paste: 1,
            baseline: build::OutputBaseline::default(),
            floor_ms: 0.0,
            have_binary: false,
            last_run_ms: 0.0,
            pending: String::new(),
            undo: Vec::new(),
            hir: None,
        };
        // Establish the host's process-spawn floor from an empty program, so `:time` can report
        // what a sample costs before the user's code runs at all.
        let empty = s.render();
        s.build_current(&empty).map_err(|m| {
            if m.contains("is stale") {
                StartupError::RuntimeArchiveStale(m)
            } else {
                StartupError::FloorBuild(m)
            }
        })?;
        let mut samples = Vec::new();
        for _ in 0..5 {
            match build::run_exe(&s.exe) {
                Ok((_, _, _, ms)) => samples.push(ms),
                Err(e) => return Err(StartupError::FloorBuild(e)),
            }
        }
        samples.sort_by(f64::total_cmp);
        s.floor_ms = samples[samples.len() / 2];
        // The probe executable measures the host floor; it is not a user session binary and must
        // not make `:time` before the first accepted entry look meaningful.
        s.have_binary = false;
        s.hir = Some(Box::new(
            build::checked_hir(&s.session_path(), &empty).map_err(StartupError::FloorBuild)?,
        ));
        Ok(s)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Accumulate one input line, honoring §9.1's continuation rule.
    pub fn feed(&mut self, line: &str) -> Feed {
        if self.pending.is_empty() && line.trim().is_empty() {
            // A blank line at the PRIMARY prompt is stage 0's no-op, not an abandon.
            return Feed::Ready(Outcome::NoOp);
        }
        if !self.pending.is_empty() && line.trim().is_empty() {
            self.pending.clear();
            return Feed::Ready(Outcome::Command(cmd::CmdResult::Message(
                "align-repl: abandoned incomplete entry".to_string(),
            )));
        }
        if !self.pending.is_empty() {
            self.pending.push('\n');
        }
        self.pending.push_str(line);
        if entry::needs_more(&self.pending) {
            return Feed::NeedMore;
        }
        let text = std::mem::take(&mut self.pending);
        Feed::Ready(self.submit(&text))
    }

    /// True while a multi-line entry is still being read.
    pub fn continuing(&self) -> bool {
        !self.pending.is_empty()
    }

    // ---------------------------------------------------------------- rendering

    /// The exact bytes compiled and written by `:save` (§3.2).
    pub fn render(&self) -> String {
        self.render_entries(&self.entries)
    }

    fn render_entries(&self, entries: &[Entry]) -> String {
        let mut out = String::from(
            "// generated by align-repl; every line below is real Align\n\
             // `main` is fixed at `-> Result<(), Error>` so `?` works in every entry\n\
             // every statement re-runs on each entry; external side effects are repeated\n",
        );
        for region in [Region::Import, Region::Const, Region::Decl] {
            for e in entries.iter().filter(|e| e.region == region) {
                out.push_str(&e.emitted);
                out.push('\n');
            }
        }
        out.push_str("fn main() -> Result<(), Error> {\n");
        for e in entries.iter().filter(|e| e.region == Region::Main) {
            for line in e.emitted.lines() {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str("  return Ok(())\n}\n");
        out
    }

    // ---------------------------------------------------------------- submit

    pub fn submit(&mut self, text: &str) -> Outcome {
        let text = text.trim_end();
        // Stage 0.
        match entry::shape_of(text) {
            entry::Shape::Empty => return Outcome::NoOp,
            entry::Shape::KeywordDecl => return self.submit_decl(text),
            entry::Shape::Other => {}
        }
        // Stage 2 (§3.0): the one collision that crosses regions. Refused, never resolved —
        // deleting a `:const` because a `main` binding reused its name would destroy an entry the
        // user never referred to.
        let binding_names = entry::top_level_binding_names(text);
        if let Some(hit) = self
            .entries
            .iter()
            .find(|e| e.region == Region::Const && e.names.iter().any(|n| binding_names.contains(n)))
        {
            let name = hit
                .names
                .iter()
                .find(|n| binding_names.contains(n))
                .cloned()
                .unwrap_or_default();
            return Outcome::RegionConflict {
                name,
                ordinal: hit.ordinal,
            };
        }
        // Steps 2-4.
        let as_file = entry::parse_as_file(text);
        let as_stmt = entry::parses_as_statement(text);
        match (as_file.is_some(), as_stmt) {
            (false, _) => self.submit_main(text),
            (true, false) => self.submit_decl(text),
            (true, true) => {
                // Step 4: both parse. MAIN is evaluated first and its verdict is final when it is
                // clean or duplicate-only, so the procedure is total and order-deterministic.
                match self.try_main(text) {
                    MainTry::Accepted(a) => self.commit(a),
                    MainTry::Rejected {
                        rendered,
                        all_dup,
                        replacing: main_replacing,
                    } => {
                        if all_dup {
                            // Unreachable in practice (a well-formed-but-duplicate statement
                            // reading requires the type to be declared, which makes the
                            // declaration reading a duplicate too), but if it happens MAIN wins.
                            return Outcome::CompileFailed {
                                rendered,
                                replacing: main_replacing,
                            };
                        }
                        match self.try_decl(text) {
                            DeclTry::Accepted(a) => self.commit(a),
                            DeclTry::Rejected {
                                rendered: decl_rendered,
                                all_dup: decl_all_dup,
                                replacing,
                            } => {
                                // A name collision settles the routing on its own: the entry is a
                                // declaration being rewritten, so its diagnostics are the ones the
                                // user needs, together with the ordinal it was replacing.
                                if decl_all_dup || !replacing.is_empty() {
                                    return Outcome::CompileFailed {
                                        rendered: decl_rendered,
                                        replacing,
                                    };
                                }
                                Outcome::CompileFailed {
                                    rendered,
                                    replacing: main_replacing,
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn submit_decl(&mut self, text: &str) -> Outcome {
        match self.try_decl(text) {
            DeclTry::Accepted(a) => self.commit(a),
            DeclTry::Rejected {
                rendered, replacing, ..
            } => Outcome::CompileFailed { rendered, replacing },
        }
    }

    fn submit_main(&mut self, text: &str) -> Outcome {
        match self.try_main(text) {
            MainTry::Accepted(a) => self.commit(a),
            MainTry::Rejected {
                rendered, replacing, ..
            } => Outcome::CompileFailed { rendered, replacing },
        }
    }
}

/// A candidate the session accepted: the new entry list plus what to report.
struct Accepted {
    entries: Vec<Entry>,
    ordinals: Vec<u32>,
    replaced: Vec<u32>,
    echo: Echo,
    source: String,
    next_ordinal: u32,
    next_paste: u32,
    hir: Box<align_sema::Program>,
    diagnostics: String,
}

enum MainTry {
    Accepted(Accepted),
    Rejected {
        rendered: String,
        all_dup: bool,
        replacing: Vec<u32>,
    },
}

enum DeclTry {
    Accepted(Accepted),
    Rejected {
        rendered: String,
        all_dup: bool,
        /// Non-empty when this entry's own AST names matched an existing declaration, i.e. the
        /// entry IS a replacement. Routing is then settled and `submit` must report these
        /// diagnostics — falling back to the statement reading would show the user an unrelated
        /// error about text the REPL already decided was a declaration.
        replacing: Vec<u32>,
    },
}

impl Session {
    /// Candidate ladder for a region-4 entry (§3.4), with §3.5.1's replacement resolution applied
    /// to each candidate in turn.
    fn try_main(&mut self, text: &str) -> MainTry {
        let mut stmt_rendered = String::new();
        let mut stmt_all_dup = false;
        let mut stmt_replacing = Vec::new();
        for kind in echo::CANDIDATE_ORDER {
            let emitted = Entry::emit_for(kind, text);
            // Parse-only prefilter: `print(x := 1)` is not an expression, so candidate P is never
            // formed for a statement entry and costs nothing.
            if !entry::parses_as_statement(&emitted) {
                continue;
            }
            if kind == EntryKind::ResultBound && !echo::only_unhandled_result(&stmt_rendered) {
                // Case 3 answers exactly one failure of candidate S: the hard `unhandled Result`
                // rule. Any other failure is a user error and is reported from S.
                break;
            }
            match self.resolve(Region::Main, kind, text, &emitted) {
                Resolved::Ok {
                    entries,
                    ordinals,
                    replaced,
                    source,
                    next_ordinal,
                    hir,
                    diagnostics,
                } => {
                    let index = entries
                        .iter()
                        .filter(|e| e.region == Region::Main)
                        .position(|e| e.ordinal == ordinals[0])
                        .unwrap_or(0);
                    let echo = self.echo_for(kind, &source, index);
                    return MainTry::Accepted(Accepted {
                        entries,
                        ordinals,
                        replaced,
                        echo,
                        source,
                        next_ordinal,
                        next_paste: self.next_paste,
                        hir,
                        diagnostics,
                    });
                }
                Resolved::Err { rendered, replacing } => {
                    if kind == EntryKind::Statement {
                        stmt_all_dup = all_errors_are_duplicates(&rendered);
                        stmt_rendered = rendered;
                        stmt_replacing = replacing;
                    } else if stmt_rendered.is_empty() {
                        stmt_rendered = rendered;
                    }
                }
            }
        }
        MainTry::Rejected {
            rendered: stmt_rendered,
            all_dup: stmt_all_dup,
            replacing: stmt_replacing,
        }
    }

    /// A declaration entry. Splitting a mixed paste happens here (§3.7.1).
    fn try_decl(&mut self, text: &str) -> DeclTry {
        let Some(file) = entry::parse_as_file(text) else {
            return DeclTry::Rejected {
                rendered: String::from("align-repl: this entry does not parse as a declaration\n"),
                all_dup: false,
                replacing: Vec::new(),
            };
        };
        let mut proposed = self.entries.clone();
        let mut ordinals = Vec::new();
        let mut replaced = Vec::new();
        let mut ordinal = self.next_ordinal;
        let group = if file.imports.len() + file.items.len() > 1 {
            Some(self.next_paste)
        } else {
            None
        };

        for path in &file.imports {
            let rendered = format!("import {}", entry::import_path(path));
            // Deduplicated by rendered dotted path: a re-import is a hard `duplicate import`
            // error, so without this a second `import core.json` would be unusable.
            if proposed
                .iter()
                .any(|e| e.region == Region::Import && e.emitted == rendered)
            {
                continue;
            }
            proposed.push(Entry {
                ordinal,
                region: Region::Import,
                kind: EntryKind::Import,
                text: rendered.clone(),
                emitted: rendered,
                names: Vec::new(),
                paste_group: group,
            });
            ordinals.push(ordinal);
            ordinal += 1;
        }

        for item in &file.items {
            let names = entry::item_names(item);
            // Declarations resolve replacement from their OWN AST names, before compiling. Region
            // 4 cannot do this (its names come from the HIR), which is what §3.5.1 exists for; a
            // declaration can, and must — a duplicate `extern "C"` block is not a compiler error
            // at all, so waiting for a diagnostic would silently accumulate duplicate blocks.
            let hit: Vec<u32> = self
                .entries
                .iter()
                .filter(|e| {
                    e.region == Region::Decl
                        && !replaced.contains(&e.ordinal)
                        && e.names.iter().any(|n| names.contains(n))
                })
                .map(|e| e.ordinal)
                .collect();
            let at = hit
                .iter()
                .copied()
                .min()
                .and_then(|lowest| proposed.iter().position(|e| e.ordinal == lowest));
            let text_owned = entry::item_text(text, item).to_string();
            let entry_ordinal = match at {
                Some(_) => {
                    let keep = hit.iter().copied().min().unwrap_or(ordinal);
                    proposed.retain(|e| e.ordinal == keep || !hit.contains(&e.ordinal));
                    replaced.extend(hit.iter().copied());
                    keep
                }
                None => {
                    let o = ordinal;
                    ordinal += 1;
                    o
                }
            };
            let new = Entry {
                ordinal: entry_ordinal,
                region: Region::Decl,
                kind: EntryKind::Decl,
                text: text_owned.clone(),
                emitted: text_owned,
                names,
                paste_group: group,
            };
            if at.is_some() {
                if let Some(index) = proposed.iter().position(|e| e.ordinal == entry_ordinal) {
                    proposed[index] = new;
                } else {
                    proposed.push(new);
                }
            } else {
                proposed.push(new);
            }
            ordinals.push(entry_ordinal);
        }

        let source = self.render_entries(&proposed);
        let checked = build::check_candidate(&self.session_path(), &source);
        if checked.had_errors {
            return DeclTry::Rejected {
                all_dup: all_errors_are_duplicates(&checked.rendered),
                rendered: checked.rendered,
                replacing: replaced,
            };
        }
        let hir = match build::checked_hir(&self.session_path(), &source) {
            Ok(hir) => hir,
            Err(rendered) => {
                return DeclTry::Rejected {
                    all_dup: all_errors_are_duplicates(&rendered),
                    rendered,
                    replacing: replaced,
                };
            }
        };
        let hir_names = declaration_hir_names(&hir);
        let previous_hir_names = self.hir.as_deref().map(declaration_hir_names).unwrap_or_default();
        for candidate in proposed
            .iter_mut()
            .filter(|entry| entry.region == Region::Decl && ordinals.contains(&entry.ordinal))
        {
            let is_replacement = replaced.contains(&candidate.ordinal);
            candidate.names.retain(|name| {
                let present = hir_names.iter().filter(|found| *found == name).count();
                let previous = previous_hir_names.iter().filter(|found| *found == name).count();
                present > previous || (is_replacement && present > 0)
            });
        }
        DeclTry::Accepted(Accepted {
            entries: proposed,
            ordinals,
            replaced,
            echo: Echo::None,
            source,
            next_ordinal: ordinal,
            next_paste: self.next_paste + u32::from(group.is_some()),
            hir: Box::new(hir),
            diagnostics: checked.rendered,
        })
    }

    /// §3.5.1: append, and on a duplicate-class failure retry as an in-place replacement.
    fn resolve(&mut self, region: Region, kind: EntryKind, text: &str, emitted: &str) -> Resolved {
        // Stage 2 (§3.0): a region-4 binding may not take a region-2 constant's name.
        let top_level_names = entry::top_level_binding_names(text);
        let candidate = |entries: &[Entry], session: &Session| -> String { session.render_entries(entries) };

        let mut appended = self.entries.clone();
        appended.push(Entry {
            ordinal: self.next_ordinal,
            region,
            kind,
            text: text.to_string(),
            emitted: emitted.to_string(),
            names: Vec::new(),
            paste_group: None,
        });
        let source = candidate(&appended, self);
        let first = build::check_candidate(&self.session_path(), &source);
        let first_hir = build::check_hir(&self.session_path(), &source);
        let names = self.main_name_delta(&first_hir.hir, &top_level_names);
        if let Some(entry) = appended.last_mut() {
            entry.names = names.clone();
        }
        if !first.had_errors {
            if first_hir.had_errors {
                return Resolved::Err {
                    rendered: first_hir.rendered,
                    replacing: Vec::new(),
                };
            }
            let ordinal = self.next_ordinal;
            return Resolved::Ok {
                entries: appended,
                ordinals: vec![ordinal],
                replaced: Vec::new(),
                source,
                next_ordinal: ordinal + 1,
                hir: Box::new(first_hir.hir),
                diagnostics: first.rendered,
            };
        }
        // Attempt 2 fires on AT LEAST ONE duplicate-class error: a rebinding routinely produces a
        // duplicate plus downstream `undefined name` noise.
        if !has_duplicate_error(&first.rendered) {
            return Resolved::Err {
                rendered: first.rendered,
                replacing: Vec::new(),
            };
        }
        let hit: Vec<u32> = self
            .entries
            .iter()
            .filter(|e| e.region == region && e.names.iter().any(|n| names.contains(n)))
            .map(|e| e.ordinal)
            .collect();
        if hit.is_empty() {
            // The duplicate came from something the REPL does not own.
            return Resolved::Err {
                rendered: first.rendered,
                replacing: Vec::new(),
            };
        }
        let Some(lowest) = hit.iter().copied().min() else {
            return Resolved::Err {
                rendered: first.rendered,
                replacing: Vec::new(),
            };
        };
        let Some(index) = self.entries.iter().position(|e| e.ordinal == lowest) else {
            return Resolved::Err {
                rendered: first.rendered,
                replacing: Vec::new(),
            };
        };
        let mut replaced_entries = self.entries.clone();
        replaced_entries.retain(|e| e.ordinal == lowest || !hit.contains(&e.ordinal));
        let index = index.min(replaced_entries.len().saturating_sub(1));
        replaced_entries[index] = Entry {
            ordinal: lowest,
            region,
            kind,
            text: text.to_string(),
            emitted: emitted.to_string(),
            names,
            paste_group: None,
        };
        let source = candidate(&replaced_entries, self);
        let second = build::check_candidate(&self.session_path(), &source);
        if second.had_errors {
            return Resolved::Err {
                rendered: second.rendered,
                replacing: hit,
            };
        }
        let second_hir = build::check_hir(&self.session_path(), &source);
        if second_hir.had_errors {
            return Resolved::Err {
                rendered: second_hir.rendered,
                replacing: hit,
            };
        }
        Resolved::Ok {
            entries: replaced_entries,
            ordinals: vec![lowest],
            replaced: hit,
            source,
            next_ordinal: self.next_ordinal,
            hir: Box::new(second_hir.hir),
            diagnostics: second.rendered,
        }
    }

    fn main_name_delta(&self, candidate: &align_sema::Program, top_level: &[String]) -> Vec<String> {
        let current = self
            .hir
            .as_deref()
            .and_then(|program| program.fns.iter().find(|function| function.name == "main"));
        let next = candidate.fns.iter().find(|function| function.name == "main");
        let mut names = Vec::new();
        for name in top_level {
            let old_count = current.map_or(0, |function| {
                function.locals.iter().filter(|local| local.name == *name).count()
            });
            let new_count = next.map_or(0, |function| {
                function.locals.iter().filter(|local| local.name == *name).count()
            });
            if new_count > old_count && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    /// What to tell the user about the accepted candidate's value. `index` is the entry's position
    /// among region-4 entries, which is its statement index in `main`'s body.
    fn echo_for(&self, kind: EntryKind, source: &str, index: usize) -> Echo {
        match kind {
            EntryKind::Printed => Echo::Printed,
            EntryKind::ResultBound => Echo::ResultBound {
                rendered: self
                    .render_result_bound_type(source, index)
                    .unwrap_or_else(|| "Result".into()),
            },
            EntryKind::Statement => match self.render_entry_type(source, index) {
                Some(rendered) => Echo::TypeOnly { rendered },
                None => Echo::None,
            },
            EntryKind::Import | EntryKind::Decl | EntryKind::Const => Echo::None,
        }
    }

    fn render_entry_type(&self, source: &str, index: usize) -> Option<String> {
        // Reached only when a type NAME is needed: a printable value never gets here, because
        // `print(E)` checking clean is already the whole answer.
        let hir = build::checked_hir(&self.session_path(), source).ok()?;
        echo::entry_type(&hir, index)
    }

    fn render_result_bound_type(&self, source: &str, index: usize) -> Option<String> {
        let hir = build::checked_hir(&self.session_path(), source).ok()?;
        echo::result_bound_type(&hir, index)
    }

    fn session_path(&self) -> std::path::PathBuf {
        self.stage.path().join("session.align")
    }

    /// Build the accepted candidate and run it. Compilation is the transaction boundary: a failure
    /// here leaves the session byte-identical to what it was.
    fn commit(&mut self, accepted: Accepted) -> Outcome {
        if let Err(rendered) = self.build_current(&accepted.source) {
            return Outcome::CompileFailed {
                rendered,
                replacing: accepted.replaced,
            };
        }
        let undo = UndoStep::between(&self.entries, &accepted.entries);
        self.entries = accepted.entries;
        self.next_ordinal = accepted.next_ordinal;
        self.next_paste = accepted.next_paste;
        self.hir = Some(accepted.hir);
        if !undo.added.is_empty() {
            self.undo.push(undo);
        }
        self.have_binary = true;
        let mut outcome = self.run_and_report(accepted.ordinals, accepted.replaced, accepted.echo);
        if !accepted.diagnostics.is_empty() {
            match &mut outcome {
                Outcome::Applied { out, .. } | Outcome::RanAndFailed { out, .. } => {
                    out.stderr_shown.insert_str(0, &accepted.diagnostics);
                }
                _ => {}
            }
        }
        outcome
    }

    fn build_current(&self, source: &str) -> Result<(), String> {
        build::build_exe(
            &self.stage,
            source,
            &self.exe,
            &self.cfg.target,
            self.cfg.profile,
            self.cfg.rt_lto,
            self.cfg.jobs,
        )
    }

    fn run_and_report(&mut self, ordinals: Vec<u32>, replaced: Vec<u32>, echo: Echo) -> Outcome {
        match build::run_exe(&self.exe) {
            Ok((out, err, status, ms)) => {
                self.last_run_ms = ms;
                let shown = self.baseline.absorb(&out, &err, self.cfg.output_cap_bytes);
                if status.success() {
                    Outcome::Applied {
                        ordinals,
                        replaced,
                        echo,
                        out: shown,
                    }
                } else {
                    // Execution is NOT the transaction boundary: the program is valid Align, it
                    // just aborts. The entry stays; `:undo` / `:drop` are the escape hatches.
                    Outcome::RanAndFailed { status, out: shown }
                }
            }
            Err(rendered) => Outcome::CompileFailed {
                rendered,
                replacing: replaced,
            },
        }
    }
}

impl Session {
    /// `:const NAME := expr` — a region-2 top-level constant, the only form a `fn` entry can
    /// reference. Replacement is by AST name, like any other declaration.
    pub fn add_const(&mut self, text: &str) -> Outcome {
        let Some(file) = entry::parse_as_file(text) else {
            return Outcome::CompileFailed {
                rendered: String::from("align-repl: `:const` takes a declaration, e.g. `:const WIDTH := 6`\n"),
                replacing: Vec::new(),
            };
        };
        if !matches!(file.items.as_slice(), [align_ast::Item::Const(_)]) {
            return Outcome::CompileFailed {
                rendered: String::from("align-repl: `:const` takes exactly one `NAME := expr`\n"),
                replacing: Vec::new(),
            };
        }
        let names = entry::declared_names(&file);
        // The mirror of the stage-2 check: a constant may not take a live `main` binding's name.
        if let Some(hit) = self
            .entries
            .iter()
            .find(|e| e.region == Region::Main && e.names.iter().any(|n| names.contains(n)))
        {
            let name = hit
                .names
                .iter()
                .find(|n| names.contains(n))
                .cloned()
                .unwrap_or_default();
            return Outcome::RegionConflict {
                name,
                ordinal: hit.ordinal,
            };
        }
        let mut proposed = self.entries.clone();
        let hit: Vec<u32> = proposed
            .iter()
            .filter(|e| e.region == Region::Const && e.names.iter().any(|n| names.contains(n)))
            .map(|e| e.ordinal)
            .collect();
        let ordinal = hit.iter().copied().min().unwrap_or(self.next_ordinal);
        let new = Entry {
            ordinal,
            region: Region::Const,
            kind: EntryKind::Const,
            text: text.to_string(),
            emitted: text.to_string(),
            names,
            paste_group: None,
        };
        match proposed.iter().position(|e| e.ordinal == ordinal) {
            Some(index) => proposed[index] = new,
            None => proposed.push(new),
        }
        let source = self.render_entries(&proposed);
        let checked = build::check_candidate(&self.session_path(), &source);
        if checked.had_errors {
            return Outcome::CompileFailed {
                rendered: checked.rendered,
                replacing: hit,
            };
        }
        let hir = match build::checked_hir(&self.session_path(), &source) {
            Ok(hir) => hir,
            Err(rendered) => {
                return Outcome::CompileFailed {
                    rendered,
                    replacing: hit,
                };
            }
        };
        let next_ordinal = self.next_ordinal + u32::from(hit.is_empty());
        self.commit(Accepted {
            entries: proposed,
            ordinals: vec![ordinal],
            replaced: hit,
            echo: Echo::None,
            source,
            next_ordinal,
            next_paste: self.next_paste,
            hir: Box::new(hir),
            diagnostics: checked.rendered,
        })
    }

    /// `:undo` — remove the most recent entry, or every entry of the paste that created it.
    pub fn undo(&mut self) -> Outcome {
        while let Some(step) = self.undo.pop() {
            if !step.is_active(&self.entries) {
                continue;
            }
            let proposed = step.reverse(&self.entries);
            let outcome = self.rebuild_after_removal(proposed, false);
            if matches!(outcome, Outcome::CompileFailed { .. }) {
                self.undo.push(step);
            }
            return outcome;
        }
        Outcome::Command(cmd::CmdResult::Message("align-repl: nothing to undo".into()))
    }

    /// `:drop N` — remove one entry by ordinal.
    pub fn drop_entry(&mut self, ordinal: u32) -> Outcome {
        if !self.entries.iter().any(|e| e.ordinal == ordinal) {
            return Outcome::Command(cmd::CmdResult::Message(format!("align-repl: no entry {ordinal}")));
        }
        let proposed: Vec<Entry> = self.entries.iter().filter(|e| e.ordinal != ordinal).cloned().collect();
        self.rebuild_after_removal(proposed, false)
    }

    /// `:clear` — drop every entry. The ordinal counter keeps its value: ordinals are never reused.
    pub fn clear(&mut self) -> Outcome {
        let outcome = self.rebuild_after_removal(Vec::new(), true);
        if self.entries.is_empty() {
            self.undo.clear();
        }
        outcome
    }

    fn rebuild_after_removal(&mut self, proposed: Vec<Entry>, clear_poison: bool) -> Outcome {
        let source = self.render_entries(&proposed);
        if let Err(rendered) = self.build_current(&source) {
            return Outcome::CompileFailed {
                rendered,
                replacing: Vec::new(),
            };
        }
        let hir = match build::checked_hir(&self.session_path(), &source) {
            Ok(hir) => hir,
            Err(rendered) => {
                return Outcome::CompileFailed {
                    rendered,
                    replacing: Vec::new(),
                };
            }
        };
        self.entries = proposed;
        self.hir = Some(Box::new(hir));
        self.have_binary = true;
        // The program's history no longer matches what the user has seen, so the next run prints
        // in full rather than as a suffix of a baseline that no longer applies.
        if clear_poison {
            self.baseline.clear();
        } else {
            self.baseline.reset();
        }
        self.run_and_report(Vec::new(), Vec::new(), Echo::None)
    }

    /// `:type EXPR` — a throwaway candidate that is never spliced and never executed.
    pub fn type_of(&self, expr: &str) -> Result<String, String> {
        let mut proposed = self.entries.clone();
        proposed.push(Entry {
            ordinal: u32::MAX,
            region: Region::Main,
            kind: EntryKind::Statement,
            text: expr.to_string(),
            emitted: expr.to_string(),
            names: Vec::new(),
            paste_group: None,
        });
        let source = self.render_entries(&proposed);
        let index = proposed.iter().filter(|e| e.region == Region::Main).count() - 1;
        let checked = build::check_hir(&self.session_path(), &source);
        if !checked.had_errors {
            return echo::entry_type(&checked.hir, index)
                .ok_or_else(|| String::from("align-repl: that entry has no value to type\n"));
        }
        if !echo::only_unhandled_result(&checked.rendered) {
            return Err(checked.rendered);
        }

        // A fallible expression cannot legally stand alone, but `:type` must not execute or retain
        // it. The same disclosed case-D wrapper gives sema a legal throwaway statement from which
        // the complete `Result<T, E>` initializer type can be read.
        if let Some(last) = proposed.last_mut() {
            last.kind = EntryKind::ResultBound;
            last.emitted = Entry::emit_for(EntryKind::ResultBound, expr);
        }
        let source = self.render_entries(&proposed);
        let hir = build::checked_hir(&self.session_path(), &source)?;
        echo::result_bound_type(&hir, index)
            .ok_or_else(|| String::from("align-repl: that entry has no value to type\n"))
    }

    /// `:list` — the generated program with §5 line numbers and an ordinal gutter.
    pub fn listing(&self) -> String {
        let source = self.render();
        let mut owner = vec![None; source.lines().count()];
        let mut line = 3usize; // generated header
        for region in [Region::Import, Region::Const, Region::Decl] {
            for entry in self.entries.iter().filter(|entry| entry.region == region) {
                for _ in entry.emitted.lines() {
                    owner[line] = Some((entry.ordinal, region));
                    line += 1;
                }
            }
        }
        line += 1; // main signature
        for entry in self.entries.iter().filter(|entry| entry.region == Region::Main) {
            for _ in entry.emitted.lines() {
                owner[line] = Some((entry.ordinal, Region::Main));
                line += 1;
            }
        }
        let mut out = String::new();
        for (index, source_line) in source.lines().enumerate() {
            let (ordinal, region) = owner[index].map_or((String::new(), ""), |(ordinal, region)| {
                let label = match region {
                    Region::Import => "import",
                    Region::Const => "const",
                    Region::Decl => "decl",
                    Region::Main => "main",
                };
                (ordinal.to_string(), label)
            });
            out.push_str(&format!(
                "{:>4} {:>4} {:>6} | {}\n",
                index + 1,
                ordinal,
                region,
                source_line
            ));
        }
        out
    }

    /// `:save` — write the exact program that was compiled, with the header that states what a
    /// reader has to know about it.
    pub fn save(&self, path: &std::path::Path, force: bool) -> Result<(), SaveError> {
        if !force && path.exists() {
            return Err(SaveError::Exists);
        }
        // Resolved against the process cwd; no `~` or variable expansion, because there is no
        // shell between the user and this call.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.is_dir()
        {
            return Err(SaveError::ParentMissing);
        }
        std::fs::write(path, self.render()).map_err(SaveError::Io)
    }

    /// `:time` — sample the already-built binary. Never recompiles.
    pub fn time(&mut self, n: u32, force: bool) -> Result<Timing, TimeRefusal> {
        if !self.have_binary {
            return Err(TimeRefusal::NoBinary);
        }
        let requested = if n == 0 { self.cfg.time_default_n } else { n };
        let clamped = requested.clamp(1, 1000);
        let projected = self.last_run_ms * f64::from(clamped) / 1000.0;
        if !force && projected > 10.0 {
            return Err(TimeRefusal::Projected { secs: projected });
        }
        let mut samples = Vec::with_capacity(usize::try_from(clamped).unwrap_or(1000));
        for _ in 0..clamped {
            match build::run_exe(&self.exe) {
                Ok((_, _, _, ms)) => samples.push(ms),
                Err(_) => return Err(TimeRefusal::NoBinary),
            }
        }
        samples.sort_by(f64::total_cmp);
        Ok(Timing {
            n: clamped,
            clamped_from: (clamped != requested).then_some(requested),
            min_ms: samples[0],
            median_ms: samples[samples.len() / 2],
            max_ms: samples[samples.len() - 1],
            floor_ms: self.floor_ms,
        })
    }

    /// `:out` — the last run's full captured output.
    pub fn last_output(&self) -> Option<String> {
        self.baseline.last_full().map(|(o, e)| format!("{o}{e}"))
    }

    /// Whether the last run was printed but could not be retained within the configured bound.
    pub fn last_output_was_truncated(&self) -> bool {
        self.baseline.last_was_truncated()
    }
}

enum Resolved {
    Ok {
        entries: Vec<Entry>,
        ordinals: Vec<u32>,
        replaced: Vec<u32>,
        source: String,
        next_ordinal: u32,
        hir: Box<align_sema::Program>,
        diagnostics: String,
    },
    Err {
        rendered: String,
        replacing: Vec<u32>,
    },
}

fn declaration_hir_names(program: &align_sema::Program) -> Vec<String> {
    let mut names = program
        .fns
        .iter()
        .filter(|function| !function.name.contains("$lambda"))
        .map(|function| function.name.clone())
        .collect::<Vec<_>>();
    names.extend(program.structs.iter().map(|item| item.source_name.clone()));
    names.extend(program.enums.iter().map(|item| item.source_name.clone()));
    names.extend(program.resources.iter().map(|item| item.source_name.clone()));
    names.extend(program.externs.iter().map(|item| item.name.clone()));
    names
}

#[cfg(test)]
mod diagnostic_class_tests {
    use super::{all_errors_are_duplicates, has_duplicate_error};

    #[test]
    fn duplicate_thresholds_ignore_source_excerpts() {
        let duplicate_only = [
            "error: `x` is already bound in this scope chain",
            "error: duplicate function 'f' in module 'main'",
            "error: duplicate type declaration: 'T' in module 'main'",
            "error: duplicate constant `K` in module `main`",
            "error: duplicate import `core.math`",
        ]
        .join("\n");
        assert!(has_duplicate_error(&duplicate_only));
        assert!(all_errors_are_duplicates(&duplicate_only));
        let mixed = format!("{duplicate_only}\nerror: undefined name: 'missing'");
        assert!(has_duplicate_error(&mixed));
        assert!(!all_errors_are_duplicates(&mixed));
        let source_excerpt = format!("{duplicate_only}\n  1 | print(\"error: not a diagnostic header\")");
        assert!(all_errors_are_duplicates(&source_excerpt));
        assert!(all_errors_are_duplicates(
            "/tmp/session.align:7:3: error: duplicate function 'f' in module 'main'"
        ));
    }
}
