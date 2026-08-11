//! Running a compiled `pkg.db` program and checking its result.

use crate::common::{backend_available, cc_available};
use std::fmt;
use std::process::Output;

/// One checkable expectation that did not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub what: String,
    pub expected: String,
    pub actual: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected {}, got {}",
            self.what, self.expected, self.actual
        )
    }
}

/// The result of running one compiled program.
pub struct Run {
    pub label: String,
    pub output: Output,
    /// The child exceeded its per-case wall-clock cap and was killed. Its exit status is then the
    /// signal, not the program's verdict, so every check must report the timeout instead.
    pub timed_out: bool,
}

impl Run {
    pub fn new(label: impl Into<String>, output: Output) -> Run {
        Run {
            label: label.into(),
            output,
            timed_out: false,
        }
    }

    pub fn stdout(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.output.stdout)
    }

    pub fn stderr(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.output.stderr)
    }

    /// The stdout lines that are NOT part of a counter dump — a program's real output.
    pub fn payload_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut inside = false;
        for line in self.stdout().lines() {
            match line {
                "#db-counters-begin" => inside = true,
                "#db-counters-end" => inside = false,
                _ if !inside => out.push(line.to_string()),
                _ => {}
            }
        }
        out
    }

    /// The full failure context: label, RAW status, stdout, stderr.
    ///
    /// The raw status matters. Twelve of the assertion sites this replaces printed only stderr and
    /// eighteen dropped the status entirely, so a child killed by a signal — where `code()` is
    /// `None` and stderr is usually empty — reported nothing but `left != right`.
    pub fn describe(&self) -> String {
        format!(
            "run `{}`{}\n  status: {:?} (code {:?})\n  stdout: {}\n  stderr: {}",
            self.label,
            if self.timed_out { " [TIMED OUT]" } else { "" },
            self.output.status,
            self.output.status.code(),
            self.stdout(),
            self.stderr(),
        )
    }

    /// Non-panicking exit-code check. This is the primitive; table iteration accumulates these so
    /// one run reports every failing row instead of aborting at the first.
    pub fn check_exit(&self, code: i32) -> Result<(), Mismatch> {
        if self.timed_out {
            return Err(Mismatch {
                what: format!("exit code of `{}`", self.label),
                expected: format!("Some({code})"),
                actual: "the case timed out and was killed".to_string(),
            });
        }
        if self.output.status.code() == Some(code) {
            return Ok(());
        }
        Err(Mismatch {
            what: format!("exit code of `{}`", self.label),
            expected: format!("Some({code})"),
            actual: format!("{:?}", self.output.status.code()),
        })
    }

    /// Panicking wrapper over [`Run::check_exit`] for single-shot call sites.
    pub fn expect_exit(&self, code: i32) -> &Run {
        if let Err(mismatch) = self.check_exit(code) {
            panic!("{mismatch}\n{}", self.describe());
        }
        self
    }

}

/// What a test needs from the host before it can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Needs {
    /// An LLVM backend (compile + link + run).
    Backend,
    /// A backend and a C compiler (a linked C fixture).
    BackendAndCc,
    /// A backend, a C compiler, and a configured live PostgreSQL.
    LivePostgres,
}

impl Needs {
    pub fn id(self) -> &'static str {
        match self {
            Needs::Backend => "backend",
            Needs::BackendAndCc => "backend+cc",
            Needs::LivePostgres => "live-postgres",
        }
    }
}

/// Proof that the host satisfies a [`Needs`].
///
/// Deliberately empty: a live case reads its URL from [`live_postgres_decision`], which is also
/// what the required-mode owner tests, so there is no second copy to keep in step.
pub struct Gate;

/// What to do about a live-PostgreSQL case, given the two environment inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveDecision {
    Run(String),
    Skip,
    /// Required mode with no URL: the required CI job exists so this is a failure.
    Fail,
}

/// The live-PostgreSQL decision as a PURE function of its inputs.
///
/// Split out so it can be tested without `set_var`. Mutating process-global environment from a
/// parallel test is the recorded `test-global-state-isolation` finding, so the harness must never
/// require it in order to be testable.
pub fn live_postgres_decision(required: bool, url: Option<&str>) -> LiveDecision {
    match url.filter(|u| !u.is_empty()) {
        Some(url) => LiveDecision::Run(url.to_string()),
        None if required => LiveDecision::Fail,
        None => LiveDecision::Skip,
    }
}

/// Whether the engine must clear `key`, given the keys it is setting explicitly on this child.
///
/// `explicitly_set` is the caller's actual `env(...)` key list, not a restatement of it: the engine
/// builds the child's environment first and then derives the removals from what it just set. That
/// way adding a variable to a child cannot desynchronise from a hardcoded exclusion list here —
/// a stray `ALIGN_DB_*` in the developer's shell is still removed, and a key the engine itself set
/// is never removed on top of setting it.
///
/// Pure for the same reason as [`live_postgres_decision`]: mutating process-global environment from
/// a parallel test is the recorded `test-global-state-isolation` finding, so neither rule may need
/// `set_var` in order to be testable.
pub fn should_clear_env(key: &str, explicitly_set: &[&str]) -> bool {
    key.starts_with("ALIGN_DB_") && !explicitly_set.contains(&key)
}

/// Gate a test on host capability.
///
/// Returns `None` when the test should be skipped. `Needs::LivePostgres` under
/// `ALIGN_DB_POSTGRES_REQUIRED=1` **asserts** instead of skipping: the required CI job exists
/// precisely so that a missing database is a failure, and a silent skip there would move first
/// execution to CI (`docs/impl/16-test-policy.md`, "Service-dependent required suites run
/// locally first").
pub fn gate(needs: Needs) -> Option<Gate> {
    if !backend_available() {
        return None;
    }
    if matches!(needs, Needs::BackendAndCc | Needs::LivePostgres) && !cc_available() {
        return None;
    }
    if needs == Needs::LivePostgres {
        let required = std::env::var("ALIGN_DB_POSTGRES_REQUIRED").ok().as_deref() == Some("1");
        let url = std::env::var("ALIGN_DB_POSTGRES_URL").ok();
        return match live_postgres_decision(required, url.as_deref()) {
            LiveDecision::Run(_) => Some(Gate),
            LiveDecision::Skip => {
                eprintln!("live PostgreSQL case skipped: ALIGN_DB_POSTGRES_URL is not set");
                None
            }
            LiveDecision::Fail => {
                panic!("ALIGN_DB_POSTGRES_REQUIRED=1 requires ALIGN_DB_POSTGRES_URL")
            }
        };
    }
    Some(Gate)
}

/// Assert an accumulated failure list is empty, reporting EVERY entry.
pub fn assert_no_mismatches(context: &str, mismatches: &[Mismatch]) {
    if mismatches.is_empty() {
        return;
    }
    let body = mismatches
        .iter()
        .map(|m| format!("  - {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    panic!(
        "{context}: {} expectation(s) failed\n{body}",
        mismatches.len()
    );
}
