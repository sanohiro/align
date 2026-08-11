//! One end-to-end case as a single record.
//!
//! A case has to be *executed* by a `#[test]` and *fingerprinted* by the golden owner. If those two
//! read separate copies of "which runner, which stubs, which exit code", the golden proves only
//! that the copy still matches itself — a runner swap or a changed expected code would sail through
//! it. So the record is written once and both consumers derive from it: [`Case::run`] executes
//! exactly what [`Case::fingerprint`] hashes.

use super::fingerprint::CaseFingerprint;
use super::layout::Layout;
use super::run::{Needs, gate};
use super::runner::{run_per_unit_c, run_static_descriptors};

/// Which compile/execute pipeline a case uses. Part of what the case proves, so it is part of the
/// fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerKind {
    /// Whole-program front end + compiler-installed static descriptors; cannot link a C fixture.
    StaticDescriptors,
    /// Per-unit walk + a linked C fixture; the only pipeline that can substitute the libpq stub.
    PerUnitC,
}

impl RunnerKind {
    pub fn id(self) -> &'static str {
        match self {
            RunnerKind::StaticDescriptors => super::runner::RUNNER_STATIC_DESCRIPTORS,
            RunnerKind::PerUnitC => super::runner::RUNNER_PER_UNIT_C,
        }
    }
}

/// Which native stubs a case links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stubs {
    None,
    Pg,
    PgAndSqlite,
}

impl Stubs {
    pub fn id(self) -> &'static str {
        match self {
            Stubs::None => "none",
            Stubs::Pg => "pg",
            Stubs::PgAndSqlite => "pg+sqlite",
        }
    }

    fn apply(self, layout: Layout) -> Layout {
        match self {
            Stubs::None => layout,
            Stubs::Pg => layout.linking_pg_stub(),
            Stubs::PgAndSqlite => layout.linking_pg_stub().linking_sqlite_stub(),
        }
    }
}

/// A compiled-and-run owner case.
pub struct Case {
    pub label: &'static str,
    pub runner: RunnerKind,
    pub needs: Needs,
    pub stubs: Stubs,
    /// Suite modules beyond the `pkg.db` package, in layout order.
    pub modules: &'static [(&'static str, &'static str)],
    pub main: &'static str,
    pub expected_exit: i32,
}

impl Case {
    /// The exact layout both [`Case::run`] and [`Case::fingerprint`] use.
    pub fn layout(&self) -> Layout {
        let mut layout = Layout::new();
        for (path, source) in self.modules {
            layout = layout.module(path, source);
        }
        self.stubs.apply(layout).main(self.main)
    }

    /// Gate, compile, run, and require the declared exit code.
    pub fn run(&self) {
        if gate(self.needs).is_none() {
            return;
        }
        let layout = self.layout();
        let run = match self.runner {
            RunnerKind::StaticDescriptors => run_static_descriptors(self.label, &layout),
            RunnerKind::PerUnitC => run_per_unit_c(self.label, &layout),
        };
        run.expect_exit(self.expected_exit);
    }

    /// The fingerprint of what [`Case::run`] would do — derived from the same fields, never
    /// restated. `stubs` rides in the environment slot because it selects linked native code and so
    /// changes what the case exercises, exactly like a runner swap does.
    pub fn fingerprint(&self) -> CaseFingerprint {
        CaseFingerprint::new(self.label, self.runner.id())
            .files(&self.layout().test_owned_files())
            .env(&[("stubs", self.stubs.id()), ("needs", self.needs.id())])
            .expected_exit(self.expected_exit)
    }
}
