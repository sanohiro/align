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
            .find(|function| function.name.as_str() == name)
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
fn product_projection_summaries_select_exact_parameter_roots() {
    let files = &[
        (
            "views.align",
            "\
module views
pub Inner { left: str, right: str }
pub Outer { inner: Inner, ignored: str }

pub fn nested_field(left: str, right: str, ignored: str) -> str {
  value := Outer {
    inner: Inner { left: left, right: right }
    ignored: ignored
  }
  return value.inner.left
}

pub fn replaced_field(left: str, right: str) -> str {
  mut value := Inner { left: left, right: right }
  value.left = right
  return value.left
}

pub fn replaced_sibling(left: str, right: str) -> str {
  mut value := Inner { left: left, right: right }
  value.left = \"fixed\"
  return value.right
}

pub fn replaced_nested_field(left: str, right: str) -> str {
  mut value := Outer {
    inner: Inner { left: left, right: \"fixed\" }
    ignored: \"fixed\"
  }
  value.inner.left = right
  return value.inner.left
}

pub fn whole_local_replacement(left: str, right: str) -> str {
  mut value := Inner { left: left, right: \"fixed\" }
  value = Inner { left: right, right: \"fixed\" }
  return value.left
}

pub fn field_self_assignment(left: str, right: str) -> str {
  mut value := Inner { left: left, right: right }
  value.left = value.left
  return value.left
}

pub fn tuple_index(left: str, right: str) -> str {
  value := (left, right)
  return value.0
}

pub fn struct_child_snapshot(first: str, second: str) -> str {
  mut current := first
  value := Inner {
    left: current
    right: {
      saved := current
      current = second
      saved
    }
  }
  return value.left
}

pub fn tuple_child_snapshot(first: str, second: str) -> str {
  mut current := first
  value := (
    current,
    {
      saved := current
      current = second
      saved
    },
  )
  return value.0
}

pub fn take_first(first: str, ignored: str) -> str = first

pub fn call_child_snapshot(first: str, second: str) -> str {
  mut current := first
  return take_first(
    current,
    {
      current = second
      current
    },
  )
}

pub fn tuple_binding(left: str, right: str) -> str {
  (selected, _) := (left, right)
  return selected
}

pub fn control_tuple(left: str, right: str, choose: bool) -> str {
  value := if choose { (left, \"fixed\") } else { (right, \"fixed\") }
  return value.0
}

pub fn branch_reassignment(
  left: str,
  right: str,
  then_value: str,
  else_value: str,
  choose: bool,
) -> str {
  mut value := Inner { left: left, right: right }
  return if choose {
    value.left = then_value
    value.left
  } else {
    value.right = else_value
    value.right
  }
}

pub fn branch_loop(left: str, right: str, choose: bool) -> str {
  value := loop {
    break if choose {
      Outer {
        inner: Inner { left: left, right: \"fixed\" }
        ignored: \"fixed\"
      }
    } else {
      Outer {
        inner: Inner { left: right, right: \"fixed\" }
        ignored: \"fixed\"
      }
    }
  }
  return value.inner.left
}

pub fn loop_reassignment(left: str, right: str, choose: bool) -> str {
  mut value := Inner { left: left, right: \"fixed\" }
  return loop {
    if choose {
      value.left = right
      break value.left
    }
    break value.left
  }
}

pub fn deferred_array_write(first: str, second: str) -> str {
  mut values := [\"fixed\", \"fixed\"]
  values[0] = first
  values[1] = second
  return values[0]
}

pub fn deferred_array_child_snapshot(first: str, second: str) -> str {
  mut current := first
  values := [
    current,
    {
      current = second
      current
    },
  ]
  return values[0]
}

pub fn deferred_element_field_write(first: str, second: str) -> str {
  mut values := [Inner { left: \"fixed\", right: \"fixed\" }]
  values[0].left = first
  values[0].right = second
  return values[0].left
}

pub fn deferred_pipeline_projection(first: str, second: str) -> str {
  values := [Inner { left: first, right: second }].left.to_array()
  return values[0]
}

",
        ),
        ("main.align", "import views\nfn main() -> i32 = 0\n"),
    ];
    let differential =
        diff_check_multi("l2b-a2-product-projections", files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "verdict mismatch:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.per_unit.diags.has_errors(),
        "product projection fixture must check:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    let summary = differential
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "views")
        .expect("views summary");
    let find = |name: &str| {
        summary
            .fns
            .iter()
            .find(|function| function.name.as_str() == name)
            .unwrap_or_else(|| panic!("{name} signature"))
    };
    for name in [
        "nested_field",
        "tuple_index",
        "struct_child_snapshot",
        "tuple_child_snapshot",
        "call_child_snapshot",
        "tuple_binding",
        "field_self_assignment",
    ] {
        assert_eq!(
            find(name).return_borrow,
            roots(&[0], &[]),
            "{name} must retain only the selected first parameter"
        );
    }
    for name in [
        "replaced_field",
        "replaced_sibling",
        "replaced_nested_field",
        "whole_local_replacement",
    ] {
        assert_eq!(
            find(name).return_borrow,
            roots(&[1], &[]),
            "{name} must retain only the installed/selected second parameter"
        );
    }
    for name in ["control_tuple", "branch_loop", "loop_reassignment"] {
        assert_eq!(
            find(name).return_borrow,
            roots(&[0, 1], &[]),
            "{name} must retain every runtime-selectable parameter"
        );
    }
    assert_eq!(
        find("branch_reassignment").return_borrow,
        roots(&[2, 3], &[]),
        "branch result facts must be captured before branch-local states join"
    );
    for name in [
        "deferred_array_write",
        "deferred_array_child_snapshot",
        "deferred_element_field_write",
        "deferred_pipeline_projection",
    ] {
        assert_eq!(
            find(name).return_borrow,
            roots(&[0, 1], &[]),
            "{name} must retain both roots until array/pipeline projection lands"
        );
    }
}

#[test]
fn source_order_snapshots_drive_imported_owner_liveness() {
    let producer = (
        "views.align",
        "\
module views
pub fn take_first(first: str, ignored: str) -> str = first
pub fn call_child_snapshot(first: str, second: str) -> str {
  mut current := first
  return take_first(
    current,
    {
      current = second
      current
    },
  )
}
pub fn array_child_snapshot(first: str, second: str) -> str {
  mut current := first
  values := [
    current,
    {
      current = second
      current
    },
  ]
  return values[0]
}
",
    );
    let unselected_named = assert_same_verdict(
        "l2b-a2-source-order-named-unselected-owner",
        &[
            producer,
            (
                "main.align",
                "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  result := views.call_child_snapshot(first, second)
  consume(second)
  return result.len() as i32
}
",
            ),
        ],
        "main.align",
    );
    assert!(
        !unselected_named.diags.has_errors(),
        "a later named-call argument must not replace the selected earlier argument's owner"
    );

    let selected_named = assert_same_verdict(
        "l2b-a2-source-order-named-selected-owner",
        &[
            producer,
            (
                "main.align",
                "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  result := views.call_child_snapshot(first, second)
  consume(first)
  return result.len() as i32
}
",
            ),
        ],
        "main.align",
    );
    assert!(
        selected_named.diags.has_errors(),
        "a named-call result must retain the completion-time owner of its selected argument"
    );

    let selected_array = assert_same_verdict(
        "l2b-a2-source-order-array-selected-owner",
        &[
            producer,
            (
                "main.align",
                "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  result := views.array_child_snapshot(first, second)
  consume(first)
  return result.len() as i32
}
",
            ),
        ],
        "main.align",
    );
    assert!(
        selected_array.diags.has_errors(),
        "the deferred array fallback must retain the earlier runtime element's owner"
    );

    for (name, source) in [
        (
            "product",
            "\
Pair { left: str, right: str }
fn main() -> i32 {
  mut owner := \"first\".clone()
  current: str := owner
  pair := Pair {
    left: current
    right: {
      owner = \"second\".clone()
      \"fixed\"
    }
  }
  return pair.left.len() as i32
}
",
        ),
        (
            "named-call",
            "\
fn take_first(first: str, ignored: str) -> str = first
fn main() -> i32 {
  mut owner := \"first\".clone()
  current: str := owner
  result := take_first(
    current,
    {
      owner = \"second\".clone()
      \"fixed\"
    },
  )
  return result.len() as i32
}
",
        ),
        (
            "direct-result-use",
            "\
fn take_first(first: str, ignored: str) -> str = first
fn main() -> i32 {
  mut owner := \"first\".clone()
  current: str := owner
  return take_first(
    current,
    {
      owner = \"second\".clone()
      \"fixed\"
    },
  ).len() as i32
}
",
        ),
        (
            "non-borrowing-call-result",
            "\
fn observe(first: str, ignored: str) -> i64 = first.len()
fn main() -> i32 {
  mut owner := \"first\".clone()
  current: str := owner
  value := observe(
    current,
    {
      owner = \"second\".clone()
      \"fixed\"
    },
  )
  return value as i32
}
",
        ),
        (
            "loop-probe-action",
            "\
fn observe(first: str, ignored: str) -> i64 = first.len()
fn main() -> i32 {
  loop {
    mut owner := \"first\".clone()
    current: str := owner
    value := observe(
      current,
      {
        owner = \"second\".clone()
        \"fixed\"
      },
    )
    break value as i32
  }
}
",
        ),
    ] {
        assert!(
            check_errs(&format!("l2b-a2-source-order-ended-{name}"), source),
            "a later eager sibling must invalidate the captured earlier owner generation ({name})"
        );
    }

    assert!(
        !check_errs(
            "l2b-a2-source-order-terminating-later-operand",
            "\
import std.process
fn observe(first: str, ignored: ()) -> i64 = first.len()
fn main() -> i32 {
  mut owner := \"first\".clone()
  current: str := owner
  value := observe(
    current,
    {
      owner = \"second\".clone()
      process.abort()
    },
  )
  return value as i32
}
",
        ),
        "a terminating later operand performs no enclosing call action and must not validate its earlier snapshot"
    );

    assert!(
        check_errs(
            "l2b-a2-source-order-wrapper-expression-identity",
            "\
Row { owned: string, view: str }
fn main() -> i32 {
  owner := \"source\".clone()
  view: str := owner
  mut keep: slice<Row> := [Row { owned: \"initial\".clone(), view: view }]
  mut done := false
  loop {
    keep = [Row { owned: \"iteration\".clone(), view: view }]
    if done {
      break
    }
    done = true
  }
  return keep.len() as i32
}
",
        ),
        "a synthetic ArrayToSlice wrapper must keep its own IterTemp root instead of aliasing its child by Span"
    );
}

#[test]
fn imported_aggregate_projection_is_exact_only_at_parameter_root_granularity() {
    let selected_files = &[
        (
            "views.align",
            "\
module views
pub Pair { selected: str, ignored: str }
pub fn selected(first: str, second: str) -> str {
  pair := Pair { selected: first, ignored: second }
  return pair.selected
}
",
        ),
        (
            "main.align",
            "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  result := views.selected(first, second)
  consume(second)
  return result.len() as i32
}
",
        ),
    ];
    let selected = assert_same_verdict(
        "l2b-a2-imported-selected-parameter",
        selected_files,
        "main.align",
    );
    assert!(
        !selected.diags.has_errors(),
        "an imported scalar projection must not retain an unselected parameter"
    );
    let selected_owner_files = &[
        selected_files[0],
        (
            "main.align",
            "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  result := views.selected(first, second)
  consume(first)
  return result.len() as i32
}
",
        ),
    ];
    let selected_owner = assert_same_verdict(
        "l2b-a2-imported-selected-owner",
        selected_owner_files,
        "main.align",
    );
    assert!(
        selected_owner.diags.has_errors(),
        "an imported scalar projection must retain its selected parameter owner"
    );

    let aggregate_files = &[
        (
            "views.align",
            "\
module views
pub Pair { selected: str, ignored: str }
pub fn selected(pair: Pair) -> str = pair.selected
",
        ),
        (
            "main.align",
            "\
import views
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  first := \"first\".clone()
  second := \"second\".clone()
  pair := views.Pair { selected: first, ignored: second }
  result := views.selected(pair)
  consume(second)
  return result.len() as i32
}
",
        ),
    ];
    let aggregate = assert_same_verdict(
        "l2b-a2-imported-aggregate-limit",
        aggregate_files,
        "main.align",
    );
    assert!(
        aggregate.diags.has_errors(),
        "the parameter-index-only interface must conservatively retain every owner in one aggregate actual"
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
            .find(|function| function.name.as_str() == name)
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
fn loop_break_recovery_uses_checked_target_and_region_evidence() {
    let accepted = "\
fn inner_break_stays_inner() -> str = loop {
  loop { break }
}
fn arena_baseline() -> i64 = arena {
  loop { break 1 }
}
fn task_group_baseline() -> i64 = task_group {
  loop { break 2 }
}
fn block_break() -> str = loop {
  { break \"block\" }
}
fn unsafe_break() -> str = loop {
  unsafe { break \"unsafe\" }
}
fn main() -> i32 {
  print(arena_baseline())
  print(task_group_baseline())
  print(block_break())
  print(unsafe_break())
  return 0
}
";
    assert!(
        !check_errs("l2b-loop-break-checked-evidence-positive", accepted),
        "inner-loop targeting, active-region baselines, block, and unsafe breaks must check"
    );

    for (name, source) in [
        (
            "arena",
            "fn bad(value: str) -> str = loop {\n  arena { break }\n  break value\n}\nfn main() -> i32 = 0\n",
        ),
        (
            "task-group",
            "fn bad(value: str) -> str = loop {\n  task_group { break }\n  break value\n}\nfn main() -> i32 = 0\n",
        ),
    ] {
        let mut source_map = SourceMap::new();
        let checked = check(
            &mut source_map,
            &format!("l2b-loop-break-rejected-{name}"),
            source,
        );
        let diagnostics =
            align_driver::format_diagnostics(&source_map, &checked.diags);
        assert!(
            diagnostics.contains(
                "a `break` inside an `arena`/`task_group` nested in the loop is not supported yet"
            ),
            "a rejected {name} break must diagnose without panicking:\n{diagnostics}"
        );
        let bad = checked
            .hir
            .fns
            .iter()
            .find(|function| function.name.as_str() == "bad")
            .expect("checked bad function");
        let loop_value = bad.body.value.as_deref().expect("loop body value");
        assert!(
            matches!(
                loop_value.kind,
                align_sema::ExprKind::Loop { diverges: true, .. }
            ),
            "a rejected {name} break must not combine with the unreachable later break to make the loop fall through:\n{diagnostics}"
        );
    }

    let lambda = check_diagnostics(
        "l2b-loop-break-lambda-isolation",
        "fn bad() -> str = loop {\n  f := fn { break }\n}\nfn main() -> i32 = 0\n",
    );
    assert!(
        lambda.contains("`break` outside of a `loop`"),
        "a lambda break must not target its enclosing source loop:\n{lambda}"
    );

    let invalid_payload = check_diagnostics(
        "l2b-loop-break-invalid-payload",
        "fn bad() -> i64 = loop { break \"wrong\" }\nfn main() -> i32 = 0\n",
    );
    assert!(
        invalid_payload.contains("type mismatch"),
        "an accepted break must retain its control edge while diagnosing its payload:\n{invalid_payload}"
    );

    let ordered = check_diagnostics(
        "l2b-loop-break-region-diagnostic-order",
        "fn bad() -> str = loop {\n  arena { break missing }\n}\nfn main() -> i32 = 0\n",
    );
    let region = ordered
        .find("a `break` inside an `arena`/`task_group` nested in the loop is not supported yet")
        .expect("region-scoped break diagnostic");
    let payload = ordered
        .find("undefined name: 'missing'")
        .expect("nested payload diagnostic");
    assert!(
        region < payload,
        "region rejection must precede nested payload diagnostics:\n{ordered}"
    );

    let mut source_map = SourceMap::new();
    let checked = check(
        &mut source_map,
        "l2b-loop-break-rejected-provenance",
        "\
pub fn bad(value: str) -> str = loop {
  arena { break value }
}
fn main() -> i32 = 0
",
    );
    let diagnostics =
        align_driver::format_diagnostics(&source_map, &checked.diags);
    assert!(
        diagnostics.contains(
            "a `break` inside an `arena`/`task_group` nested in the loop is not supported yet"
        ),
        "the rejected-provenance fixture must diagnose its region edge:\n{diagnostics}"
    );
    let bad = checked
        .hir
        .fns
        .iter()
        .find(|function| function.name.as_str() == "bad")
        .expect("checked bad provenance function");
    assert_eq!(
        bad.return_borrow,
        ReturnBorrowSummary::None,
        "a rejected break payload must not become a return-borrow root"
    );
    assert_eq!(
        bad.return_region,
        ReturnRegionSummary::None,
        "a rejected break payload must not become a return-region root"
    );

    let nested_effect = check_diagnostics(
        "l2b-loop-break-rejected-nested-effect",
        "\
fn impure(value: i64) -> i64 {
  print(value)
  return value
}
fn bad() -> i64 = loop {
  arena {
    break {
      values := [1, 2]
      values.par_map(impure).sum()
    }
  }
}
fn main() -> i32 = 0
",
    );
    assert!(
        nested_effect.contains("'par_map' requires a Pure function"),
        "a nested payload effect error must survive rejection:\n{nested_effect}"
    );

    let nested_ownership = check_diagnostics(
        "l2b-loop-break-rejected-nested-ownership",
        "\
fn consume(value: string) -> i64 = value.len()
fn bad(value: string) -> i64 {
  loop {
    arena {
      break {
        consume(value)
        consume(value)
      }
    }
  }
  return consume(value)
}
fn main() -> i32 = 0
",
    );
    assert_eq!(
        nested_ownership
            .matches("use of moved value 'value'")
            .count(),
        1,
        "the payload-internal second move must diagnose, but rejected break state must not reach the post-loop return:\n{nested_ownership}"
    );

    let nested_escape = check_diagnostics(
        "l2b-loop-break-rejected-nested-escape",
        "\
fn bad() -> slice<i64> = loop {
  arena {
    break {
      local := [1, 2]
      return local[0..1]
    }
  }
}
fn main() -> i32 = 0
",
    );
    assert!(
        nested_escape.contains(
            "cannot return a slice that views a local array"
        ),
        "a payload-internal return escape must survive rejection:\n{nested_escape}"
    );
    assert!(
        !nested_escape.contains("cannot `break` a view"),
        "the rejected outer break must not run the accepted-edge escape check:\n{nested_escape}"
    );
}

#[test]
fn unreachable_return_and_break_edges_do_not_taint_provenance() {
    let direct = "\
Numbers { first: i64, second: i64 }
fn fixed_return(value: str) -> str {
  return \"fixed\"
  return value
}
fn fixed_break(value: str) -> str = loop {
  break \"fixed\"
  break value
}
fn add(first: i64, second: i64) -> i64 = first + second
fn fixed_argument(value: str) -> str {
  add({ return \"fixed\"; 0 }, { return value; 1 })
  return value
}
fn fixed_operand(value: str) -> str {
  { return \"fixed\"; 0 } + { return value; 1 }
  return value
}
fn fixed_member(value: str) -> str {
  pair := Numbers {
    first: { return \"fixed\"; 0 }
    second: { return value; 1 }
  }
  return value
}
fn fixed_bound(value: str) -> str {
  \"abc\"[{ return \"fixed\"; 0 }..{ return value; 1 }]
  return value
}
fn fixed_index(value: str) -> str {
  values := [1, 2]
  view: slice<i64> := values
  { return \"fixed\"; view }[{ return value; 0 }]
  return value
}
fn fixed_branch(value: str, choose: bool) -> str =
  if choose { return \"fixed\"; value } else { \"fixed\" }
fn fixed_match(value: str, selected: Option<i64>) -> str =
  match selected {
    Some(_) => { return \"fixed\"; value }
    None => \"fixed\"
  }
fn fixed_else(value: str) -> str =
  Some(\"fixed\") else { return \"fixed\"; value }
fn fixed_loop(value: str) -> str = loop {
  return \"fixed\"
  break value
}
fn fixed_break_payload(value: str) -> str = loop {
  break { return \"fixed\"; value }
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
  print(broken.len())

  third := \"argument owner\".clone()
  argument := fixed_argument(third)
  consume(third)
  print(argument.len())

  fourth := \"operand owner\".clone()
  operand := fixed_operand(fourth)
  consume(fourth)
  print(operand.len())

  fifth := \"member owner\".clone()
  member := fixed_member(fifth)
  consume(fifth)
  print(member.len())

  sixth := \"bound owner\".clone()
  bound := fixed_bound(sixth)
  consume(sixth)
  print(bound.len())

  seventh := \"index owner\".clone()
  indexed := fixed_index(seventh)
  consume(seventh)
  print(indexed.len())

  eighth := \"branch owner\".clone()
  branched := fixed_branch(eighth, true)
  consume(eighth)
  print(branched.len())

  ninth := \"match owner\".clone()
  matched := fixed_match(ninth, Some(1))
  consume(ninth)
  print(matched.len())

  tenth := \"else owner\".clone()
  unwrapped := fixed_else(tenth)
  consume(tenth)
  print(unwrapped.len())

  eleventh := \"loop owner\".clone()
  looped := fixed_loop(eleventh)
  consume(eleventh)
  print(looped.len())

  twelfth := \"break payload owner\".clone()
  payload := fixed_break_payload(twelfth)
  consume(twelfth)
  return payload.len() as i32
}
";
    let direct_diagnostics =
        check_diagnostics("l2b-unreachable-exits-direct", direct);
    assert!(
        !check_errs("l2b-unreachable-exits-direct", direct),
        "dead returns and breaks must not retain an otherwise-unselected owner:\n{direct_diagnostics}"
    );

    let files = &[
        (
            "dep.align",
            "\
module dep
pub Numbers { first: i64, second: i64 }
pub fn fixed_return(value: str) -> str {
  return \"fixed\"
  return value
}
pub fn fixed_break(value: str) -> str = loop {
  break \"fixed\"
  break value
}
fn add(first: i64, second: i64) -> i64 = first + second
pub fn fixed_argument(value: str) -> str {
  add({ return \"fixed\"; 0 }, { return value; 1 })
  return value
}
pub fn fixed_operand(value: str) -> str {
  { return \"fixed\"; 0 } + { return value; 1 }
  return value
}
pub fn fixed_member(value: str) -> str {
  pair := Numbers {
    first: { return \"fixed\"; 0 }
    second: { return value; 1 }
  }
  return value
}
pub fn fixed_bound(value: str) -> str {
  \"abc\"[{ return \"fixed\"; 0 }..{ return value; 1 }]
  return value
}
pub fn fixed_index(value: str) -> str {
  values := [1, 2]
  view: slice<i64> := values
  { return \"fixed\"; view }[{ return value; 0 }]
  return value
}
pub fn fixed_branch(value: str, choose: bool) -> str =
  if choose { return \"fixed\"; value } else { \"fixed\" }
pub fn fixed_match(value: str, selected: Option<i64>) -> str =
  match selected {
    Some(_) => { return \"fixed\"; value }
    None => \"fixed\"
  }
pub fn fixed_else(value: str) -> str =
  Some(\"fixed\") else { return \"fixed\"; value }
pub fn fixed_loop(value: str) -> str = loop {
  return \"fixed\"
  break value
}
pub fn fixed_break_payload(value: str) -> str = loop {
  break { return \"fixed\"; value }
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
  print(broken.len())

  third := \"argument owner\".clone()
  argument := dep.fixed_argument(third)
  consume(third)
  print(argument.len())

  fourth := \"operand owner\".clone()
  operand := dep.fixed_operand(fourth)
  consume(fourth)
  print(operand.len())

  fifth := \"member owner\".clone()
  member := dep.fixed_member(fifth)
  consume(fifth)
  print(member.len())

  sixth := \"bound owner\".clone()
  bound := dep.fixed_bound(sixth)
  consume(sixth)
  print(bound.len())

  seventh := \"index owner\".clone()
  indexed := dep.fixed_index(seventh)
  consume(seventh)
  print(indexed.len())

  eighth := \"branch owner\".clone()
  branched := dep.fixed_branch(eighth, true)
  consume(eighth)
  print(branched.len())

  ninth := \"match owner\".clone()
  matched := dep.fixed_match(ninth, Some(1))
  consume(ninth)
  print(matched.len())

  tenth := \"else owner\".clone()
  unwrapped := dep.fixed_else(tenth)
  consume(tenth)
  print(unwrapped.len())

  eleventh := \"loop owner\".clone()
  looped := dep.fixed_loop(eleventh)
  consume(eleventh)
  print(looped.len())

  twelfth := \"break payload owner\".clone()
  payload := dep.fixed_break_payload(twelfth)
  consume(twelfth)
  return payload.len() as i32
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
    for name in [
        "fixed_return",
        "fixed_break",
        "fixed_argument",
        "fixed_operand",
        "fixed_member",
        "fixed_bound",
        "fixed_index",
        "fixed_branch",
        "fixed_match",
        "fixed_else",
        "fixed_loop",
        "fixed_break_payload",
    ] {
        let function = dependency
            .fns
            .iter()
            .find(|function| function.name.as_str() == name)
            .unwrap_or_else(|| panic!("{name} signature"));
        assert_eq!(function.return_borrow, ReturnBorrowSummary::None);
        assert_eq!(function.return_region, ReturnRegionSummary::None);
    }
}

#[test]
fn reachable_conditional_returns_and_breaks_retain_provenance() {
    let files = &[
        (
            "dep.align",
            "\
module dep
pub fn conditional_return(value: str, choose: bool) -> str {
  choose || { return value; false }
  return \"fixed\"
}
pub fn conditional_break(value: str, choose: bool) -> str = loop {
  if choose { break value }
  break \"fixed\"
}
",
        ),
        (
            "main.align",
            "\
import dep
fn main() -> i32 = 0
",
        ),
    ];
    let differential =
        diff_check_multi("l2b-reachable-conditional-edges", files, "main.align");
    assert_eq!(
        differential.whole_errors, differential.per_unit_errors,
        "verdict mismatch:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags, differential.per_unit_diags
    );
    assert!(
        !differential.per_unit.diags.has_errors(),
        "reachable conditional edges must check:\nwhole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags
    );
    let dependency = differential
        .per_unit
        .summaries
        .iter()
        .find(|summary| summary.unit == "dep")
        .expect("dependency summary");
    for name in ["conditional_return", "conditional_break"] {
        let function = dependency
            .fns
            .iter()
            .find(|function| function.name.as_str() == name)
            .unwrap_or_else(|| panic!("{name} signature"));
        assert_eq!(function.return_borrow, roots(&[0], &[]));
        assert_eq!(
            function.return_region,
            ReturnRegionSummary::Roots {
                params: vec![0],
                captures: vec![],
            }
        );
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
        .find(|function| function.name.as_str() == "lambda0")
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
fn terminating_and_mixed_break_payloads_preserve_typed_runtime_values() {
    if !backend_available() {
        return;
    }
    let src = "\
Choice { Left, Right }
fn mixed_if(flag: bool) -> str = loop {
  break if flag { break \"inner\"; \"dead\" } else { \"outer\" }
}
fn mixed_match(choice: Choice) -> str = loop {
  break match choice {
    Left => { break \"inner\"; \"dead\" }
    Right => \"outer\"
  }
}
fn mixed_else(value: Option<str>) -> str = loop {
  break value else { break \"fallback\"; \"dead\" }
}
fn mixed_try(value: Result<str, Error>) -> Result<str, Error> {
  result := loop { break value? }
  return Ok(result)
}
fn mixed_short(flag: bool) -> bool = loop {
  break flag || { break false; true }
}
fn main() -> i32 {
  tried := mixed_try(Ok(\"try\")) else { return 99 }
  short_score := if mixed_short(true) { 1 } else { 0 }
  mut total := mixed_if(true).len()
  total = total + mixed_if(false).len()
  total = total + mixed_match(Choice.Left).len()
  total = total + mixed_match(Choice.Right).len()
  total = total + mixed_else(Some(\"some\")).len()
  total = total + mixed_else(None).len()
  total = total + tried.len()
  total = total + short_score
  return total as i32
}
";
    let output = build_and_run("l2b-a1-terminating-break-copy", src);
    assert_eq!(
        output.status.code(),
        Some(36),
        "typed Copy loop results must survive inner termination: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn mixed_owned_break_payload_transfers_and_drops_once() {
    if !backend_available() {
        return;
    }
    let src = "\
fn select_owned(flag: bool) -> string = loop {
  current := \"outer\".clone()
  break {
    if flag { break \"inner\".clone() }
    current
  }
}
fn main() -> i32 {
  first := select_owned(true)
  second := select_owned(false)
  return (first.len() + second.len()) as i32
}
";
    let output = build_and_run("l2b-a1-terminating-break-owned", src);
    assert_eq!(
        output.status.code(),
        Some(10),
        "each selected owned loop result must transfer once and drop once: {}",
        String::from_utf8_lossy(&output.stderr)
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
        .find(|function| function.name.as_str() == "second")
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
        .find(|function| function.name.as_str() == "identity")
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
