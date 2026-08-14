//! The `alignc build` path and the output policy (`docs/impl/22-repl-plan.md` §4.1, §6).

use align_driver::{ArtifactStage, BuildTarget, CacheContext, PgoMode, Profile, UnitReuse};
use align_span::SourceMap;

const COMPILER_STACK_BYTES: usize = 32 * 1024 * 1024;
const COMPILER_THREAD_NAME: &str = "align-repl-compiler";

/// Run one compiler transaction with the same stack headroom as the in-process driver tests.
fn on_compiler_worker<T, F>(work: F) -> Result<T, String>
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name(COMPILER_THREAD_NAME.to_string())
            .stack_size(COMPILER_STACK_BYTES)
            .spawn_scoped(scope, work)
            .map_err(|error| format!("cannot start the compiler worker: {error}"))?;
        match worker.join() {
            Ok(result) => Ok(result),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

/// A candidate compiled far enough to judge it: the driver's own diagnostics, rendered.
pub(crate) struct Checked {
    pub had_errors: bool,
    pub rendered: String,
}

/// A whole-program check whose HIR remains available even when diagnostics contain errors. The
/// duplicate-name retry needs that partial HIR to compute the candidate's local-name delta without
/// scraping names out of diagnostic text (§3.5.1/§3.6).
pub(crate) struct HirChecked {
    pub hir: align_sema::Program,
    pub had_errors: bool,
    pub rendered: String,
}

/// Compile `src` through the package frontend — the same entry point `alignc build` uses, so a
/// candidate that is later built pays a *unit* frontend the memo can serve.
///
/// This is deliberately NOT `align_driver::check`: that is the whole-program projection, which
/// memoizes under a different stage, so judging a candidate with it and then building with
/// `build_package` would miss on every entry (§4.2).
pub(crate) fn check_candidate(path: &std::path::Path, src: &str) -> Result<Checked, String> {
    on_compiler_worker(|| {
        let mut sm = SourceMap::new();
        let name = path.display().to_string();
        let build = align_driver::build_package(&mut sm, &name, src, &CacheContext::from_env(), UnitReuse::Allowed);
        Checked {
            had_errors: build.diags.has_errors(),
            rendered: align_driver::format_diagnostics(&sm, &build.diags),
        }
    })
}

/// The whole-program HIR for `src`, for the type echo and `:type`.
///
/// Only reached when a type NAME is actually needed — a printable value never gets here, because
/// `print(E)` checking clean is already the whole answer (§3.4 case 1).
pub(crate) fn checked_hir(path: &std::path::Path, src: &str) -> Result<align_sema::Program, String> {
    let checked = check_hir(path, src)?;
    if checked.had_errors {
        return Err(checked.rendered);
    }
    Ok(checked.hir)
}

pub(crate) fn check_hir(path: &std::path::Path, src: &str) -> Result<HirChecked, String> {
    on_compiler_worker(|| {
        let mut sm = SourceMap::new();
        let name = path.display().to_string();
        let checked = align_driver::check(&mut sm, &name, src);
        HirChecked {
            had_errors: checked.diags.has_errors(),
            rendered: align_driver::format_diagnostics(&sm, &checked.diags),
            hir: checked.hir,
        }
    })
}

/// Build `src` into `exe`, exactly as `alignc build` does: package frontend, parallel per-unit
/// codegen against the shared cache, link, then same-directory atomic rename.
pub(crate) fn build_exe(
    stage: &ArtifactStage,
    src: &str,
    exe: &std::path::Path,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
) -> Result<(), String> {
    on_compiler_worker(|| {
        let entry = stage.path().join("session.align");
        std::fs::write(&entry, src).map_err(|e| format!("cannot write the session source: {e}"))?;
        build_from_path(
            stage, &entry, src, exe, target, profile, rt_lto, jobs, UnitReuse::Allowed,
        )
    })?
}

#[allow(clippy::too_many_arguments)]
fn build_from_path(
    stage: &ArtifactStage,
    entry: &std::path::Path,
    src: &str,
    exe: &std::path::Path,
    target: &BuildTarget,
    profile: Profile,
    rt_lto: bool,
    jobs: usize,
    reuse: UnitReuse,
) -> Result<(), String> {
    let mut sm = SourceMap::new();
    let name = entry.display().to_string();
    let cache = CacheContext::from_env();
    let mut build = align_driver::build_package(&mut sm, &name, src, &cache, reuse);
    if build.diags.has_errors() {
        return Err(align_driver::format_diagnostics(&sm, &build.diags));
    }
    let obj_paths: Vec<std::path::PathBuf> = (0..build.units.len())
        .map(|i| stage.path().join(format!("unit{i}.o")))
        .collect();
    match align_driver::codegen_package_parallel(
        &mut build,
        &obj_paths,
        &cache,
        target,
        profile,
        rt_lto,
        jobs,
        &PgoMode::Off,
    ) {
        Ok(_) => {}
        // The one recoverable failure, matched on its SHAPE, exactly as `alignc` does: a poisoned
        // cache entry would otherwise wedge the session for its whole life.
        Err(align_driver::PackageCodegenError::StaleCacheEntry { .. }) if reuse == UnitReuse::Allowed => {
            return build_from_path(
                stage,
                entry,
                src,
                exe,
                target,
                profile,
                rt_lto,
                jobs,
                UnitReuse::Forbidden,
            );
        }
        Err(e) => return Err(format!("{e}")),
    }
    // First-seen union across units, matching the driver's deterministic order. A v1 session is a
    // one-unit package, so this reduces to that unit's list.
    let mut link_libs: Vec<String> = Vec::new();
    for unit in &build.units {
        for lib in &unit.link_libs {
            if !link_libs.iter().any(|l| l == lib) {
                link_libs.push(lib.clone());
            }
        }
    }
    let obj_refs: Vec<&std::path::Path> = obj_paths.iter().map(|p| p.as_path()).collect();
    let staged_exe = stage.path().join("session.pending");
    align_driver::link_objects(&obj_refs, &staged_exe, &link_libs, profile)?;
    std::fs::rename(&staged_exe, exe).map_err(|e| format!("cannot publish executable {}: {e}", exe.display()))
}

/// The bytes one run produced, and what §6 decided to show of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOutput {
    pub stdout_shown: Vec<u8>,
    pub stderr_shown: Vec<u8>,
    pub diverged: bool,
    pub truncated: bool,
}

/// The retained snapshot of the previous successful run, plus its truncation state.
#[derive(Default)]
pub(crate) struct OutputBaseline {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// Set when a run exceeded the cap. While set, the prefix rule is disabled and every run
    /// prints in full — the REPL never compares against a partial snapshot.
    poisoned: bool,
    /// The last run's full bytes, for `:out`.
    last_full: Option<(Vec<u8>, Vec<u8>)>,
    /// The most recent run exceeded the retention bound, so `:out` cannot reproduce it.
    last_truncated: bool,
}

impl OutputBaseline {
    /// Reset the byte-prefix baseline after an edit. A prior retention overflow remains poisoned:
    /// §6 disables suffix elision until `:clear`, not merely until the next successful run.
    pub fn reset(&mut self) {
        self.stdout.clear();
        self.stderr.clear();
    }

    /// Reset both the history baseline and a retention poison. This is the `:clear` operation.
    pub fn clear(&mut self) {
        self.reset();
        self.poisoned = false;
    }

    pub fn last_full(&self) -> Option<&(Vec<u8>, Vec<u8>)> {
        self.last_full.as_ref()
    }

    pub fn last_was_truncated(&self) -> bool {
        self.last_truncated
    }

    /// Apply §6 to one run's captured bytes.
    pub fn absorb(&mut self, out: &[u8], err: &[u8], cap: usize) -> RunOutput {
        let over_cap = out.len() > cap || err.len() > cap;
        if over_cap {
            let was_poisoned = self.poisoned;
            // Retention is bounded. The caller already owns these bytes long enough to print this
            // run in full; `:out` cannot keep another unbounded copy after that hand-off.
            self.last_full = None;
            self.last_truncated = true;
            // Print in full, and stop comparing: a partial snapshot would make a later suffix a
            // lie about what the user has seen.
            self.poisoned = true;
            self.stdout.clear();
            self.stderr.clear();
            return RunOutput {
                stdout_shown: out.to_vec(),
                stderr_shown: err.to_vec(),
                diverged: false,
                truncated: !was_poisoned,
            };
        }
        self.last_truncated = false;
        self.last_full = Some((out.to_vec(), err.to_vec()));
        if self.poisoned {
            return RunOutput {
                stdout_shown: out.to_vec(),
                stderr_shown: err.to_vec(),
                diverged: false,
                truncated: false,
            };
        }
        let out_prefix = out.starts_with(&self.stdout);
        let err_prefix = err.starts_with(&self.stderr);
        let result = if out_prefix && err_prefix {
            RunOutput {
                stdout_shown: out[self.stdout.len()..].to_vec(),
                stderr_shown: err[self.stderr.len()..].to_vec(),
                diverged: false,
                truncated: false,
            }
        } else {
            RunOutput {
                stdout_shown: out.to_vec(),
                stderr_shown: err.to_vec(),
                diverged: true,
                truncated: false,
            }
        };
        self.stdout = out.to_vec();
        self.stderr = err.to_vec();
        result
    }
}

/// Run `exe`, capturing both streams. Returns the raw bytes plus the wall time.
pub(crate) fn run_exe(exe: &std::path::Path) -> Result<(Vec<u8>, Vec<u8>, std::process::ExitStatus, f64), String> {
    let start = std::time::Instant::now();
    let out = std::process::Command::new(exe)
        .output()
        .map_err(|e| format!("cannot run the session program: {e}"))?;
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok((out.stdout, out.stderr, out.status, ms))
}

/// Run without capturing either stream. `:time` must not allocate in proportion to discarded output.
pub(crate) fn run_exe_discard(exe: &std::path::Path) -> Result<(std::process::ExitStatus, f64), String> {
    let start = std::time::Instant::now();
    let status = std::process::Command::new(exe)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("cannot run the session program: {e}"))?;
    Ok((status, start.elapsed().as_secs_f64() * 1000.0))
}

#[cfg(test)]
mod tests {
    use align_driver::{BuildTarget, CacheContext, PgoMode, Profile, UnitReuse};

    use super::{COMPILER_STACK_BYTES, COMPILER_THREAD_NAME, OutputBaseline, build_exe, on_compiler_worker};

    #[test]
    fn compiler_work_uses_the_named_32_mib_worker() {
        let name = on_compiler_worker(|| std::thread::current().name().map(str::to_string))
            .unwrap_or_else(|error| panic!("start compiler worker: {error}"));
        assert_eq!(name.as_deref(), Some(COMPILER_THREAD_NAME));
        assert_eq!(COMPILER_STACK_BYTES, 32 * 1024 * 1024);
    }

    #[test]
    fn output_matrix_keeps_retention_poison_until_clear() {
        let mut baseline = OutputBaseline::default();
        let first = baseline.absorb(b"abc", b"", 4);
        assert_eq!(first.stdout_shown, b"abc");
        assert!(!first.diverged);

        let suffix = baseline.absorb(b"abcdef", b"", 8);
        assert_eq!(suffix.stdout_shown, b"def");
        let divergence = baseline.absorb(b"changed", b"", 8);
        assert!(divergence.diverged);
        assert_eq!(divergence.stdout_shown, b"changed");

        let exact_limit = baseline.absorb(b"1234", b"", 4);
        assert!(!exact_limit.truncated);
        let overflow = baseline.absorb(b"12345", b"", 4);
        assert!(overflow.truncated);
        assert!(baseline.last_was_truncated());
        assert!(baseline.last_full().is_none());
        let still_poisoned = baseline.absorb(b"ok", b"", 4);
        assert_eq!(still_poisoned.stdout_shown, b"ok");
        assert!(!still_poisoned.truncated);
        assert!(!baseline.last_was_truncated());
        baseline.reset();
        assert_eq!(baseline.absorb(b"again", b"", 8).stdout_shown, b"again");
        baseline.clear();
        let reset = baseline.absorb(b"fresh", b"", 8);
        assert_eq!(reset.stdout_shown, b"fresh");
        assert!(!reset.diverged);

        let mut streams = OutputBaseline::default();
        let stderr_only = streams.absorb(b"", b"err", 4);
        assert_eq!(stderr_only.stderr_shown, b"err");
        let both = streams.absorb(b"out", b"err+", 4);
        assert_eq!(both.stdout_shown, b"out");
        assert_eq!(both.stderr_shown, b"+");
    }

    #[test]
    fn repl_object_matches_the_alignc_build_path() {
        let source = "// generated by align-repl; every line below is real Align\n\
                      // `main` is fixed at `-> Result<(), Error>` so `?` works in every entry\n\
                      // every statement re-runs on each entry; external side effects are repeated\n\
                      fn main() -> Result<(), Error> {\n\
                        print(40 + 2)\n\
                        return Ok(())\n\
                      }\n";
        let repl_stage = align_driver::ArtifactStage::temp("align-repl-object-owner")
            .unwrap_or_else(|error| panic!("create repl stage: {error}"));
        let alignc_stage = align_driver::ArtifactStage::temp("alignc-object-owner")
            .unwrap_or_else(|error| panic!("create alignc stage: {error}"));
        let profile = Profile::Release;
        let target = BuildTarget::Baseline;
        build_exe(
            &repl_stage,
            source,
            &repl_stage.path().join("session"),
            &target,
            profile,
            align_driver::default_rt_lto(profile),
            1,
        )
        .unwrap_or_else(|error| panic!("build through repl path: {error}"));

        let entry = alignc_stage.path().join("saved.align");
        std::fs::write(&entry, source).unwrap_or_else(|error| panic!("write alignc input: {error}"));
        let mut source_map = align_span::SourceMap::new();
        let cache = CacheContext::from_env();
        let mut build = align_driver::build_package(
            &mut source_map,
            &entry.display().to_string(),
            source,
            &cache,
            UnitReuse::Allowed,
        );
        assert!(
            !build.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&source_map, &build.diags)
        );
        let object = alignc_stage.path().join("unit0.o");
        align_driver::codegen_package_parallel(
            &mut build,
            std::slice::from_ref(&object),
            &cache,
            &target,
            profile,
            align_driver::default_rt_lto(profile),
            1,
            &PgoMode::Off,
        )
        .unwrap_or_else(|error| panic!("codegen through alignc path: {error}"));

        let repl_object = std::fs::read(repl_stage.path().join("unit0.o"))
            .unwrap_or_else(|error| panic!("read repl object: {error}"));
        let alignc_object = std::fs::read(object).unwrap_or_else(|error| panic!("read alignc object: {error}"));
        assert_eq!(repl_object, alignc_object);
    }
}
