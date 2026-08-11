//! Shared owner-test harness for the `pkg.db` end-to-end suites
//! (`docs/impl/16-test-policy.md`, "pkg.db owner-test harness").
//!
//! # Why this module exists
//!
//! Every `pkg.db` E2E suite used to hand-write the same four things: the eight-module package
//! layout, a tool-availability guard, an exit-code assertion, and a trailing block of native
//! stub-counter checks encoded as hand-numbered sentinel exit codes. Those four blocks accounted for
//! ~1,300 duplicated lines across the corpus, the exit assertion had drifted into four
//! incompatible shapes, and — the reason this is machinery rather than tidying — the two members of
//! each dual-driver pair were kept identical **by hand**. The recorded
//! `operation-matrix-completeness` finding class
//! (`.claude/skills/align-self-review/FINDINGS.md`, 4 events / 3 PRs) is exactly that drift, and it
//! is past the three-event threshold where CLAUDE.md requires a mechanism instead of a checklist
//! line. [`parity`] is that mechanism.
//!
//! # Module contract
//!
//! This module is **not** part of `tests/common/`. It is included only by the `pkg_db_*` E2E
//! suites, so editing it rebuilds those binaries rather than all ~167 driver test binaries. It
//! depends on `common` being declared in the including binary, and refers to it as
//! `crate::common::…`; every including file therefore keeps
//!
//! ```ignore
//! mod common;
//! use common::*;
//! mod db_harness;
//! ```
//!
//! in that order.
//!
//! `dead_code` / `unused_imports` are allowed for the same reason `common/mod.rs` allows them: each
//! test binary compiles this whole module but uses only the subset it needs, and the subsets
//! genuinely differ — q5b2 uses only `package_source`, q4b uses the parity engine, q4a and q6 use
//! the counter tables. Do not "clean up" the attribute; without it every suite would need a
//! hand-maintained import list that says nothing about correctness.
//!
//! Cargo auto-discovers only `tests/*.rs` and `tests/*/main.rs`, so this directory is a shared
//! module and not an extra test binary (no extra link, no extra process startup).
#![allow(dead_code, unused_imports)]

pub mod case;
pub mod counters;
pub mod fingerprint;
pub mod layout;
pub mod parity;
pub mod run;
pub mod stubs;
pub mod runner;

pub use case::{Case, RunnerKind};
pub use counters::{CounterExpect, Counters};
pub use fingerprint::{CaseFingerprint, FingerprintLog};
pub use layout::{Layout, package_source};
pub use stubs::{PG, SQLITE_Q4A, SQLITE_Q6, Stub};
pub use parity::{
    Driver, Expect, Limits, ParityCase, ParityProgram, run_of, run_parity,
    run_parity_with_limits,
};
pub use run::{
    Gate, LiveDecision, Mismatch, Needs, Run, assert_no_mismatches, gate,
    live_postgres_decision, should_clear_env,
};
pub use runner::{
    RUNNER_PER_UNIT_C, RUNNER_STATIC_DESCRIPTORS, RUNNER_WHOLE_PROGRAM, expect_checks_clean,
    expect_checks_rejected, run_per_unit_c, run_static_descriptors, run_whole_program,
};
