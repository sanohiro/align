//! The compile/execute pipelines, named.
//!
//! Which pipeline a case uses is part of what the case proves, not an implementation detail:
//! `static_descriptors` exercises the whole-program front end plus
//! `lower_to_mir_with_static_descriptors`, while `per_unit_c` exercises the per-unit walk and links
//! a C fixture ahead of the capability libraries. Naming them here keeps the choice visible at the
//! call site and gives [`super::fingerprint`] a stable `runner_id` to hash.

use super::layout::Layout;
use super::run::Run;
use crate::common::{
    build_and_run_multi_args_with_env, build_and_run_multi_with_c,
    build_and_run_multi_with_static_descriptors, diff_check_multi,
};

/// Whole-program front end + compiler-installed static descriptors. No C fixture.
pub const RUNNER_STATIC_DESCRIPTORS: &str = "static_descriptors";
/// Per-unit walk + a linked C fixture (the only pipeline that can substitute the libpq stub).
pub const RUNNER_PER_UNIT_C: &str = "per_unit_c";
/// Whole-program front end WITHOUT compiler-installed static descriptors.
///
/// Distinct from `static_descriptors`: a program that never constructs a Query descriptor exercises
/// a different amount of the driver, so which of the two a case uses is part of what it proves.
pub const RUNNER_WHOLE_PROGRAM: &str = "whole_program";

/// Compile through the whole-program static-descriptor pipeline, run, and wrap the result.
pub fn run_static_descriptors(tag: &str, layout: &Layout) -> Run {
    assert!(
        !layout.has_c_fixture(),
        "`{tag}` declares a C fixture but uses the static-descriptor pipeline, which cannot link \
         one; use run_per_unit_c"
    );
    let output = build_and_run_multi_with_static_descriptors(tag, &layout.files(), "main.align");
    Run::new(tag, output)
}

/// Compile through the plain whole-program pipeline, run, and wrap.
pub fn run_whole_program(tag: &str, layout: &Layout, envs: &[(&str, &str)]) -> Run {
    assert!(
        !layout.has_c_fixture(),
        "`{tag}` declares a C fixture but uses the whole-program pipeline, which cannot link one; \
         use run_per_unit_c"
    );
    let output = build_and_run_multi_args_with_env(tag, &layout.files(), "main.align", &[], envs);
    Run::new(tag, output)
}

/// Compile through the per-unit pipeline with the layout's C fixture linked, run, and wrap.
pub fn run_per_unit_c(tag: &str, layout: &Layout) -> Run {
    let fixture = layout.c_fixture().unwrap_or_else(|| {
        panic!("`{tag}` uses the per-unit + C pipeline but declares no C fixture")
    });
    let output = build_and_run_multi_with_c(tag, &layout.files(), "main.align", &fixture);
    Run::new(tag, output)
}

/// Check the layout with BOTH front ends and assert they agree that it is clean.
pub fn expect_checks_clean(tag: &str, layout: &Layout) {
    let checked = diff_check_multi(tag, &layout.files(), "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "`{tag}` should check cleanly\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
}

/// Check the layout with BOTH front ends and assert they agree that it is rejected.
///
/// Both verdicts are required: a rule enforced by only one front end is a per-unit/whole-program
/// divergence, which is its own defect class.
pub fn expect_checks_rejected(tag: &str, layout: &Layout) {
    let checked = diff_check_multi(tag, &layout.files(), "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "`{tag}` should be rejected by BOTH front ends (whole={}, per_unit={})\nwhole:\n{}\nper-unit:\n{}",
        checked.whole_errors,
        checked.per_unit_errors,
        checked.whole_diags,
        checked.per_unit_diags,
    );
}
