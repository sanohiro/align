//! One end-to-end case as a single record.
//!
//! A case has to be *executed* by a `#[test]` and *fingerprinted* by the golden owner. If those two
//! read separate copies of "which runner, which stubs, which exit code", the golden proves only
//! that the copy still matches itself — a runner swap or a changed expected code would sail through
//! it. So the record is written once and both consumers derive from it: [`Case::run`] executes
//! exactly what [`Case::fingerprint`] hashes.

use super::fingerprint::CaseFingerprint;
use super::layout::Layout;
use super::counters::CounterExpect;
use super::run::{Needs, gate};
use super::stubs::Stub;
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

/// A compiled-and-run owner case.
pub struct Case {
    pub label: &'static str,
    pub runner: RunnerKind,
    pub needs: Needs,
    /// Native stubs whose C source is linked.
    pub links: &'static [&'static Stub],
    /// Stubs whose Align counters module is also added. Must be a subset of `links`; the layout
    /// builder enforces it, so a counters module can never reach a program without its definitions.
    pub counters: &'static [&'static Stub],
    /// Suite modules beyond the `pkg.db` package, in layout order.
    ///
    /// A function rather than a slice so a suite whose modules are read from disk at run time
    /// (`common::fixture`) can still declare its cases as `const`.
    pub modules: fn() -> Vec<(&'static str, &'static str)>,
    pub main: &'static str,
    pub expected_exit: i32,
    /// Native counter expectations asserted after a successful run, as `(name, value)`.
    ///
    /// Part of the record rather than the test body so the fingerprint covers them: silently
    /// relaxing an expected call count is exactly the kind of change a golden should catch.
    pub expect_counters: &'static [(&'static str, i64)],
}

impl Case {
    /// The exact layout both [`Case::run`] and [`Case::fingerprint`] use.
    pub fn layout(&self) -> Layout {
        let mut layout = Layout::new();
        for (path, source) in (self.modules)() {
            layout = layout.module(path, source);
        }
        for stub in self.links {
            layout = layout.linking(stub);
        }
        for stub in self.counters {
            assert!(
                self.links.iter().any(|linked| std::ptr::eq(*linked, *stub)),
                "`{}` asks for {}'s counters module without linking its C source",
                self.label,
                stub.id
            );
            layout = layout.with_counters(stub);
        }
        layout.main(self.main)
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
        if !self.expect_counters.is_empty() {
            let mut expect = CounterExpect::new();
            for (name, value) in self.expect_counters {
                expect = expect.eq(name, *value);
            }
            expect.assert(&run);
        }
    }

    /// The fingerprint of what [`Case::run`] would do — derived from the same fields, never
    /// restated. `links` and `counters` ride in the environment slot because they select linked
    /// native code and added modules, so they change what the case exercises exactly as a runner
    /// swap does.
    pub fn fingerprint(&self) -> CaseFingerprint {
        CaseFingerprint::new(self.label, self.runner.id())
            .files(&self.layout().test_owned_files())
            .env(&[
                ("links", &stub_ids(self.links)),
                ("counters", &stub_ids(self.counters)),
                ("needs", self.needs.id()),
                ("expect_counters", &counter_ids(self.expect_counters)),
            ])
            .expected_exit(self.expected_exit)
    }
}

/// Stable identity for a stub list, for the fingerprint.
fn stub_ids(stubs: &[&'static Stub]) -> String {
    stubs
        .iter()
        .map(|stub| stub.id)
        .collect::<Vec<_>>()
        .join("+")
}

/// Stable identity for a counter expectation list, for the fingerprint.
fn counter_ids(pairs: &[(&'static str, i64)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}
