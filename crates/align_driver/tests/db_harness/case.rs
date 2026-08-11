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
    /// Whole-program front end without static descriptors.
    WholeProgram,
}

impl RunnerKind {
    pub fn id(self) -> &'static str {
        match self {
            RunnerKind::StaticDescriptors => super::runner::RUNNER_STATIC_DESCRIPTORS,
            RunnerKind::PerUnitC => super::runner::RUNNER_PER_UNIT_C,
            RunnerKind::WholeProgram => super::runner::RUNNER_WHOLE_PROGRAM,
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
    /// Environment applied to the child. Part of the record, so it lands in the fingerprint: a
    /// variable added or changed alters what the case exercises.
    pub envs: &'static [(&'static str, &'static str)],
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
        // Adding a counters module without expecting anything from it compiles and links happily
        // while asserting nothing, which looks like coverage and is not. The converse — expecting
        // counters with no module to print them — fails at parse time, but failing here names the
        // case instead.
        assert_eq!(
            self.counters.is_empty(),
            self.expect_counters.is_empty(),
            "`{}` links {} counters module(s) but declares {} counter expectation(s); a counters \
             module with nothing asserted against it is not coverage",
            self.label,
            self.counters.len(),
            self.expect_counters.len(),
        );
        let mut layout = Layout::new();
        for (path, source) in (self.modules)() {
            layout = layout.module(path, source);
        }
        for stub in self.links {
            layout = layout.linking(stub);
        }
        for stub in self.counters {
            assert!(
                self.links.iter().any(|linked| linked.id == stub.id),
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
            RunnerKind::StaticDescriptors => {
                assert!(
                    self.envs.is_empty(),
                    "`{}` declares a child environment, which only the whole-program runner \
                     applies; the descriptor runner would silently drop it",
                    self.label
                );
                run_static_descriptors(self.label, &layout)
            }
            RunnerKind::PerUnitC => {
                assert!(
                    self.envs.is_empty(),
                    "`{}` declares a child environment, which only the whole-program runner \
                     applies; the per-unit runner would silently drop it",
                    self.label
                );
                run_per_unit_c(self.label, &layout)
            }
            RunnerKind::WholeProgram => {
                super::runner::run_whole_program(self.label, &layout, self.envs)
            }
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
        let mut attributes: Vec<(String, String)> = vec![("needs".to_string(), self.needs.id().to_string())];
        push_indexed(&mut attributes, "links", self.links.iter().map(|s| s.id.to_string()));
        push_indexed(&mut attributes, "counters", self.counters.iter().map(|s| s.id.to_string()));
        for (name, value) in self.expect_counters {
            attributes.push((format!("expect_counter.{name}"), value.to_string()));
        }
        for (key, value) in self.envs {
            attributes.push((format!("child_env.{key}"), (*value).to_string()));
        }
        CaseFingerprint::new(self.label, self.runner.id())
            .files(&self.layout().test_owned_files())
            .env_pairs(attributes)
            .expected_exit(self.expected_exit)
    }
}

/// Record a list attribute as one entry per element, so no join character can alias.
fn push_indexed(
    into: &mut Vec<(String, String)>,
    name: &str,
    values: impl Iterator<Item = String>,
) {
    for (index, value) in values.enumerate() {
        into.push((format!("{name}.{index}"), value));
    }
}
