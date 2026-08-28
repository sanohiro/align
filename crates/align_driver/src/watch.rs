//! Foreground `alignc build --watch` controller.

use super::watch_native::{NativeHandle, NativeWatchErrorKind, NativeWatcher};
use super::{link_lib_union, stem};
use align_driver::{
    ArtifactStage, BuildInputSet, BuildSourceError, BuildTarget, CacheContext, FinalBuildInputSet,
    LinkOutputSink, LinkOutputStream, LinkStopSignal, ObservedBuildAttempt, ObservedPerUnitBuild,
    PgoMode, Profile, UnitReuse, WatchRepairDependency, finalize_watch_inputs,
    merge_observed_build_inputs, snapshot_watch_repair,
};
use align_span::SourceMap;
use align_watch::{
    MonitorBaseline, MonitorRefresh, WatchRegistration, monitor_baseline,
    monitor_watch_registrations, refresh_monitor_baseline,
};
use std::io::{self, Write};
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::{Duration, Instant};

const AUDIT_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_QUIET: Duration = Duration::from_millis(50);
const EVENT_MAX: Duration = Duration::from_millis(250);
const RECORD_CHUNK: usize = 4_096;
const MAX_RECORD_FRAME: usize = 12_331;
const SIGNAL_MASK: u16 = 0b111;
const FATAL_SHIFT: u16 = 3;
const FATAL_MASK: u16 = 0b111 << FATAL_SHIFT;

static SIGNAL_OWNER: AtomicBool = AtomicBool::new(false);

pub(super) struct WakeState {
    control: AtomicU16,
    dirty: AtomicBool,
    uncertain: AtomicBool,
    read_fd: RawFd,
    write_fd: RawFd,
}

impl WakeState {
    fn wake(&self) {
        let byte = [1u8];
        loop {
            // SAFETY: the process-lifetime owner keeps `write_fd` open, and `byte` is a live
            // one-byte buffer. `write` is async-signal-safe.
            let result = unsafe { libc::write(self.write_fd, byte.as_ptr().cast(), 1) };
            if result == 1 {
                return;
            }
            let error = last_errno();
            if error == libc::EINTR {
                continue;
            }
            if error == libc::EAGAIN {
                return;
            }
            return;
        }
    }

    fn set_signal(&self, signal: u16) {
        let mut current = self.control.load(Ordering::Acquire);
        loop {
            if current & SIGNAL_MASK != 0 {
                break;
            }
            let next = (current & !SIGNAL_MASK) | signal;
            match self.control.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
        self.wake();
    }

    pub(super) fn set_fatal(&self, class: u16) {
        let mut current = self.control.load(Ordering::Acquire);
        loop {
            if current & FATAL_MASK != 0 {
                break;
            }
            let next = (current & !FATAL_MASK) | (class << FATAL_SHIFT);
            match self.control.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
        self.wake();
    }

    pub(super) fn set_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
        self.wake();
    }

    pub(super) fn set_uncertain(&self) {
        self.uncertain.store(true, Ordering::Release);
        self.wake();
    }
}

#[cfg(target_os = "linux")]
fn last_errno() -> i32 {
    // SAFETY: reading the calling thread's errno location has no side effect and is
    // async-signal-safe.
    unsafe { *libc::__errno_location() }
}

#[cfg(target_os = "macos")]
fn last_errno() -> i32 {
    // SAFETY: reading the calling thread's errno location has no side effect and is
    // async-signal-safe.
    unsafe { *libc::__error() }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_watch_build(
    path: &str,
    target: BuildTarget,
    profile: Profile,
    rt_lto: bool,
    thin_lto: bool,
    pgo: &PgoMode,
    jobs: usize,
    cache_stats: bool,
) -> ExitCode {
    let mut transcript = Transcript::stderr();
    let input = match align_watch::absolute_lexical(Path::new(path)) {
        Ok(path) => path,
        Err(error) => {
            return terminal_error(&mut transcript, format!("initialize: {error}"));
        }
    };
    let output = match std::env::current_dir() {
        Ok(cwd) => cwd.join(stem(path)),
        Err(error) => {
            return terminal_error(&mut transcript, format!("initialize: {error}"));
        }
    };
    if let Err(error) = snapshot_watch_repair(&output) {
        return terminal_error(&mut transcript, format!("initialize: {error}"));
    }
    let wake = match install_wake_and_signals() {
        Ok(wake) => wake,
        Err(error) => {
            return terminal_error(&mut transcript, format!("initialize: {error}"));
        }
    };
    if let Some(outcome) = selected_control(&wake, None) {
        return finish_terminal(None, &mut transcript, outcome);
    }
    let watcher = match NativeWatcher::new(Arc::clone(&wake)) {
        Ok(watcher) => watcher,
        Err(error) => {
            return finish_terminal(
                None,
                &mut transcript,
                TerminalOutcome::Error(format!("initialize: {error}")),
            );
        }
    };
    let mut watcher = Some(watcher);
    if let Some(outcome) = selected_control(&wake, None) {
        return finish_terminal(watcher.take(), &mut transcript, outcome);
    }

    let mut revision = 1u64;
    let mut last_success: Option<FinalBuildInputSet> = None;
    let mut installed = InstalledWatches::default();
    'revisions: loop {
        if let Some(outcome) = selected_control(&wake, None) {
            return finish_terminal(watcher.take(), &mut transcript, outcome);
        }
        if let Err(error) = transcript.text_record(
            "started",
            format!("alignc: watch: revision {revision} started").as_bytes(),
        ) {
            return finish_transcript_failure(watcher.take(), error);
        }
        let attempt = match build_revision(
            &input,
            &output,
            target.clone(),
            profile,
            rt_lto,
            thin_lto,
            pgo,
            jobs,
            cache_stats,
            &wake,
            &mut transcript,
        ) {
            Ok(attempt) => attempt,
            Err(error)
                if wake.control.load(Ordering::Acquire) & SIGNAL_MASK != 0
                    && error.starts_with("child stopped by ") =>
            {
                return finish_terminal(
                    watcher.take(),
                    &mut transcript,
                    TerminalOutcome::Stop(wake.control.load(Ordering::Acquire)),
                );
            }
            Err(error) => {
                return finish_terminal(
                    watcher.take(),
                    &mut transcript,
                    TerminalOutcome::Error(error),
                );
            }
        };
        let RevisionResult {
            success,
            inputs,
            repair,
        } = attempt;
        let retry_after_install = inputs.changed_during_attempt();
        if let Err(error) = transcript.marker(revision, success) {
            return finish_transcript_failure(watcher.take(), error);
        }
        if let Some(outcome) = selected_control(&wake, None) {
            return finish_terminal(watcher.take(), &mut transcript, outcome);
        }
        let failed_inputs;
        let retained_states;
        let (baseline_inputs, retained) = if success {
            (&*last_success.insert(inputs), None)
        } else {
            failed_inputs = inputs;
            retained_states = last_success
                .as_ref()
                .map(|previous| {
                    previous
                        .inputs()
                        .iter()
                        .filter(|prior| {
                            !failed_inputs
                                .inputs()
                                .iter()
                                .any(|current| current.path() == prior.path())
                        })
                        .map(|prior| prior.path().to_path_buf())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (&failed_inputs, Some(retained_states.as_slice()))
        };
        let mut baseline = match monitor_baseline(baseline_inputs, retained.unwrap_or(&[])) {
            Ok(baseline) => baseline,
            Err(error) => {
                return finish_terminal(
                    watcher.take(),
                    &mut transcript,
                    TerminalOutcome::Error(format!("snapshot '{}': {error}", encode_path(&input))),
                );
            }
        };
        let transition = match reinstall_watches(
            watcher.as_mut().expect("active watcher"),
            &mut installed,
            &mut baseline,
            repair.as_ref(),
            &wake,
        ) {
            Ok(transition) => transition,
            Err(error) => {
                let outcome = selected_control(&wake, Some(error)).unwrap_or(
                    TerminalOutcome::Error("watch transition failed".to_string()),
                );
                return finish_terminal(watcher.take(), &mut transcript, outcome);
            }
        };
        if transition == WatchTransition::Changed || retry_after_install {
            revision = match revision.checked_add(1) {
                Some(revision) => revision,
                None => {
                    return finish_terminal(
                        watcher.take(),
                        &mut transcript,
                        TerminalOutcome::Error("revision counter exhausted".to_string()),
                    );
                }
            };
            continue 'revisions;
        }

        let mut next_audit = Instant::now() + AUDIT_INTERVAL;
        let mut transition_retry = (transition == WatchTransition::Retry)
            .then(|| Instant::now() + Duration::from_millis(50));
        loop {
            if let Some(outcome) = selected_control(&wake, None) {
                return finish_terminal(watcher.take(), &mut transcript, outcome);
            }
            let timeout = transition_retry
                .map(|deadline| deadline.min(next_audit))
                .unwrap_or(next_audit)
                .saturating_duration_since(Instant::now());
            let readable = match wait_wake(wake.read_fd, timeout) {
                Ok(readable) => readable,
                Err(error) => {
                    let outcome = selected_control(&wake, Some(format!("wake read: {error}")))
                        .unwrap_or(TerminalOutcome::Error("wake read failed".to_string()));
                    return finish_terminal(watcher.take(), &mut transcript, outcome);
                }
            };
            if readable && let Err(error) = drain_wake(wake.read_fd) {
                let outcome = selected_control(&wake, Some(format!("wake read: {error}")))
                    .unwrap_or(TerminalOutcome::Error("wake read failed".to_string()));
                return finish_terminal(watcher.take(), &mut transcript, outcome);
            }
            if let Some(outcome) = selected_control(&wake, None) {
                return finish_terminal(watcher.take(), &mut transcript, outcome);
            }
            let event = wake.dirty.swap(false, Ordering::AcqRel)
                | wake.uncertain.swap(false, Ordering::AcqRel);
            if event {
                let first = Instant::now();
                let mut last = first;
                loop {
                    let wait = EVENT_QUIET
                        .saturating_sub(last.elapsed())
                        .min(EVENT_MAX.saturating_sub(first.elapsed()));
                    if wait.is_zero() {
                        break;
                    }
                    match wait_wake(wake.read_fd, wait) {
                        Ok(false) => break,
                        Ok(true) => {
                            if let Err(error) = drain_wake(wake.read_fd) {
                                return finish_terminal(
                                    watcher.take(),
                                    &mut transcript,
                                    TerminalOutcome::Error(format!("wake read: {error}")),
                                );
                            }
                            if let Some(outcome) = selected_control(&wake, None) {
                                return finish_terminal(watcher.take(), &mut transcript, outcome);
                            }
                            if wake.dirty.swap(false, Ordering::AcqRel)
                                | wake.uncertain.swap(false, Ordering::AcqRel)
                            {
                                last = Instant::now();
                            }
                        }
                        Err(error) => {
                            return finish_terminal(
                                watcher.take(),
                                &mut transcript,
                                TerminalOutcome::Error(format!("wake read: {error}")),
                            );
                        }
                    }
                }
            }
            let transition_due =
                transition_retry.is_some_and(|deadline| Instant::now() >= deadline);
            if event || transition_due || Instant::now() >= next_audit {
                match observed_change(&mut baseline, repair.as_ref()) {
                    Ok(true) => break,
                    Ok(false) => {
                        match reinstall_watches(
                            watcher.as_mut().expect("active watcher"),
                            &mut installed,
                            &mut baseline,
                            repair.as_ref(),
                            &wake,
                        ) {
                            Ok(WatchTransition::Changed) => break,
                            Ok(WatchTransition::Stable) => transition_retry = None,
                            Ok(WatchTransition::Retry) => {
                                transition_retry = Some(Instant::now() + Duration::from_millis(50));
                            }
                            Err(error) => {
                                let outcome = selected_control(&wake, Some(error)).unwrap_or(
                                    TerminalOutcome::Error("watch transition failed".to_string()),
                                );
                                return finish_terminal(watcher.take(), &mut transcript, outcome);
                            }
                        }
                        next_audit = Instant::now() + AUDIT_INTERVAL;
                    }
                    Err(error) => {
                        return finish_terminal(
                            watcher.take(),
                            &mut transcript,
                            TerminalOutcome::Error(format!(
                                "snapshot '{}': {error}",
                                encode_path(&input)
                            )),
                        );
                    }
                }
            }
        }
        revision = match revision.checked_add(1) {
            Some(revision) => revision,
            None => {
                return finish_terminal(
                    watcher.take(),
                    &mut transcript,
                    TerminalOutcome::Error("revision counter exhausted".to_string()),
                );
            }
        };
    }
}

struct RevisionResult {
    success: bool,
    inputs: FinalBuildInputSet,
    repair: Option<WatchRepairDependency>,
}

#[allow(clippy::too_many_arguments)]
fn build_revision(
    input: &Path,
    output: &Path,
    target: BuildTarget,
    profile: Profile,
    rt_lto: bool,
    thin_lto: bool,
    pgo: &PgoMode,
    jobs: usize,
    cache_stats: bool,
    signal: &Arc<WakeState>,
    transcript: &mut Transcript,
) -> Result<RevisionResult, String> {
    if thin_lto {
        build_thin_revision(
            input,
            output,
            target,
            profile,
            rt_lto,
            jobs,
            cache_stats,
            signal,
            transcript,
        )
    } else {
        build_ordinary_revision(
            input,
            output,
            target,
            profile,
            rt_lto,
            pgo,
            jobs,
            cache_stats,
            signal,
            transcript,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn build_ordinary_revision(
    input: &Path,
    output: &Path,
    target: BuildTarget,
    profile: Profile,
    rt_lto: bool,
    pgo: &PgoMode,
    jobs: usize,
    cache_stats: bool,
    signal: &Arc<WakeState>,
    transcript: &mut Transcript,
) -> Result<RevisionResult, String> {
    let mut first_inputs = None;
    for reuse in [UnitReuse::Allowed, UnitReuse::Forbidden] {
        let mut source_map = SourceMap::new();
        let attempt = align_driver::build_path_pipelined_observed(
            &mut source_map,
            input,
            CacheContext::from_env(),
            reuse,
            &target,
            profile,
            rt_lto,
            jobs,
            pgo,
        );
        let (build, inputs) = match attempt {
            ObservedBuildAttempt::ObservationFailed { error } => return Err(error.to_string()),
            ObservedBuildAttempt::SourceFailed { error, inputs } => {
                transcript
                    .text_record("diagnostic", source_error(input, &error).as_bytes())
                    .map_err(io_text)?;
                return failed_inputs(merge_if_retry(first_inputs.take(), inputs)?);
            }
            ObservedBuildAttempt::Pipeline { build, inputs } => (build, inputs),
        };
        let errors_only = reuse == UnitReuse::Forbidden;
        match build {
            align_driver::PipelinedPackageBuild::FrontendFailed { diags } => {
                record_diags(transcript, &source_map, &diags, errors_only)?;
                return failed_inputs(merge_if_retry(first_inputs.take(), inputs)?);
            }
            align_driver::PipelinedPackageBuild::CodegenFailed {
                diags,
                error: align_driver::PackageCodegenError::StaleCacheEntry { unit, failure },
            } if reuse == UnitReuse::Allowed => {
                record_diags(transcript, &source_map, &diags, false)?;
                transcript.text_record(
                    "cache",
                    format!("alignc: cached unit `{unit}`: {failure}; rebuilding this package without cache reuse").as_bytes(),
                ).map_err(io_text)?;
                first_inputs = Some(inputs);
            }
            align_driver::PipelinedPackageBuild::CodegenFailed { diags, error } => {
                record_diags(transcript, &source_map, &diags, errors_only)?;
                transcript
                    .text_record("diagnostic", format!("alignc: {error}").as_bytes())
                    .map_err(io_text)?;
                return failed_inputs(merge_if_retry(first_inputs.take(), inputs)?);
            }
            align_driver::PipelinedPackageBuild::Complete(build) => {
                record_diags(transcript, &source_map, &build.diags, errors_only)?;
                let inputs = merge_if_retry(first_inputs.take(), inputs)?;
                if cache_stats {
                    record_cache_stats(
                        transcript,
                        &build.units,
                        &build.codegen.outcomes,
                        CacheContext::from_env().codegen_is_enabled(),
                    )?;
                }
                record_pgo(transcript, pgo, &build.codegen)?;
                let link_libs =
                    link_lib_union(build.units.iter().map(|unit| unit.link_libs.as_slice()));
                let objects: Vec<&Path> = build
                    .units
                    .iter()
                    .map(align_driver::PipelinedBuiltUnit::object)
                    .collect();
                return finalize_and_link(
                    inputs, output, &objects, &link_libs, profile, pgo, &target, signal, transcript,
                );
            }
        }
    }
    Err("ordinary retry state exhausted".to_string())
}

fn failed_inputs(inputs: BuildInputSet) -> Result<RevisionResult, String> {
    let finalized = finalize_watch_inputs(inputs, None).map_err(|error| error.to_string())?;
    let (inputs, _) = finalized.into_parts();
    Ok(RevisionResult {
        success: false,
        inputs,
        repair: None,
    })
}

fn merge_if_retry(
    first: Option<BuildInputSet>,
    current: BuildInputSet,
) -> Result<BuildInputSet, String> {
    match first {
        Some(first) => {
            merge_observed_build_inputs(first, current).map_err(|error| error.to_string())
        }
        None => Ok(current),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_thin_revision(
    input: &Path,
    output: &Path,
    target: BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    cache_stats: bool,
    signal: &Arc<WakeState>,
    transcript: &mut Transcript,
) -> Result<RevisionResult, String> {
    let mut source_map = SourceMap::new();
    let (walk, inputs) = match align_driver::build_path_per_unit_observed(&mut source_map, input) {
        ObservedPerUnitBuild::ObservationFailed { error } => return Err(error.to_string()),
        ObservedPerUnitBuild::SourceFailed { error, inputs } => {
            transcript
                .text_record("diagnostic", source_error(input, &error).as_bytes())
                .map_err(io_text)?;
            return failed_inputs(inputs);
        }
        ObservedPerUnitBuild::Walk { walk, inputs } => (walk, inputs),
    };
    record_diags(transcript, &source_map, &walk.diags, false)?;
    if walk.diags.has_errors() || walk.units.is_empty() {
        return failed_inputs(inputs);
    }
    let stage = match ArtifactStage::temp("align-watch-thin") {
        Ok(stage) => stage,
        Err(error) => {
            transcript
                .text_record("diagnostic", format!("alignc: {error}").as_bytes())
                .map_err(io_text)?;
            return failed_inputs(inputs);
        }
    };
    let object_paths: Vec<PathBuf> = (0..walk.units.len())
        .map(|index| stage.path().join(format!("unit{index}.o")))
        .collect();
    let cache = CacheContext::from_env();
    let outcomes = if walk.units.len() >= 2 {
        align_driver::build_thin_lto(
            &walk.units,
            &object_paths,
            &cache,
            &target,
            profile,
            &[],
            rt_lto,
            stage.path(),
            jobs,
        )
        .map(|build| build.outcomes)
    } else {
        align_driver::codegen_units_parallel(
            &walk.units,
            &object_paths,
            &cache,
            &target,
            profile,
            rt_lto,
            jobs,
            &PgoMode::Off,
        )
        .map(|build| build.outcomes)
    };
    let outcomes = match outcomes {
        Ok(outcomes) => outcomes,
        Err(error) => {
            transcript
                .text_record("diagnostic", format!("alignc: {error}").as_bytes())
                .map_err(io_text)?;
            return failed_inputs(inputs);
        }
    };
    if cache_stats {
        if walk.units.len() >= 2 {
            record_thin_cache_stats(transcript, &outcomes, cache.codegen_is_enabled())?;
        } else {
            record_plain_outcomes(transcript, &outcomes, cache.codegen_is_enabled())?;
        }
    }
    let link_libs = link_lib_union(walk.units.iter().map(|unit| unit.mir.link_libs.as_slice()));
    let objects: Vec<&Path> = object_paths.iter().map(PathBuf::as_path).collect();
    finalize_and_link(
        inputs,
        output,
        &objects,
        &link_libs,
        profile,
        &PgoMode::Off,
        &target,
        signal,
        transcript,
    )
}

#[allow(clippy::too_many_arguments)]
fn finalize_and_link(
    inputs: BuildInputSet,
    output: &Path,
    objects: &[&Path],
    link_libs: &[String],
    profile: Profile,
    pgo: &PgoMode,
    target: &BuildTarget,
    signal: &Arc<WakeState>,
    transcript: &mut Transcript,
) -> Result<RevisionResult, String> {
    let finalized =
        finalize_watch_inputs(inputs, Some(output)).map_err(|error| error.to_string())?;
    if finalized.inputs().changed_during_attempt() {
        transcript
            .text_record("notice", b"alignc: watch: inputs changed during revision")
            .map_err(io_text)?;
        let (inputs, repair) = finalized.into_parts();
        return Ok(RevisionResult {
            success: false,
            inputs,
            repair,
        });
    }
    if let Some(index) = finalized.alias_index() {
        transcript
            .text_record(
                "notice",
                format!(
                    "alignc: watch: output '{}' aliases observed input '{}'",
                    encode_path(output),
                    encode_path(finalized.inputs().inputs()[index].path())
                )
                .as_bytes(),
            )
            .map_err(io_text)?;
        let (inputs, repair) = finalized.into_parts();
        return Ok(RevisionResult {
            success: false,
            inputs,
            repair,
        });
    }
    let mut sink = TranscriptSink {
        transcript,
        signal,
        first_error: None,
    };
    let link = if matches!(pgo, PgoMode::Instrument) {
        match align_driver::profile_runtime_archive(target) {
            Ok(profile_rt) => {
                let destination = std::env::var("LLVM_PROFILE_FILE")
                    .unwrap_or_else(|_| "default.profraw".to_string());
                sink.transcript
                    .text_record(
                        "notice",
                        format!(
                            "alignc: --pgo-instrument: instrumented binary will write its profile to `{destination}` when run (set LLVM_PROFILE_FILE to redirect); then `llvm-profdata-22 merge` it and rebuild with `--pgo-use <file.profdata>`"
                        )
                        .as_bytes(),
                    )
                    .map_err(io_text)?;
                align_driver::link_objects_instrumented_with_output(
                    objects,
                    output,
                    link_libs,
                    profile,
                    &profile_rt,
                    &mut sink,
                )
            }
            Err(error) => Err(error),
        }
    } else {
        align_driver::link_objects_with_output(objects, output, link_libs, profile, &mut sink)
    };
    if let Some(error) = sink.first_error.take() {
        return Err(error);
    }
    let (inputs, repair) = finalized.into_parts();
    match link {
        Ok(()) => {
            sink.transcript
                .text_record(
                    "success",
                    format!("alignc: built executable: {}", encode_path(output)).as_bytes(),
                )
                .map_err(io_text)?;
            Ok(RevisionResult {
                success: true,
                inputs,
                repair,
            })
        }
        Err(error)
            if signal.control.load(Ordering::Acquire) & SIGNAL_MASK != 0
                && error.starts_with("child stopped by ") =>
        {
            Err(error)
        }
        Err(error) if link_infrastructure_error(&error) => Err(error),
        Err(error) => {
            sink.transcript
                .text_record("diagnostic", format!("alignc: {error}").as_bytes())
                .map_err(io_text)?;
            Ok(RevisionResult {
                success: false,
                inputs,
                repair,
            })
        }
    }
}

fn link_infrastructure_error(error: &str) -> bool {
    [
        "child wait setup:",
        "child group cleanup:",
        "child group changed",
        "child group check:",
        "child wait:",
        "child output poll:",
        "child stdout read:",
        "child stderr read:",
        "child stdout output:",
        "child stderr output:",
    ]
    .iter()
    .any(|prefix| error.starts_with(prefix))
}

fn record_diags(
    transcript: &mut Transcript,
    source_map: &SourceMap,
    diagnostics: &align_diag::Diagnostics,
    errors_only: bool,
) -> Result<(), String> {
    if diagnostics.is_empty() {
        return Ok(());
    }
    let rendered = render_diags(source_map, diagnostics, errors_only);
    if rendered.is_empty() {
        return Ok(());
    }
    transcript
        .text_record("diagnostic", rendered.as_bytes())
        .map_err(io_text)
}

fn render_diags(
    source_map: &SourceMap,
    diagnostics: &align_diag::Diagnostics,
    errors_only: bool,
) -> String {
    let mut rendered = String::new();
    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| !errors_only || diagnostic.severity == align_diag::Severity::Error)
    {
        let severity = match diagnostic.severity {
            align_diag::Severity::Error => "error",
            align_diag::Severity::Warning => "warning",
        };
        if let Some(span) = diagnostic.span {
            let file = source_map.get(span.file);
            let (line, column) = file.line_col(span.lo);
            use std::fmt::Write as _;
            let _ = writeln!(
                rendered,
                "{}:{line}:{column}: {severity}: {}",
                file.name, diagnostic.message
            );
        } else {
            use std::fmt::Write as _;
            let _ = writeln!(rendered, "{severity}: {}", diagnostic.message);
        }
    }
    rendered
}

fn record_cache_stats(
    transcript: &mut Transcript,
    units: &[align_driver::PipelinedBuiltUnit],
    outcomes: &[align_driver::CacheOutcome],
    enabled: bool,
) -> Result<(), String> {
    let mut frontend_hits = 0usize;
    let mut frontend_misses = 0usize;
    for unit in units {
        if let Some(frontend) = &unit.frontend {
            if frontend.hit {
                frontend_hits += 1;
            } else {
                frontend_misses += 1;
            }
            record_outcome(transcript, frontend, Some("frontend"))?;
        }
    }
    if frontend_hits + frontend_misses > 0 {
        transcript
            .text_record(
                "cache",
                format!(
                    "alignc: cache: {} frontend: {frontend_hits} hit, {frontend_misses} miss",
                    frontend_hits + frontend_misses
                )
                .as_bytes(),
            )
            .map_err(io_text)?;
    }
    record_plain_outcomes(transcript, outcomes, enabled)
}

fn record_plain_outcomes(
    transcript: &mut Transcript,
    outcomes: &[align_driver::CacheOutcome],
    enabled: bool,
) -> Result<(), String> {
    if !enabled {
        return transcript
            .text_record(
                "cache",
                b"alignc: cache: disabled (set ALIGNC_CACHE=on or a path to enable)",
            )
            .map_err(io_text);
    }
    for outcome in outcomes {
        record_outcome(transcript, outcome, None)?;
    }
    let hits = outcomes.iter().filter(|outcome| outcome.hit).count();
    transcript
        .text_record(
            "cache",
            format!(
                "alignc: cache: {} unit(s): {hits} hit, {} miss",
                outcomes.len(),
                outcomes.len() - hits
            )
            .as_bytes(),
        )
        .map_err(io_text)
}

fn record_thin_cache_stats(
    transcript: &mut Transcript,
    outcomes: &[align_driver::CacheOutcome],
    enabled: bool,
) -> Result<(), String> {
    if !enabled {
        return transcript
            .text_record(
                "cache",
                b"alignc: cache: disabled (set ALIGNC_CACHE=on or a path to enable)",
            )
            .map_err(io_text);
    }
    for stage in [
        align_driver::CacheStage::ThinLtoPrelink,
        align_driver::CacheStage::ThinLtoBackend,
    ] {
        let mut hits = 0usize;
        let mut misses = 0usize;
        for outcome in outcomes.iter().filter(|outcome| outcome.stage == stage) {
            if outcome.hit {
                hits += 1;
            } else {
                misses += 1;
            }
            record_outcome(transcript, outcome, Some(stage.label()))?;
        }
        transcript
            .text_record(
                "cache",
                format!(
                    "alignc: cache: {} {}: {hits} hit, {misses} miss",
                    hits + misses,
                    stage.label()
                )
                .as_bytes(),
            )
            .map_err(io_text)?;
    }
    Ok(())
}

fn record_outcome(
    transcript: &mut Transcript,
    outcome: &align_driver::CacheOutcome,
    stage: Option<&str>,
) -> Result<(), String> {
    let state = if outcome.hit {
        "hit".to_string()
    } else {
        let reason = outcome
            .miss_reason
            .as_ref()
            .map_or("miss", |reason| reason.reason());
        format!("miss ({reason})")
    };
    let stage = stage.map_or_else(String::new, |stage| format!(" {stage}"));
    transcript
        .text_record(
            "cache",
            format!("alignc: cache: {}{stage} {state}", outcome.unit).as_bytes(),
        )
        .map_err(io_text)
}

fn record_pgo(
    transcript: &mut Transcript,
    pgo: &PgoMode,
    build: &align_driver::UnitCodegen,
) -> Result<(), String> {
    if matches!(pgo, PgoMode::Use(_)) && build.pgo_total > 0 && build.pgo_matched == 0 {
        transcript.text_record("diagnostic", format!(
            "alignc: --pgo-use: the profile matched 0 of {} rebuilt function(s) — is this profile from this program? Proceeding without profile guidance (this affects performance only, never correctness).",
            build.pgo_total
        ).as_bytes()).map_err(io_text)?;
    } else if matches!(pgo, PgoMode::Use(_)) && !build.pgo_warnings.is_empty() {
        transcript.text_record("diagnostic", format!(
            "alignc: --pgo-use: proceeding despite {} PGO profile-use warning(s) across the rebuilt unit(s) ({} of {} function(s) matched the profile; the rest changed since it was collected); first: {}",
            build.pgo_warnings.len(), build.pgo_matched, build.pgo_total, build.pgo_warnings[0]
        ).as_bytes()).map_err(io_text)?;
    }
    Ok(())
}

struct Transcript {
    stderr: io::Stderr,
    frame: Vec<u8>,
}

impl Transcript {
    fn stderr() -> Self {
        Self {
            stderr: io::stderr(),
            frame: Vec::with_capacity(MAX_RECORD_FRAME),
        }
    }

    fn text_record(&mut self, kind: &str, payload: &[u8]) -> io::Result<()> {
        let encoded = encode_text(payload);
        self.record(kind, encoded.as_bytes())
    }

    fn record(&mut self, kind: &str, payload: &[u8]) -> io::Result<()> {
        if payload.is_empty() {
            return self.record_chunk(kind, "end", &[]);
        }
        let mut chunks = payload.chunks(RECORD_CHUNK).peekable();
        while let Some(chunk) = chunks.next() {
            let state = if chunks.peek().is_some() {
                "more"
            } else {
                "end"
            };
            self.record_chunk(kind, state, chunk)?;
        }
        Ok(())
    }

    fn record_chunk(&mut self, kind: &str, state: &str, chunk: &[u8]) -> io::Result<()> {
        self.frame.clear();
        self.frame.extend_from_slice(b"alignc: watch: record ");
        self.frame.extend_from_slice(kind.as_bytes());
        self.frame.push(b' ');
        self.frame.extend_from_slice(state.as_bytes());
        self.frame.extend_from_slice(b": ");
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for &byte in chunk {
            if (0x20..=0x7e).contains(&byte) && byte != b'%' {
                self.frame.push(byte);
            } else {
                self.frame.push(b'%');
                self.frame.push(HEX[usize::from(byte >> 4)]);
                self.frame.push(HEX[usize::from(byte & 0x0f)]);
            }
        }
        self.frame.push(b'\n');
        if self.frame.len() > MAX_RECORD_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "watch record frame exceeds 12331 bytes",
            ));
        }
        self.stderr.lock().write_all(&self.frame)
    }

    fn marker(&mut self, revision: u64, success: bool) -> io::Result<()> {
        let mut writer = self.stderr.lock();
        writer.flush()?;
        let state = if success { "ready" } else { "failed" };
        self.frame.clear();
        writeln!(self.frame, "alignc: watch: revision {revision} {state}")?;
        if self.frame.len() > 128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "watch marker exceeds 128 bytes",
            ));
        }
        write_terminal_fd(libc::STDERR_FILENO, &self.frame)
    }

    fn terminal(&mut self, line: &str) -> io::Result<()> {
        let mut writer = self.stderr.lock();
        writer.flush()?;
        self.frame.clear();
        self.frame.extend_from_slice(line.as_bytes());
        self.frame.push(b'\n');
        if self.frame.len() > 128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "watch terminal line exceeds 128 bytes",
            ));
        }
        write_terminal_fd(libc::STDERR_FILENO, &self.frame)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stderr.lock().flush()
    }
}

fn write_terminal_fd(fd: RawFd, bytes: &[u8]) -> io::Result<()> {
    loop {
        // SAFETY: `bytes` remains live for the duration of this single descriptor write.
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if usize::try_from(written).ok() != Some(bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short watch terminal write",
            ));
        }
        return Ok(());
    }
}

struct TranscriptSink<'a> {
    transcript: &'a mut Transcript,
    signal: &'a Arc<WakeState>,
    first_error: Option<String>,
}

impl LinkOutputSink for TranscriptSink<'_> {
    fn write(&mut self, stream: LinkOutputStream, bytes: &[u8]) -> io::Result<()> {
        let result = self.transcript.record(
            match stream {
                LinkOutputStream::Stdout => "child-stdout",
                LinkOutputStream::Stderr => "child-stderr",
            },
            bytes,
        );
        if let Err(error) = &result {
            self.first_error
                .get_or_insert_with(|| format!("transcript write: {error}"));
        }
        result
    }

    fn stop_signal(&mut self) -> Option<LinkStopSignal> {
        match self.signal.control.load(Ordering::Acquire) & SIGNAL_MASK {
            1 => Some(LinkStopSignal::SigHup),
            2 => Some(LinkStopSignal::SigInt),
            3 => Some(LinkStopSignal::SigQuit),
            4 => Some(LinkStopSignal::SigTerm),
            _ => None,
        }
    }
}

fn install_wake_and_signals() -> io::Result<Arc<WakeState>> {
    if SIGNAL_OWNER
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(io::Error::other("another watch signal owner is active"));
    }
    let result = install_wake_and_signals_inner();
    if result.is_err() {
        SIGNAL_OWNER.store(false, Ordering::Release);
    }
    result
}

fn install_wake_and_signals_inner() -> io::Result<Arc<WakeState>> {
    let signals = [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];
    let mut blocked = std::mem::MaybeUninit::<libc::sigset_t>::zeroed();
    let mut previous = std::mem::MaybeUninit::<libc::sigset_t>::zeroed();
    // SAFETY: both sets are writable fixed records, and all inserted values are signals.
    unsafe {
        if libc::sigemptyset(blocked.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        for signal in signals {
            if libc::sigaddset(blocked.as_mut_ptr(), signal) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let result =
            libc::pthread_sigmask(libc::SIG_BLOCK, blocked.as_ptr(), previous.as_mut_ptr());
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
    }
    // SAFETY: successful `pthread_sigmask` initialized both records.
    let previous = unsafe { previous.assume_init() };
    let restore_mask = || -> io::Result<()> {
        // SAFETY: `previous` is the saved caller mask and the output argument is unused.
        let result =
            unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(result))
        }
    };

    let mut fds = [-1; 2];
    // SAFETY: `fds` is a writable two-element descriptor array.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
        let error = io::Error::last_os_error();
        let _ = restore_mask();
        return Err(error);
    }
    for fd in fds {
        if let Err(error) = set_nonblocking_cloexec(fd) {
            // SAFETY: both descriptors were returned by `pipe` and have not been closed.
            unsafe {
                libc::close(fds[1]);
                libc::close(fds[0]);
            }
            let _ = restore_mask();
            return Err(error);
        }
    }
    let state = Arc::new(WakeState {
        control: AtomicU16::new(0),
        dirty: AtomicBool::new(false),
        uncertain: AtomicBool::new(false),
        read_fd: fds[0],
        write_fd: fds[1],
    });
    let mut registered = Vec::with_capacity(signals.len());
    for (index, signal) in signals.into_iter().enumerate() {
        let signal_code = u16::try_from(index + 1).unwrap_or(0);
        let handler_state = Arc::clone(&state);
        // SAFETY: the handler performs only lock-free atomic operations and one async-signal-safe
        // nonblocking write through a process-lifetime descriptor.
        match unsafe {
            signal_hook::low_level::register(signal, move || {
                handler_state.set_signal(signal_code);
            })
        } {
            Ok(id) => registered.push(id),
            Err(error) => {
                for id in registered.into_iter().rev() {
                    signal_hook::low_level::unregister(id);
                }
                // SAFETY: handler registration was rolled back before descriptor closure.
                unsafe {
                    libc::close(fds[1]);
                    libc::close(fds[0]);
                }
                let _ = restore_mask();
                return Err(error);
            }
        }
    }
    if let Err(error) = restore_mask() {
        for id in registered.into_iter().rev() {
            signal_hook::low_level::unregister(id);
        }
        // SAFETY: handlers are gone, so the descriptors can no longer be reached.
        unsafe {
            libc::close(fds[1]);
            libc::close(fds[0]);
        }
        return Err(error);
    }
    Ok(state)
}

fn set_nonblocking_cloexec(fd: RawFd) -> io::Result<()> {
    // SAFETY: the descriptor is live and owned by the caller during setup.
    let status = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if status == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, status | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains live through the flag update.
    let descriptor = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if descriptor == -1
        || unsafe { libc::fcntl(fd, libc::F_SETFD, descriptor | libc::FD_CLOEXEC) } == -1
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn wait_wake(fd: RawFd, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = remaining.as_millis().min(i32::MAX as u128);
        let timeout_ms = i32::try_from(millis).unwrap_or(i32::MAX);
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pollfd` is a live one-element poll array.
        let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if result > 0 {
            if pollfd.revents & libc::POLLIN != 0 {
                return Ok(true);
            }
            return Err(io::Error::other("wake pipe closed"));
        }
        if result == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn drain_wake(fd: RawFd) -> io::Result<()> {
    let mut bytes = [0u8; 4_096];
    loop {
        // SAFETY: `bytes` is writable and the descriptor is the live nonblocking wake read end.
        let result = unsafe { libc::read(fd, bytes.as_mut_ptr().cast(), bytes.len()) };
        if result > 0 {
            return Ok(());
        }
        if result == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "wake pipe closed",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.kind() == io::ErrorKind::WouldBlock {
            return Ok(());
        }
        return Err(error);
    }
}

enum TerminalOutcome {
    Stop(u16),
    Error(String),
}

fn selected_control(
    wake: &WakeState,
    synchronous_error: Option<String>,
) -> Option<TerminalOutcome> {
    let control = wake.control.load(Ordering::Acquire);
    if control & SIGNAL_MASK != 0 {
        return Some(TerminalOutcome::Stop(control));
    }
    let fatal = (control & FATAL_MASK) >> FATAL_SHIFT;
    if fatal != 0 {
        let message = match fatal {
            1 => "backend: disconnected",
            2 => "backend: io",
            3 => "backend: capacity",
            4 => "backend: invalid config",
            _ => "backend: other",
        };
        return Some(TerminalOutcome::Error(message.to_string()));
    }
    synchronous_error.map(TerminalOutcome::Error)
}

fn finish_terminal(
    watcher: Option<NativeWatcher>,
    transcript: &mut Transcript,
    mut outcome: TerminalOutcome,
) -> ExitCode {
    let wake = watcher.as_ref().map(NativeWatcher::wake);
    drop(watcher);
    if let Some(wake) = wake
        && let Some(selected) = selected_control(&wake, None)
    {
        outcome = selected;
    }
    let code = match outcome {
        TerminalOutcome::Stop(control) => stop_code(control, transcript).unwrap_or(1),
        TerminalOutcome::Error(error) => {
            let _ = transcript.text_record(
                "watcher-error",
                format!("alignc: watch: watcher error: {error}").as_bytes(),
            );
            let _ = transcript.flush();
            1
        }
    };
    std::process::exit(code)
}

fn finish_transcript_failure(watcher: Option<NativeWatcher>, _error: io::Error) -> ExitCode {
    drop(watcher);
    std::process::exit(1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WatchTransition {
    Stable,
    Changed,
    Retry,
}

struct InstalledWatches {
    entries: Vec<InstalledWatch>,
    tentative: bool,
    next_generation: u64,
}

impl Default for InstalledWatches {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            tentative: false,
            next_generation: 1,
        }
    }
}

struct InstalledWatch {
    registration: WatchRegistration,
    handle: NativeHandle,
    generation: u64,
}

fn reinstall_watches(
    watcher: &mut NativeWatcher,
    installed: &mut InstalledWatches,
    baseline: &mut MonitorBaseline,
    repair: Option<&WatchRepairDependency>,
    wake: &WakeState,
) -> Result<WatchTransition, String> {
    for _ in 0..8 {
        transition_checkpoint(wake)?;
        let desired = monitor_watch_registrations(baseline, repair)
            .map_err(|error| error.to_string())?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        transition_checkpoint(wake)?;
        if !registrations_are_coherent(&desired) {
            return Ok(WatchTransition::Changed);
        }
        if installed.tentative {
            compact_watches(watcher, installed, &desired, wake)?;
            installed.tentative = false;
        }
        let missing = desired.iter().any(|registration| {
            !installed
                .entries
                .iter()
                .any(|entry| entry.registration == *registration)
        });
        let full_generation = missing && !watcher.retargets_by_path();
        let additions = desired
            .iter()
            .filter(|registration| {
                full_generation
                    || !installed
                        .entries
                        .iter()
                        .any(|entry| entry.registration == **registration)
            })
            .cloned()
            .collect::<Vec<_>>();
        let generation = installed.next_generation;
        if !additions.is_empty() {
            installed.next_generation = installed
                .next_generation
                .checked_add(1)
                .ok_or_else(|| "watch generation counter exhausted".to_string())?;
        }
        let mut added = Vec::new();
        let mut retargeted = Vec::new();
        let mut lost = false;
        let mut add_error = None;
        for registration in &additions {
            transition_checkpoint(wake)?;
            if watcher.retargets_by_path()
                && let Some((index, entry)) = installed
                    .entries
                    .iter_mut()
                    .enumerate()
                    .find(|(_, entry)| entry.handle.path() == registration.path())
            {
                retargeted.push((index, entry.registration.clone(), entry.generation));
                entry.registration = registration.clone();
                entry.generation = generation;
                continue;
            }
            if installed.entries.len().checked_add(added.len()) == Some(131_072) {
                add_error =
                    Some("too many native watch registrations (maximum 131072)".to_string());
                break;
            }
            match watcher.watch(registration, generation) {
                Ok(handle) => added.push(InstalledWatch {
                    registration: registration.clone(),
                    handle,
                    generation,
                }),
                Err(error) if error.kind == NativeWatchErrorKind::Lost => {
                    lost = true;
                    break;
                }
                Err(error) => {
                    add_error = Some(format!(
                        "add '{}': {error}",
                        encode_path(registration.path())
                    ));
                    break;
                }
            }
            transition_checkpoint(wake)?;
        }
        if lost || add_error.is_some() {
            rollback_additions(watcher, added, wake)?;
            for (index, registration, old_generation) in retargeted.into_iter().rev() {
                installed.entries[index].registration = registration;
                installed.entries[index].generation = old_generation;
            }
            retain_native_generations(watcher, installed);
            if let Some(error) = add_error {
                return Err(error);
            }
            transition_checkpoint(wake)?;
            if observed_change(baseline, repair)? {
                return Ok(WatchTransition::Changed);
            }
            transition_checkpoint(wake)?;
            continue;
        }
        installed.entries.extend(added);
        transition_checkpoint(wake)?;
        if observed_change(baseline, repair)? {
            installed.tentative = true;
            return Ok(WatchTransition::Changed);
        }
        transition_checkpoint(wake)?;
        let refreshed_desired = monitor_watch_registrations(baseline, repair)
            .map_err(|error| error.to_string())?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        transition_checkpoint(wake)?;
        if refreshed_desired != desired {
            installed.tentative = true;
            return Ok(WatchTransition::Changed);
        }

        compact_watches(watcher, installed, &desired, wake)?;
        transition_checkpoint(wake)?;
        if observed_change(baseline, repair)? {
            installed.tentative = true;
            return Ok(WatchTransition::Changed);
        }
        transition_checkpoint(wake)?;
        let final_desired = monitor_watch_registrations(baseline, repair)
            .map_err(|error| error.to_string())?
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        transition_checkpoint(wake)?;
        if final_desired != desired {
            continue;
        }
        return Ok(WatchTransition::Stable);
    }
    Ok(WatchTransition::Retry)
}

fn registrations_are_coherent(
    registrations: &std::collections::BTreeSet<WatchRegistration>,
) -> bool {
    let mut prior: Option<&WatchRegistration> = None;
    for registration in registrations {
        if let Some(prior) = prior
            && prior.path() == registration.path()
            && prior != registration
        {
            return false;
        }
        prior = Some(registration);
    }
    true
}

fn compact_watches(
    watcher: &mut NativeWatcher,
    installed: &mut InstalledWatches,
    desired: &std::collections::BTreeSet<WatchRegistration>,
    wake: &WakeState,
) -> Result<(), String> {
    let mut newest = std::collections::BTreeMap::new();
    for entry in &installed.entries {
        if desired.contains(&entry.registration) {
            newest
                .entry(entry.registration.clone())
                .and_modify(|generation: &mut u64| {
                    *generation = (*generation).max(entry.generation)
                })
                .or_insert(entry.generation);
        }
    }
    installed.entries.sort_by(|left, right| {
        left.registration
            .cmp(&right.registration)
            .then(left.generation.cmp(&right.generation))
    });
    let mut retained = Vec::with_capacity(desired.len());
    let mut pending = std::mem::take(&mut installed.entries).into_iter();
    while let Some(entry) = pending.next() {
        transition_checkpoint(wake)?;
        if newest.get(&entry.registration) == Some(&entry.generation) {
            retained.push(entry);
            continue;
        }
        match watcher.unwatch(&entry.handle) {
            Ok(()) => {}
            Err(error) if error.kind == NativeWatchErrorKind::Lost => {}
            Err(error) => {
                let path = entry.handle.path().to_path_buf();
                retained.push(entry);
                retained.extend(pending);
                installed.entries = retained;
                return Err(format!("remove '{}': {error}", encode_path(&path)));
            }
        }
        transition_checkpoint(wake)?;
    }
    installed.entries = retained;
    retain_native_generations(watcher, installed);
    Ok(())
}

fn rollback_additions(
    watcher: &mut NativeWatcher,
    added: Vec<InstalledWatch>,
    wake: &WakeState,
) -> Result<(), String> {
    for entry in added.into_iter().rev() {
        transition_checkpoint(wake)?;
        match watcher.unwatch(&entry.handle) {
            Ok(()) => {}
            Err(error) if error.kind == NativeWatchErrorKind::Lost => {}
            Err(error) => {
                return Err(format!(
                    "remove '{}': {error}",
                    encode_path(entry.handle.path())
                ));
            }
        }
        transition_checkpoint(wake)?;
    }
    Ok(())
}

fn transition_checkpoint(wake: &WakeState) -> Result<(), String> {
    if selected_control(wake, None).is_some() {
        Err("watch transition interrupted".to_string())
    } else {
        Ok(())
    }
}

fn retain_native_generations(watcher: &mut NativeWatcher, installed: &InstalledWatches) {
    watcher.retain_generations(
        &installed
            .entries
            .iter()
            .map(|entry| entry.generation)
            .collect(),
    );
}

fn observed_change(
    baseline: &mut MonitorBaseline,
    repair: Option<&WatchRepairDependency>,
) -> Result<bool, String> {
    if refresh_monitor_baseline(baseline).map_err(|error| error.to_string())?
        == MonitorRefresh::Changed
    {
        return Ok(true);
    }
    if let Some(repair) = repair {
        let current = snapshot_watch_repair(repair.path()).map_err(|error| error.to_string())?;
        if &current != repair {
            return Ok(true);
        }
    }
    Ok(false)
}

fn source_error(path: &Path, error: &BuildSourceError) -> String {
    let detail = match error {
        BuildSourceError::Missing => "No such file or directory".to_string(),
        BuildSourceError::NonRegular => "not a regular file".to_string(),
        BuildSourceError::InvalidUtf8 { offset } => {
            format!("invalid UTF-8 at byte {offset}")
        }
        BuildSourceError::Io { message } => message.clone(),
    };
    format!("alignc: cannot read '{}': {detail}", encode_path(path))
}

fn terminal_error(transcript: &mut Transcript, error: String) -> ExitCode {
    let _ = transcript.text_record(
        "watcher-error",
        format!("alignc: watch: watcher error: {error}").as_bytes(),
    );
    ExitCode::FAILURE
}

fn stop_code(control: u16, transcript: &mut Transcript) -> Option<i32> {
    let (name, code) = match control & SIGNAL_MASK {
        1 => ("SIGHUP", 129),
        2 => ("SIGINT", 130),
        3 => ("SIGQUIT", 131),
        4 => ("SIGTERM", 143),
        _ => return None,
    };
    match transcript.terminal(&format!("alignc: watch: stopped by {name}")) {
        Ok(()) => Some(code),
        Err(_) => Some(1),
    }
}

fn io_text(error: io::Error) -> String {
    format!("transcript write: {error}")
}

fn encode_path(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        encode_path_bytes(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        encode_path_bytes(path.to_string_lossy().as_bytes())
    }
}

fn encode_path_bytes(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-') {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(result, "%{byte:02X}");
        }
    }
    result
}

fn encode_text(bytes: &[u8]) -> String {
    if bytes.len() > 16_384 || std::str::from_utf8(bytes).is_err() {
        return "message exceeds 16384-byte limit".to_string();
    }
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(3));
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in bytes {
        if (0x20..=0x7e).contains(&byte) && byte != b'%' {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{encode_text, render_diags};

    #[test]
    fn watch_text_is_reversible_and_bounded() {
        assert_eq!(encode_text(b"marker\n%"), "marker%0A%25");
        assert_eq!(encode_text(&vec![b'x'; 16_384]).len(), 16_384);
        assert_eq!(
            encode_text(&vec![b'x'; 16_385]),
            "message exceeds 16384-byte limit"
        );
    }

    #[test]
    fn retry_diagnostics_keep_errors_and_do_not_encode_observed_paths_twice() {
        let mut source_map = align_span::SourceMap::new();
        let file = source_map.add_file("/tmp/space%20%25.align", "x");
        let span = align_span::Span::new(file, 0, 1);
        let mut diagnostics = align_diag::Diagnostics::new();
        diagnostics.push(align_diag::Diagnostic::warning("warning", span));
        diagnostics.error("error", span);

        let all = render_diags(&source_map, &diagnostics, false);
        assert!(all.contains("/tmp/space%20%25.align:1:1: warning: warning"));
        assert!(all.contains(": error: error"));
        assert!(!all.contains("%2520"));

        let retry = render_diags(&source_map, &diagnostics, true);
        assert!(!retry.contains("warning"));
        assert!(retry.contains("/tmp/space%20%25.align:1:1: error: error"));
    }
}
