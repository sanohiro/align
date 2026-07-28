//! L2b-a1 gate: return-borrow/region summaries retain caller-relative parameter roots across
//! named, direct, recursive, and imported call paths while aggregates stay conservatively flat.

mod common;
use common::*;

use align_interface::{ReturnBorrowSummary, ReturnRegionSummary};

fn roots(params: &[u32], captures: &[u32]) -> ReturnBorrowSummary {
    ReturnBorrowSummary::Roots {
        params: params.to_vec(),
        captures: captures.to_vec(),
    }
}

#[test]
fn direct_return_summaries_cover_scalar_recursion_and_flattened_aggregates() {
    let files = &[
        (
            "views.align",
            "\
module views
pub BoxedView { value: str }
pub Choice { First(str), Second(str) }
pub fn second(first: str, second: str) -> str = second
pub fn boxed(value: str, ignored: str) -> BoxedView =
  BoxedView { value: value }
pub fn branch(left: str, right: str, take_left: bool) -> str =
  if take_left { left } else { right }
pub fn recursive(value: str, depth: i64) -> str {
  if depth == 0 { return value }
  return recursive(value, depth - 1)
}
pub fn loop_identity(value: str) -> str = loop { break value }
pub fn propagate(value: Result<str, Error>, fallback: str) -> Result<str, Error> {
  selected := value?
  return Ok(fallback)
}
pub fn choose(first: str, second: str, take_first: bool) -> str {
  value := if take_first { Choice.First(first) } else { Choice.Second(second) }
  return match value {
    First(selected) => selected
    Second(_) => \"fixed\"
  }
}
",
        ),
        ("main.align", "import views\nfn main() -> i32 = 0\n"),
    ];
    let differential = diff_check_multi("l2b-direct-summaries", files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "verdict mismatch:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.per_unit.diags.has_errors(),
        "direct summary fixture must check:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    let checked = differential.per_unit;
    let summary = checked
        .summaries
        .iter()
        .find(|summary| summary.unit == "views")
        .expect("views summary");
    let find = |name: &str| {
        summary
            .fns
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("{name} signature"))
    };
    assert_eq!(find("second").return_borrow, roots(&[1], &[]));
    assert_eq!(find("boxed").return_borrow, roots(&[0], &[]));
    assert_eq!(find("branch").return_borrow, roots(&[0, 1], &[]));
    assert_eq!(find("recursive").return_borrow, roots(&[0], &[]));
    assert_eq!(find("loop_identity").return_borrow, roots(&[0], &[]));
    assert_eq!(
        find("propagate").return_borrow,
        roots(&[0, 1], &[]),
        "L2b-a1 conservatively flattens the implicit Err edge and continuing Ok value"
    );
    assert_eq!(
        find("choose").return_borrow,
        roots(&[0, 1], &[]),
        "L2b-a1 deliberately retains the flattened sum-payload union"
    );
    assert_eq!(
        find("second").return_region,
        ReturnRegionSummary::Roots {
            params: vec![1],
            captures: vec![],
        }
    );
}

#[test]
fn loop_break_value_roots_drive_direct_and_imported_liveness() {
    let direct = "\
fn loop_identity(value: str) -> str = loop { break value }
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"direct loop\".clone()
  result := loop_identity(owned)
  consume(owned)
  return result.len() as i32
}
";
    assert!(
        check_errs("l2b-loop-direct", direct),
        "a loop break value must retain its direct argument owner"
    );

    let files = &[
        (
            "views.align",
            "module views\npub fn loop_identity(value: str) -> str = loop { break value }\n",
        ),
        (
            "main.align",
            "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"imported loop\".clone()
  result := views.loop_identity(owned)
  consume(owned)
  return result.len() as i32
}
",
        ),
    ];
    let checked = assert_same_verdict("l2b-loop-imported", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "the imported loop summary must retain the selected argument owner"
    );
}

#[test]
fn unrelated_direct_argument_no_longer_taints_call_result() {
    let src = "\
fn fixed(input: str) -> str = \"fixed\"
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"temporary\".clone()
  view: str := owned
  result := fixed(view)
  consume(owned)
  return result.len() as i32
}
";
    assert!(
        !check_errs("l2b-direct-precision", src),
        "a result independent of its argument must survive that argument's owner move"
    );
}

#[test]
fn direct_region_summary_ignores_unselected_arena_argument() {
    let src = "\
fn fixed(input: str) -> str = \"fixed\"
fn wrapper(seed: str) -> str {
  arena {
    local := template \"{seed}\"
    return fixed(local)
  }
}
fn main() -> i32 = 0
";
    assert!(
        !check_errs("l2b-direct-region-fixed", src),
        "a fixed result must not inherit an unrelated arena argument"
    );
}

#[test]
fn direct_region_summary_retains_selected_arena_argument() {
    let src = "\
fn identity(input: str) -> str = input
fn wrapper(seed: str) -> str {
  arena {
    local := template \"{seed}\"
    return identity(local)
  }
}
fn main() -> i32 = 0
";
    assert!(
        check_errs("l2b-direct-region-identity", src),
        "an identity result must not outlive its selected arena argument"
    );
}

#[test]
fn direct_result_keeps_the_selected_argument_generation() {
    let src = "\
fn second(first: str, second: str) -> str = second
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second_value := \"second\".clone()
  result := second(first, second_value)
  consume(first)
  print(result.len())
  consume(second_value)
  return result.len() as i32
}
";
    assert!(
        check_errs("l2b-direct-selected-root", src),
        "the result must be invalidated by the selected second owner"
    );
}

#[test]
fn imported_summary_matches_whole_program_and_drives_liveness() {
    let files = &[
        (
            "views.align",
            "module views\npub fn second(first: str, second: str) -> str = second\n",
        ),
        (
            "main.align",
            "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  result := views.second(first, second)
  consume(second)
  return result.len() as i32
}
",
        ),
    ];
    let checked = assert_same_verdict("l2b-imported-summary", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "whole-program and per-unit consumers must reject the stale imported result"
    );
    let views = checked
        .summaries
        .iter()
        .find(|summary| summary.unit == "views")
        .expect("views summary");
    let second = views
        .fns
        .iter()
        .find(|function| function.name == "second")
        .expect("second signature");
    assert_eq!(second.return_borrow, roots(&[1], &[]));
}

#[test]
fn compatibility_api_keeps_unknown_interface_provenance_conservative() {
    let dependency =
        "module dep\npub fn identity(value: str) -> str {}\n";
    let consumer = "\
import dep
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"compatibility\".clone()
  result := dep.identity(owned)
  consume(owned)
  return result.len() as i32
}
";
    let mut diags = align_diag::Diagnostics::new();
    let dependency_tokens = align_lexer::tokenize(0, dependency, &mut diags);
    let dependency_file = align_parser::parse_file(dependency_tokens, &mut diags);
    let consumer_tokens = align_lexer::tokenize(1, consumer, &mut diags);
    let consumer_file = align_parser::parse_file(consumer_tokens, &mut diags);
    assert!(!diags.has_errors(), "compatibility fixture must parse");
    let modules = [
        align_sema::Module {
            path: "dep".to_string(),
            file: &dependency_file,
            is_entry: false,
            interface_only: true,
        },
        align_sema::Module {
            path: "main".to_string(),
            file: &consumer_file,
            is_entry: true,
            interface_only: false,
        },
    ];
    align_sema::check_program_with_effects(
        &modules,
        &std::collections::HashMap::new(),
        &mut diags,
    );
    assert!(
        diags.has_errors(),
        "an interface-only import without explicit provenance facts must retain the all-compatible-input fallback"
    );
}

#[test]
fn foreign_qualified_nominal_is_not_confused_with_a_local_name_collision() {
    let files = &[
        (
            "dep.align",
            "\
module dep
pub Payload { value: str }
",
        ),
        (
            "wrapper.align",
            "\
module wrapper
import dep
pub Payload { value: i64 }
pub fn identity(value: dep.Payload) -> dep.Payload = value
",
        ),
        (
            "main.align",
            "import dep\nimport wrapper\nfn main() -> i32 = 0\n",
        ),
    ];
    let differential = diff_check_multi("l2b-qualified-nominal-collision", files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "verdict mismatch:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.per_unit.diags.has_errors(),
        "a foreign qualified borrowing type must not resolve to the wrapper's scalar local type:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    let wrapper = differential
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "wrapper")
        .expect("wrapper summary");
    let identity = wrapper
        .fns
        .iter()
        .find(|function| function.name == "identity")
        .expect("identity signature");
    assert_eq!(identity.return_borrow, roots(&[0], &[]));
}

#[test]
fn imported_region_summary_does_not_taint_a_fixed_result() {
    let files = &[
        (
            "views.align",
            "module views\npub fn fixed(input: str) -> str = \"fixed\"\n",
        ),
        (
            "main.align",
            "\
import views
fn wrapper(seed: str) -> str {
  arena {
    local := template \"{seed}\"
    return views.fixed(local)
  }
}
fn main() -> i32 = 0
",
        ),
    ];
    let checked = assert_same_verdict("l2b-imported-region", files, "main.align");
    assert!(
        !checked.diags.has_errors(),
        "an imported fixed result must not inherit an unrelated arena argument"
    );
}

#[test]
fn generic_monomorph_infers_provenance_in_the_consumer() {
    let files = &[
        (
            "generic.align",
            "module generic\npub fn identity<T>(value: T) -> T = value\n",
        ),
        (
            "main.align",
            "\
import generic
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"generic\".clone()
  view: str := owned
  result := generic.identity(view)
  consume(owned)
  return result.len() as i32
}
",
        ),
    ];
    let checked = assert_same_verdict("l2b-generic-monomorph", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "the consumer-instantiated identity must retain its argument root"
    );
}

#[test]
fn lifted_closure_capture_roots_remain_deferred_to_l2b_b() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  owned := \"captured\".clone()
  view: str := owned
  f := fn unused: i64 { view }
  result := f(0)
  return result.len() as i32
}
";
    let output = build_and_run("l2b-a1-lifted-capture", src);
    assert_eq!(output.status.code(), Some(8));
}

#[test]
fn named_function_value_summaries_remain_deferred_to_l2b_b() {
    if !backend_available() {
        return;
    }
    let src = "\
fn identity(value: str) -> str = value
fn main() -> i32 {
  owned := \"function value\".clone()
  view: str := owned
  f := identity
  result := f(view)
  return result.len() as i32
}
";
    let output = build_and_run("l2b-a1-named-fn-value", src);
    assert_eq!(output.status.code(), Some(14));
}
