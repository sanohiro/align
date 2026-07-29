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
pub fn consume_try_success(value: Result<str, Error>) -> Result<i64, Error> {
  selected := value?
  return Ok(selected.len())
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
        find("consume_try_success").return_borrow,
        ReturnBorrowSummary::None,
        "a borrowing Ok payload must not put provenance on a non-borrowing Result return"
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
fn caller_before_callee_and_mutual_recursion_converge_independently_of_declaration_order() {
    let files = &[
        (
            "views.align",
            "\
module views
pub fn caller(first: str, second: str, depth: i64) -> str =
  even(first, second, depth)
pub fn even(first: str, second: str, depth: i64) -> str {
  if depth == 0 { return first }
  return odd(first, second, depth - 1)
}
pub fn odd(first: str, second: str, depth: i64) -> str {
  if depth == 0 { return second }
  return even(first, second, depth - 1)
}
",
        ),
        ("main.align", "import views\nfn main() -> i32 = 0\n"),
    ];
    let differential =
        diff_check_multi("l2b-return-summary-worklist", files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "verdict mismatch:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.per_unit.diags.has_errors(),
        "worklist fixture must check:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    let summary = differential
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "views")
        .expect("views summary");
    for name in ["caller", "even", "odd"] {
        let function = summary
            .fns
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("{name} signature"));
        assert_eq!(
            function.return_borrow,
            roots(&[0, 1], &[]),
            "{name} must converge to both mutually reachable parameter roots"
        );
        assert_eq!(
            function.return_region,
            ReturnRegionSummary::Roots {
                params: vec![0, 1],
                captures: vec![],
            }
        );
    }
}

#[test]
fn unreachable_return_and_break_edges_do_not_taint_provenance() {
    let direct = "\
fn fixed_return(value: str) -> str {
  return \"fixed\"
  return value
}
fn fixed_break(value: str) -> str = loop {
  break \"fixed\"
  break value
}
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"return owner\".clone()
  returned := fixed_return(first)
  consume(first)
  print(returned.len())

  second := \"break owner\".clone()
  broken := fixed_break(second)
  consume(second)
  return broken.len() as i32
}
";
    assert!(
        !check_errs("l2b-unreachable-exits-direct", direct),
        "dead returns and breaks must not retain an otherwise-unselected owner"
    );

    let files = &[
        (
            "dep.align",
            "\
module dep
pub fn fixed_return(value: str) -> str {
  return \"fixed\"
  return value
}
pub fn fixed_break(value: str) -> str = loop {
  break \"fixed\"
  break value
}
",
        ),
        (
            "main.align",
            "\
import dep
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"return owner\".clone()
  returned := dep.fixed_return(first)
  consume(first)
  print(returned.len())

  second := \"break owner\".clone()
  broken := dep.fixed_break(second)
  consume(second)
  return broken.len() as i32
}
",
        ),
    ];
    let checked =
        assert_same_verdict("l2b-unreachable-exits-imported", files, "main.align");
    assert!(
        !checked.diags.has_errors(),
        "whole-program and per-unit summaries must exclude dead exits"
    );
    let dependency = checked
        .summaries
        .iter()
        .find(|summary| summary.unit == "dep")
        .expect("dependency summary");
    for name in ["fixed_return", "fixed_break"] {
        let function = dependency
            .fns
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("{name} signature"));
        assert_eq!(function.return_borrow, ReturnBorrowSummary::None);
        assert_eq!(function.return_region, ReturnRegionSummary::None);
    }
}

#[test]
fn ordinary_exported_lambda_suffix_is_not_a_lifted_function() {
    let files = &[
        (
            "dep.align",
            "\
module dep
pub fn lambda0(value: str) -> str = value
",
        ),
        (
            "main.align",
            "\
import dep
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"named lambda suffix\".clone()
  result := dep.lambda0(owned)
  consume(owned)
  return result.len() as i32
}
",
        ),
    ];
    let checked =
        assert_same_verdict("l2b-named-lambda-suffix", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "the named dependency result must retain its owner in both compilation modes"
    );
    let dependency = checked
        .summaries
        .iter()
        .find(|summary| summary.unit == "dep")
        .expect("dependency summary");
    let function = dependency
        .fns
        .iter()
        .find(|function| function.name == "lambda0")
        .expect("ordinary exported lambda0");
    assert_eq!(function.return_borrow, roots(&[0], &[]));
    assert_eq!(
        function.return_region,
        ReturnRegionSummary::Roots {
            params: vec![0],
            captures: vec![],
        }
    );
}

#[test]
fn compiler_produced_builtin_name_collisions_import_in_both_modes() {
    let files = &[
        (
            "names.align",
            "\
module names
pub Option { value: i64 }
pub Task { value: i64 }
pub Combined {
  builtin_value: Option<str>,
  local_option: names.Option,
  local_task: Task,
}
",
        ),
        (
            "params.align",
            "\
module params
pub BuiltinHolder<Option> { value: Option<str> }
pub BuiltinChoice<Option> { Some(Option<str>) }
pub fn builtin<Option>(value: Option<str>) -> Option<str> = value
",
        ),
        (
            "main.align",
            "import names\nimport params\nfn main() -> i32 = 0\n",
        ),
    ];
    let differential =
        diff_check_multi("l2b-builtin-name-collisions", files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "verdict mismatch:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.per_unit.diags.has_errors(),
        "compiler-produced public definitions must remain importable:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
}

#[test]
fn producer_and_importer_reject_generic_parameter_duplicates_and_shadowing() {
    let cases = [
        (
            "fn-shadow",
            "pub Local { value: i64 }\npub fn bad<Local>(value: Local) -> Local = value\n",
            "type parameter 'Local' shadows a declared type",
        ),
        (
            "struct-shadow",
            "pub Local { value: i64 }\npub Holder<Local> { value: Local }\n",
            "type parameter 'Local' shadows a declared type",
        ),
        (
            "enum-shadow",
            "pub Local { value: i64 }\npub Choice<Local> { Some(Local) }\n",
            "type parameter 'Local' shadows a declared type",
        ),
        (
            "fn-duplicate",
            "pub fn bad<T, T>(value: T) -> T = value\n",
            "duplicate type parameter 'T'",
        ),
        (
            "struct-duplicate",
            "pub Pair<T, T> { value: T }\n",
            "duplicate type parameter 'T'",
        ),
        (
            "enum-duplicate",
            "pub Choice<T, T> { Some(T) }\n",
            "duplicate type parameter 'T'",
        ),
        (
            "duplicate-before-shadow",
            "pub T { value: i64 }\npub Pair<T, T> { value: T }\n",
            "duplicate type parameter 'T'",
        ),
    ];
    for (case, declaration, message) in cases {
        let dependency = format!("module bad\n{declaration}");
        let files = [
            ("bad.align", dependency.as_str()),
            ("main.align", "import bad\nfn main() -> i32 = 0\n"),
        ];
        let differential = diff_check_multi(
            &format!("l2b-generic-param-{case}"),
            &files,
            "main.align",
        );
        assert!(
            differential.whole_errors && differential.per_unit_errors,
            "{case} must reject in both modes:\nwhole:\n{}\nper-unit:\n{}",
            differential.whole_diags,
            differential.per_unit_diags
        );
        assert!(
            differential.whole_diags.contains(message)
                && differential.per_unit_diags.contains(message),
            "{case} must preserve the owner diagnostic:\nwhole:\n{}\nper-unit:\n{}",
            differential.whole_diags,
            differential.per_unit_diags
        );
        if case == "duplicate-before-shadow" {
            assert!(
                !differential
                    .whole_diags
                    .contains("shadows a declared type")
                    && !differential
                        .per_unit_diags
                        .contains("shadows a declared type"),
                "duplicate validation must suppress the later shadow class"
            );
        }
    }
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
