//! L2b return-provenance gate across direct/imported calls, projections, and function values.

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
pub ViewError { Text(str), Fixed }
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
pub fn keep_view_error(value: ViewError) -> ViewError = value
pub fn fixed_view_error(_: ViewError) -> ViewError = ViewError.Fixed
pub fn map_ok(value: str, ignored: str) -> Result<str, ViewError> {
  result: Result<str, ViewError> := Ok(value)
  return result.map_err(keep_view_error)
}
pub fn map_error(value: str, ignored: str) -> Result<str, ViewError> {
  result: Result<str, ViewError> := Err(ViewError.Text(value))
  return result.map_err(keep_view_error)
}
pub fn map_fixed_error(value: str) -> Result<str, ViewError> {
  result: Result<str, ViewError> := Err(ViewError.Text(value))
  return result.map_err(fixed_view_error)
}
pub fn map_captured_error(value: str) -> Result<str, ViewError> {
  result: Result<str, ViewError> := Err(ViewError.Fixed)
  mapper := fn _: ViewError { ViewError.Text(value) }
  return result.map_err(mapper)
}
pub fn map_unresolved(
  result: Result<str, ViewError>,
  mapper: fn(ViewError) -> ViewError,
) -> Result<str, ViewError> = result.map_err(mapper)
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
        roots(&[1], &[]),
        "the implicit Err edge owns its payload; only the continuing fallback view borrows"
    );
    assert_eq!(
        find("consume_try_success").return_borrow,
        ReturnBorrowSummary::None,
        "a borrowing Ok payload must not put provenance on a non-borrowing Result return"
    );
    assert_eq!(
        find("choose").return_borrow,
        roots(&[0], &[]),
        "the selected First payload must not retain the inactive Second sibling"
    );
    assert_eq!(find("map_ok").return_borrow, roots(&[0], &[]));
    assert_eq!(find("map_error").return_borrow, roots(&[0], &[]));
    assert_eq!(
        find("map_fixed_error").return_borrow,
        ReturnBorrowSummary::None
    );
    assert_eq!(find("map_captured_error").return_borrow, roots(&[0], &[]));
    assert_eq!(
        find("map_unresolved").return_borrow,
        roots(&[0, 1], &[]),
        "an unresolved mapper must retain both its compatible Result input and environment"
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
            roots(&[0], &[]),
            "{name} must retain only the selected first parameter"
        );
    }
}

#[test]
fn borrowed_str_element_stores_run_for_fixed_dynamic_and_slice_bases() {
    if !backend_available() {
        return;
    }
    let src = "fn install(out values: slice<str>, value: str) {\n  values[0] = value\n}\nfn main() -> i32 {\n  mut fixed := [\"old\", \"keep\"]\n  fixed[0] = \"fixed\"\n  mut dynamic := [\"left\", \"right\"].to_array()\n  dynamic[1] = \"fixed\"\n  install(dynamic, \"slice\")\n  if fixed[0] == \"fixed\" && dynamic[0] == \"slice\" && dynamic[1] == \"fixed\" { return 0 }\n  return 9\n}\n";
    assert_eq!(
        build_and_run("l2b-a2-indexed-str-stores", src)
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn indexed_str_store_accepts_frame_and_same_arena_storage() {
    let src = "\
fn main() -> i32 {
  frame_owner := \"frame\".clone()
  frame_value: str := frame_owner
  mut frame_fixed := [\"old\"]
  frame_fixed[0] = frame_value
  mut frame_dynamic := [\"old\"].to_array()
  frame_dynamic[0] = frame_value
  arena {
    n := 42
    arena_value := template \"arena={n}\"
    mut arena_fixed := [\"old\"]
    arena_fixed[0] = arena_value
    mut arena_dynamic := [\"old\"].to_array()
    arena_dynamic[0] = arena_value
    if arena_fixed[0] != arena_value || arena_dynamic[0] != arena_value {
      return 1
    }
  }
  if frame_fixed[0] == frame_value && frame_dynamic[0] == frame_value { return 0 }
  return 2
}
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "l2b-a2-indexed-str-same-storage", src);
    let diagnostics = align_driver::format_diagnostics(&source_map, &checked.diags);
    assert!(
        !checked.diags.has_errors(),
        "fixed and dynamic arrays may retain a str for exactly their own storage lifetime:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_alias_uses_backing_storage_region() {
    for (name, storage) in [
        ("fixed", "mut values := [\"old\"]"),
        ("dynamic", "mut values := [\"old\"].to_array()"),
    ] {
        let src = format!(
            "fn main() -> i32 {{\n  {storage}\n  arena {{\n    mut alias: slice<str> := values\n    n := 42\n    short := template \"short={{n}}\"\n    alias[0] = short\n  }}\n  if values[0] == \"never\" {{ return 1 }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-indexed-str-alias-{name}"), &src);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived array"),
            "a slice alias must keep its {name} backing lifetime:\n{diagnostics}"
        );
    }
}

#[test]
fn indexed_str_store_reassigned_and_joined_aliases_use_reaching_backings() {
    for (name, transition) in [
        ("straight", "    alias = outer"),
        ("branch", "    if choose { alias = outer }"),
        (
            "loop",
            "    loop {\n      if choose { alias = outer }\n      break\n    }",
        ),
    ] {
        let src = format!(
            "fn run(choose: bool) -> i32 {{\n  mut outer := [\"outer\"]\n  arena {{\n    mut inner := [\"inner\"]\n    mut alias: slice<str> := inner\n{transition}\n    n := 42\n    short := template \"short={{n}}\"\n    alias[0] = short\n  }}\n  if outer[0] == \"never\" {{ return 1 }}\n  return 0\n}}\nfn main() -> i32 = run(false)\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-indexed-str-join-{name}"), &src);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived array"),
            "the {name} alias transition must retain every reaching outer backing:\n{diagnostics}"
        );
    }

    let strong_update = "\
fn main() -> i32 {
  mut outer := [\"outer\"]
  arena {
    mut inner := [\"inner\"]
    mut alias: slice<str> := outer
    alias = inner
    n := 42
    short := template \"short={n}\"
    alias[0] = short
    if alias[0] == short { return 0 }
  }
  return 1
}
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-reassign-strong", strong_update);
    assert!(
        diagnostics.is_empty(),
        "a straight-line reassignment to same-arena storage must replace the obsolete outer backing:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_mixed_region_and_heap_join_keeps_the_longer_backing() {
    let src = "\
fn region_values(out: region) -> array<str> {
  mut builder: array_builder<str> := array_builder(out)
  builder.push(\"outer\")
  return builder.build()
}
fn run(choose_heap: bool) -> i32 {
  arena outer {
    mut values := region_values(outer)
    if choose_heap { values = [\"heap\"].to_array() }
    arena inner {
      n := 42
      short := template \"short={n}\"
      values[0] = short
    }
  }
  return 0
}
fn main() -> i32 = run(false)
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-region-heap-join", src);
    assert!(
        diagnostics.contains("cannot be stored into a longer-lived array"),
        "a caller-region/lexical-heap join must keep the longer possible destination lifetime:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_unknown_backing_fails_closed_for_all_region_bearing_values() {
    let rejecting = "\
fn identity(values: slice<str>) -> slice<str> = values
fn main() -> i32 {
  mut values := [\"old\"]
  mut unknown: slice<str> := identity(values)
  owner := \"frame\".clone()
  view: str := owner
  unknown[0] = view
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-unknown", rejecting);
    assert!(
        diagnostics.contains("cannot be stored into a longer-lived array"),
        "a non-static view must not be retained through unresolved writable storage:\n{diagnostics}"
    );

    let static_control = rejecting.replace("unknown[0] = view", "unknown[0] = \"static\"");
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-unknown-static", &static_control);
    assert!(
        diagnostics.contains("cannot be stored into a longer-lived array"),
        "an unresolved writable slice must reject every region-bearing write:\n{diagnostics}"
    );
}

#[test]
fn non_borrowing_mutable_calls_do_not_require_region_backing() {
    let scalar_slice = "\
fn identity(values: slice<i64>) -> slice<i64> = values
fn fill(out dst: slice<i64>) {
  dst[0] = 7
}
fn touch(borrow mut dst: slice<i64>) {
  dst[0] = 8
}
fn main() -> i32 {
  mut values := [0]
  fill(identity(values))
  mut alias: slice<i64> := identity(values)
  touch(alias)
  if values[0] == 8 { return 0 }
  return 1
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-non-borrowing-mutable-slice",
        scalar_slice,
    );
    assert!(
        diagnostics.is_empty(),
        "scalar-only out and borrow-mut calls cannot retain region provenance:\n{diagnostics}"
    );

    let scalar_soa_column = "\
Point { x: i64, y: i64 }
fn fill(out dst: slice<i64>) {
  dst[0] = 7
}
fn main() -> i32 {
  arena {
    rows := [Point { x: 0, y: 0 }].to_soa()
    fill(rows.x)
  }
  return 0
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-non-borrowing-mutable-soa-column",
        scalar_soa_column,
    );
    assert!(
        diagnostics.is_empty(),
        "a primitive SoA column has no element retention backing to prove:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_requires_a_retaining_parameter_mode_for_caller_backing() {
    for (name, mode) in [("by-value", ""), ("shared-borrow", "borrow ")] {
        let src = format!(
            "fn install({mode}dst: slice<str>, value: str) {{\n  mut alias: slice<str> := dst\n  alias[0] = value\n}}\nfn main() -> i32 {{\n  mut values := [\"old\"]\n  owner := \"source\".clone()\n  view: str := owner\n  install(values, view)\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("l2b-a2-indexed-str-untracked-parameter-{name}"),
            &src,
        );
        assert!(
            diagnostics.contains(
                "may retain a borrowed view only through an `out` or `borrow mut` parameter"
            ),
            "a {name} slice parameter has no caller retention transition:\n{diagnostics}"
        );
    }

    let static_control = "\
fn install(dst: slice<str>) {
  mut alias: slice<str> := dst
  alias[0] = \"static\"
}
fn main() -> i32 {
  mut values := [\"old\"]
  install(values)
  return 0
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-indexed-str-untracked-parameter-static",
        static_control,
    );
    assert!(
        diagnostics
            .contains("may retain a borrowed view only through an `out` or `borrow mut` parameter"),
        "a caller-backed write needs an explicit retention transition even for a static value:\n{diagnostics}"
    );

    for (name, mode) in [("by-value", ""), ("shared-borrow", "borrow ")] {
        let src = format!(
            "User {{ name: str, age: i64 }}\nfn install({mode}rows: soa<User>, value: str) {{\n  mut alias: soa<User> := rows\n  alias[0].name = value\n}}\nfn main() -> i32 {{\n  arena {{\n    rows := [User {{ name: \"old\", age: 1 }}].to_soa()\n    install(rows, \"static\")\n  }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("l2b-a2-indexed-soa-untracked-parameter-{name}"),
            &src,
        );
        assert!(
            diagnostics.contains(
                "may retain a borrowed view only through an `out` or `borrow mut` parameter"
            ),
            "a {name} soa parameter has no caller retention transition:\n{diagnostics}"
        );
    }

    let retaining_control = "\
fn install(borrow mut dst: slice<str>) {
  mut alias: slice<str> := dst
  alias[0] = \"static\"
}
fn main() -> i32 {
  mut values := [\"old\"]
  mut destination: slice<str> := values
  install(destination)
  if values[0] == \"static\" { return 0 }
  return 1
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-indexed-str-retaining-parameter-control",
        retaining_control,
    );
    assert!(
        diagnostics.is_empty(),
        "a borrow-mut parameter publishes the caller retention transition:\n{diagnostics}"
    );
}

#[test]
fn forwarded_mutable_str_stores_require_a_retaining_parameter_mode() {
    for (leaf_name, leaf_mode) in [("out", "out "), ("borrow-mut", "borrow mut ")] {
        for (wrapper_name, wrapper_mode) in [("by-value", ""), ("shared", "borrow ")] {
            let src = format!(
                "fn leaf({leaf_mode}dst: slice<str>, value: str) {{\n  dst[0] = value\n}}\nfn forward({wrapper_mode}dst: slice<str>, value: str) {{\n  mut alias: slice<str> := dst\n  leaf(alias, value)\n}}\nfn main() -> i32 {{\n  mut values := [\"old\"]\n  forward(values, \"static\")\n  return 0\n}}\n"
            );
            let diagnostics =
                check_diagnostics(&format!("l2b-a2-{leaf_name}-forward-{wrapper_name}"), &src);
            assert!(
                diagnostics.contains(
                    "may retain a borrowed view only through an `out` or `borrow mut` parameter"
                ),
                "a {wrapper_name} wrapper must not launder a {leaf_name} element mutation:\n{diagnostics}"
            );
        }
    }
}

#[test]
fn borrow_mut_str_store_checks_backing_and_preserves_same_region_control() {
    let rejecting = "\
fn install(borrow mut dst: slice<str>, value: str) {
  dst[0] = value
}
fn main() -> i32 {
  mut values := [\"old\"]
  arena {
    mut alias: slice<str> := values
    n := 42
    short := template \"short={n}\"
    install(alias, short)
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-borrow-mut-str-backing", rejecting);
    assert!(
        diagnostics.contains("cannot retain a shorter-lived view through this mutable borrow"),
        "borrow mut must validate the backing as well as the slice header:\n{diagnostics}"
    );

    let accepted = rejecting.replace(
        "  mut values := [\"old\"]\n  arena {\n    mut alias: slice<str> := values",
        "  arena {\n    mut values := [\"old\"]\n    mut alias: slice<str> := values",
    );
    let diagnostics = check_diagnostics("l2b-a2-borrow-mut-str-same-region", &accepted);
    assert!(
        diagnostics.is_empty(),
        "a same-region borrow-mut element mutation must stay accepted:\n{diagnostics}"
    );
}

#[test]
fn moved_and_destructured_owned_arrays_use_binding_storage_lifetime() {
    let moved = "\
fn main() -> i32 {
  mut outer := [\"old\"].to_array()
  arena {
    mut inner := outer
    n := 42
    short := template \"short={n}\"
    inner[0] = short
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-moved-owner", moved);
    assert!(
        diagnostics.is_empty(),
        "a moved dynamic buffer dies with its new inner binding:\n{diagnostics}"
    );

    let destructured = "\
fn keep(value: str) -> bool = true
fn main() -> i32 {
  owner := \"source\".clone()
  view: str := owner
  (selected, _) := [view].partition(keep)
  mut writable := selected
  writable[0] = view
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-destructured-owner", destructured);
    assert!(
        diagnostics.is_empty(),
        "a tuple-destructured owned array must regain an exact binding owner:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_ignores_diverging_backing_arms() {
    let src = "\
fn main() -> i32 {
  owner := \"source\".clone()
  view: str := owner
  mut values := [\"old\"]
  mut live: slice<str> := values
  mut alias: slice<str> := match Some(true) {
    Some(_) => live
    None => { return 1 }
  }
  alias[0] = view
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-diverging-arm", src);
    assert!(
        diagnostics.is_empty(),
        "a diverging arm cannot poison the only reachable backing:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_distinguishes_exact_and_conservative_overwrites() {
    let exact = "\
fn main() -> i32 {
  mut old_owner := \"old\".clone()
  old_view: str := old_owner
  new_owner := \"new\".clone()
  new_view: str := new_owner
  mut values := [old_view, \"keep\"]
  values[0] = new_view
  old_owner = \"replaced\".clone()
  if values[0] == new_view { return 0 }
  return 1
}
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-exact-overwrite", exact);
    assert!(
        diagnostics.is_empty(),
        "an exact fixed-index overwrite must clear the obsolete element owner:\n{diagnostics}"
    );

    for (name, storage, store, read) in [
        (
            "computed-fixed",
            "mut values := [old_view, \"keep\"]\n  index := 0",
            "values[index] = new_view",
            "values[0]",
        ),
        (
            "dynamic",
            "mut values := [old_view].to_array()",
            "values[0] = new_view",
            "values[0]",
        ),
        (
            "slice",
            "mut values := [old_view]\n  mut alias: slice<str> := values",
            "alias[0] = new_view",
            "alias[0]",
        ),
    ] {
        let src = format!(
            "fn main() -> i32 {{\n  mut old_owner := \"old\".clone()\n  old_view: str := old_owner\n  new_owner := \"new\".clone()\n  new_view: str := new_owner\n  {storage}\n  {store}\n  old_owner = \"replaced\".clone()\n  if {read} == new_view {{ return 0 }}\n  return 1\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-indexed-str-{name}-overwrite"), &src);
        assert!(
            diagnostics.contains("use of invalidated borrow"),
            "the {name} write has no exact element map and must conservatively retain the old owner:\n{diagnostics}"
        );
    }
}

#[test]
fn indexed_str_store_updates_the_contained_region_before_return() {
    let src = "\
fn leak(seed: str) -> str {
  arena {
    short := template \"short={seed}\"
    mut values := [\"old\"]
    values[0] = short
    return values[0]
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-indexed-str-store-return", src);
    assert!(
        diagnostics.contains("cannot return a value allocated in an arena"),
        "an accepted same-arena element store must update the base's contained region before a later read/return:\n{diagnostics}"
    );
}

#[test]
fn out_str_store_rejects_callee_local_and_inner_arena_views() {
    for (name, body) in [
        (
            "frame",
            "  owner := \"local\".clone()\n  value: str := owner\n  dst[0] = value",
        ),
        (
            "arena",
            "  arena {\n    n := 42\n    value := template \"local={n}\"\n    dst[0] = value\n  }",
        ),
    ] {
        let src = format!(
            "fn install(out dst: slice<str>) {{\n{body}\n}}\nfn main() -> i32 {{\n  mut values := [\"old\"]\n  install(values)\n  if values[0] == \"never\" {{ return 1 }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-out-str-callee-{name}"), &src);
        assert!(
            diagnostics.contains("cannot be stored into a longer-lived array"),
            "an out parameter must denote caller storage, not the callee's {name}:\n{diagnostics}"
        );
    }
}

#[test]
fn out_str_store_rejects_inner_arena_sources_at_the_call_site() {
    for (name, storage, destination) in [
        ("fixed", "mut values := [\"old\"]", "values"),
        ("dynamic", "mut values := [\"old\"].to_array()", "values"),
        (
            "nested-range",
            "mut values := [\"old\", \"keep\"]",
            "values[0..2][0..1]",
        ),
    ] {
        let src = format!(
            "fn install(out dst: slice<str>, value: str) {{\n  dst[0] = value\n}}\nfn main() -> i32 {{\n  {storage}\n  arena {{\n    n := 42\n    short := template \"short={{n}}\"\n    install({destination}, short)\n  }}\n  if values[0] == \"never\" {{ return 1 }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-out-str-call-{name}"), &src);
        assert!(
            diagnostics.contains("cannot retain a shorter-lived view through this mutable borrow"),
            "the direct out summary must reject an inner-arena source retained by outer {name} storage:\n{diagnostics}"
        );
    }
}

#[test]
fn indexed_and_out_str_stores_keep_source_owners_live_through_backing_roots() {
    for (name, storage, setup, store, read) in [
        (
            "direct-alias-fixed",
            "mut values := [\"old\"]",
            "  mut alias: slice<str> := values",
            "  alias[0] = view",
            "values[0]",
        ),
        (
            "direct-alias-dynamic",
            "mut values := [\"old\"].to_array()",
            "  mut alias: slice<str> := values",
            "  alias[0] = view",
            "values[0]",
        ),
        (
            "direct-range-fixed",
            "mut values := [\"keep\", \"old\"]",
            "  mut alias: slice<str> := values[1..2]",
            "  alias[0] = view",
            "values[1]",
        ),
        (
            "direct-nested-range-fixed",
            "mut values := [\"keep\", \"old\", \"keep\"]",
            "  mut alias: slice<str> := values[0..3][1..2]",
            "  alias[0] = view",
            "values[1]",
        ),
        (
            "inline-array-to-slice-out",
            "mut values := [\"old\"]",
            "",
            "  install(values, view)",
            "values[0]",
        ),
        (
            "dynamic-alias-out",
            "mut values := [\"old\"].to_array()",
            "  mut alias: slice<str> := values",
            "  install(alias, view)",
            "values[0]",
        ),
        (
            "nested-range-out",
            "mut values := [\"old\", \"keep\"]",
            "",
            "  install(values[0..2][1..2], view)",
            "values[1]",
        ),
    ] {
        let src = format!(
            "fn install(out dst: slice<str>, value: str) {{\n  dst[0] = value\n}}\nfn main() -> i32 {{\n  {storage}\n  mut owner := \"first\".clone()\n  view: str := owner\n{setup}\n{store}\n  owner = \"second\".clone()\n  if {read} == \"never\" {{ return 1 }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-indexed-str-owner-{name}"), &src);
        assert!(
            diagnostics.contains("use of invalidated borrow 'values'"),
            "the {name} store must publish the installed owner's generation to the original backing root:\n{diagnostics}"
        );
    }
}

#[test]
fn indexed_str_stores_update_preexisting_and_unresolved_alias_observers() {
    for (name, alias, store) in [
        (
            "direct-known",
            "mut alias: slice<str> := values",
            "values[0] = view",
        ),
        (
            "out-known",
            "mut alias: slice<str> := values",
            "install(values, view)",
        ),
        (
            "direct-unresolved",
            "mut alias: slice<str> := identity(values)",
            "values[0] = view",
        ),
        (
            "out-unresolved",
            "mut alias: slice<str> := identity(values)",
            "install(values, view)",
        ),
    ] {
        let src = format!(
            "fn identity(values: slice<str>) -> slice<str> = values\nfn install(out dst: slice<str>, value: str) {{\n  dst[0] = value\n}}\nfn main() -> i32 {{\n  mut values := [\"old\"]\n  {alias}\n  mut owner := \"source\".clone()\n  view: str := owner\n  {store}\n  owner = \"replacement\".clone()\n  if alias[0] == \"never\" {{ return 1 }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-indexed-str-observer-{name}"), &src);
        assert!(
            diagnostics.contains("use of invalidated borrow 'alias'"),
            "the {name} store must publish new contents to an alias created before it:\n{diagnostics}"
        );
    }

    for (name, alias) in [
        ("known", "mut alias: slice<str> := values"),
        ("unresolved", "mut alias: slice<str> := identity(values)"),
    ] {
        let src = format!(
            "fn identity(values: slice<str>) -> slice<str> = values\nfn leak(seed: str) -> str {{\n  owner := seed.clone()\n  view: str := owner\n  mut values := [\"old\"]\n  {alias}\n  values[0] = view\n  return alias[0]\n}}\nfn main() -> i32 = 0\n"
        );
        let diagnostics =
            check_diagnostics(&format!("l2b-a2-indexed-str-observer-return-{name}"), &src);
        assert!(
            diagnostics.contains("cannot return a view that borrows local storage"),
            "the {name} observer must receive the backing root's contained region:\n{diagnostics}"
        );
    }

    let unrelated_contents = "\
fn main() -> i32 {
  old_owner := \"old\".clone()
  old_view: str := old_owner
  mut left := [old_view]
  mut right := [old_view]
  mut new_owner := \"new\".clone()
  new_view: str := new_owner
  left[0] = new_view
  new_owner = \"replacement\".clone()
  if right[0] == old_view { return 0 }
  return 1
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-indexed-str-unrelated-content-control",
        unrelated_contents,
    );
    assert!(
        diagnostics.is_empty(),
        "sharing an old element owner does not make two arrays backing aliases:\n{diagnostics}"
    );
}

#[test]
fn mutable_str_retention_uses_argument_completion_snapshots() {
    let destination = "\
fn install(out dst: slice<str>, value: str, ignored: i64) {
  dst[0] = value
}
fn main() -> i32 {
  mut first := [\"first\"]
  mut second := [\"second\"]
  mut destination: slice<str> := first
  mut owner := \"source\".clone()
  view: str := owner
  install(destination, view, {
    destination = second
    0
  })
  owner = \"replacement\".clone()
  if first[0] == \"never\" { return 1 }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-out-destination-snapshot", destination);
    assert!(
        diagnostics.contains("use of invalidated borrow 'first'"),
        "a later argument must not retarget an already-evaluated out slice:\n{diagnostics}"
    );

    let rebound_control = "\
fn install(out dst: slice<str>, value: str, ignored: i64) {
  dst[0] = value
}
fn select_rebound(seed: str) -> str = arena outer {
  mut outer_values := [\"outer\"]
  arena inner {
    mut inner_values := [\"inner\"]
    mut destination: slice<str> := inner_values
    inner_owner := seed.clone()
    inner_view: str := inner_owner
    install(destination, inner_view, {
      destination = outer_values
      0
    })
    destination[0]
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-out-rebound-destination-control", rebound_control);
    assert!(
        diagnostics.is_empty(),
        "the old captured backing must not publish contents into the rebound slice local:\n{diagnostics}"
    );

    let source_owner = "\
fn install(out dst: slice<str>, value: str, ignored: i64) {
  dst[0] = value
}
fn main() -> i32 {
  mut values := [\"old\"]
  mut owner := \"source\".clone()
  mut source: str := owner
  install(values, source, {
    source = \"static\"
    0
  })
  owner = \"replacement\".clone()
  if values[0] == \"never\" { return 1 }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-out-source-owner-snapshot", source_owner);
    assert!(
        diagnostics.contains("use of invalidated borrow 'values'"),
        "a later argument must not replace an already-evaluated source owner:\n{diagnostics}"
    );

    let source_region = "\
fn install(out dst: slice<str>, value: str, ignored: i64) {
  dst[0] = value
}
fn main() -> i32 {
  mut values := [\"old\"]
  arena {
    n := 42
    mut source: str := template \"short={n}\"
    install(values, source, {
      source = \"static\"
      0
    })
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-out-source-region-snapshot", source_region);
    assert!(
        diagnostics.contains("cannot retain a shorter-lived view through this mutable borrow"),
        "escape checking must use the source region captured before a later argument rebinds it:\n{diagnostics}"
    );
}

#[test]
fn out_str_retention_matches_whole_and_per_unit_checking() {
    let files = &[
        (
            "views.align",
            "module views\npub fn install(out dst: slice<str>, value: str) {\n  dst[0] = value\n}\n",
        ),
        (
            "main.align",
            "\
import views
fn main() -> i32 {
  mut values := [\"old\"]
  arena {
    n := 42
    short := template \"short={n}\"
    views.install(values, short)
  }
  if values[0] == \"never\" { return 1 }
  return 0
}
",
        ),
    ];
    let checked = assert_same_verdict("l2b-a2-out-str-whole-per-unit", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "both exact whole-program and conservative imported retention must reject the inner-arena source"
    );

    let exact_vs_fallback = diff_check_multi(
        "l2b-a2-out-str-exact-vs-fallback",
        &[
            (
                "views.align",
                "module views\npub fn install(out dst: slice<str>, selected: str, ignored: str) {\n  dst[0] = selected\n}\n",
            ),
            (
                "main.align",
                "\
import views
fn main() -> i32 {
  mut values := [\"old\"]
  arena {
    n := 42
    ignored := template \"ignored={n}\"
    views.install(values, \"static\", ignored)
  }
  if values[0] == \"static\" { return 0 }
  return 1
}
",
            ),
        ],
        "main.align",
    );
    assert!(
        !exact_vs_fallback.whole_errors && exact_vs_fallback.per_unit_errors,
        "the available body must select only the stored source while the interface-only fallback retains every compatible argument:\nwhole:\n{}\nper-unit:\n{}",
        exact_vs_fallback.whole_diags,
        exact_vs_fallback.per_unit_diags,
    );
}

#[test]
fn forwarded_and_branching_out_str_stores_retain_every_possible_source() {
    for (name, helper, call, ended_owner) in [
        (
            "forwarded",
            "fn forward(out dst: slice<str>, value: str) {\n  install(dst, value)\n}",
            "forward(values, left_view)",
            "left",
        ),
        (
            "branch",
            "fn choose(out dst: slice<str>, left: str, right: str, take_left: bool) {\n  if take_left { install(dst, left) } else { install(dst, right) }\n}",
            "choose(values, left_view, right_view, true)",
            "right",
        ),
        (
            "match",
            "fn choose_match(out dst: slice<str>, left: str, right: str, choice: Option<bool>) {\n  _ := match choice {\n    Some(_) => { install(dst, left) }\n    None => { install(dst, right) }\n  }\n}",
            "choose_match(values, left_view, right_view, Some(true))",
            "right",
        ),
        (
            "loop",
            "fn choose_loop(out dst: slice<str>, left: str, right: str, take_left: bool) {\n  loop {\n    if take_left {\n      install(dst, left)\n      break\n    }\n    install(dst, right)\n    break\n  }\n}",
            "choose_loop(values, left_view, right_view, true)",
            "right",
        ),
    ] {
        let src = format!(
            "fn install(out dst: slice<str>, value: str) {{\n  dst[0] = value\n}}\n{helper}\nfn main() -> i32 {{\n  mut values := [\"old\"]\n  mut left := \"left\".clone()\n  left_view: str := left\n  mut right := \"right\".clone()\n  right_view: str := right\n  {call}\n  {ended_owner} = \"replacement\".clone()\n  if values[0] == \"never\" {{ return 1 }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("l2b-a2-out-str-{name}"), &src);
        assert!(
            diagnostics.contains("use of invalidated borrow 'values'"),
            "the {name} summary must retain every source that may reach the out destination:\n{diagnostics}"
        );
    }
}

#[test]
fn recursive_and_try_exit_out_str_summaries_retain_the_installed_source() {
    let recursive = "\
fn retain_recursive(out dst: slice<str>, value: str, remaining: i64) {
  if remaining == 0 {
    dst[0] = value
    return
  }
  retain_recursive(dst, value, remaining - 1)
}
fn main() -> i32 {
  mut values := [\"old\"]
  mut owner := \"source\".clone()
  view: str := owner
  retain_recursive(values, view, 2)
  owner = \"replacement\".clone()
  if values[0] == \"never\" { return 1 }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-out-str-recursive", recursive);
    assert!(
        diagnostics.contains("use of invalidated borrow 'values'"),
        "recursive Out retention must converge through the direct-call fixed point:\n{diagnostics}"
    );

    let try_exit = "\
fn retain_then_try(
  out dst: slice<str>,
  value: str,
  pass: bool,
) -> Result<(), i64> {
  dst[0] = value
  result: Result<(), i64> := if pass { Ok(()) } else { Err(1) }
  _ := result?
  dst[0] = \"cleared\"
  return Ok(())
}
fn main() -> i32 {
  mut values := [\"old\"]
  mut owner := \"source\".clone()
  view: str := owner
  attempted := retain_then_try(values, view, false)
  _ := match attempted { Ok(_) => 0 Err(_) => 1 }
  owner = \"replacement\".clone()
  if values[0] == \"never\" { return 1 }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-out-str-try-exit", try_exit);
    assert!(
        diagnostics.contains("use of invalidated borrow 'values'"),
        "the `?` error exit must publish the Out destination state before the later clear:\n{diagnostics}"
    );
}

#[test]
fn two_out_str_destinations_snapshot_regions_before_updates() {
    let src = "\
fn swap(out first: slice<str>, out second: slice<str>) {
  saved := first[0]
  first[0] = second[0]
  second[0] = saved
}
fn main() -> i32 {
  arena outer {
    outer_n := 1
    outer_text := template \"outer={outer_n}\"
    mut outer_values := [outer_text]
    arena inner {
      inner_n := 2
      inner_text := template \"inner={inner_n}\"
      mut inner_values := [inner_text]
      swap(inner_values, outer_values)
    }
    if outer_values[0] == \"never\" { return 1 }
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-out-str-two-destinations", src);
    assert!(
        diagnostics.contains("cannot retain a shorter-lived view through this mutable borrow"),
        "both Out destinations and all sources must be snapshotted before either update:\n{diagnostics}"
    );
}

#[test]
fn soa_str_field_store_keeps_the_installed_owner_live() {
    let src = "\
import core.json
User { name: str, age: i64 }
fn main() -> Result<(), Error> {
  mut owner := \"source\".clone()
  view: str := owner
  arena {
    mut rows: soa<User> := json.decode(\"[{\\\"name\\\":\\\"old\\\",\\\"age\\\":1}]\")?
    rows[0].name = view
    owner = \"replacement\".clone()
    if rows[0].name == \"never\" { return Ok(()) }
  }
  return Ok(())
}
";
    let diagnostics = check_diagnostics("l2b-a2-soa-str-owner", src);
    assert!(
        diagnostics.contains("use of invalidated borrow 'rows'"),
        "AssignElemField on SoA storage must publish the installed owner to the collection root:\n{diagnostics}"
    );
}

#[test]
fn indexed_str_store_updates_aggregate_alias_observers() {
    for (name, declaration, setup, observe) in [
        (
            "struct",
            "",
            "holder := Holder { values: values }",
            "holder.values.len()",
        ),
        (
            "nested-option",
            "",
            "holder: Option<Option<Holder>> := Some(Some(Holder { values: values }))",
            "match holder { Some(outer) => match outer { Some(inner) => inner.values.len() None => 0 } None => 0 }",
        ),
        (
            "result",
            "",
            "holder: Result<Holder, i64> := Ok(Holder { values: values })",
            "match holder { Ok(inner) => inner.values.len() Err(_) => 0 }",
        ),
        (
            "user-sum",
            "Envelope { Empty, Wrapped(Holder) }",
            "holder := Envelope.Wrapped(Holder { values: values })",
            "match holder { Wrapped(inner) => inner.values.len() Empty => 0 }",
        ),
    ] {
        let src = format!(
            "Holder {{ values: slice<str> }}\n{declaration}\nfn main() -> i32 {{\n  mut values := [\"old\"]\n  {setup}\n  mut owner := \"source\".clone()\n  view: str := owner\n  values[0] = view\n  owner = \"replacement\".clone()\n  _ := {observe}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("l2b-a2-indexed-str-aggregate-observer-{name}"),
            &src,
        );
        assert!(
            diagnostics.contains("use of invalidated borrow 'holder'"),
            "a collection header nested in a {name} must observe writes to its backing:\n{diagnostics}"
        );
    }

    let tuple = "\
fn main() -> i32 {
  mut values := [\"old\"].to_array()
  mut alias: slice<str> := values
  holder := (values, 0)
  mut owner := \"source\".clone()
  view: str := owner
  alias[0] = view
  owner = \"replacement\".clone()
  _ := holder.0.len()
  return 0
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-indexed-str-aggregate-observer-tuple",
        tuple,
    );
    assert!(
        diagnostics.contains("use of invalidated borrow 'holder'"),
        "a dynamic collection header nested in a tuple must observe writes through its pre-move alias:\n{diagnostics}"
    );
}

#[test]
fn scalar_element_views_are_not_collection_alias_observers() {
    let src = "\
fn main() -> i32 {
  old_owner := \"old\".clone()
  old_view: str := old_owner
  mut values := [old_view, \"keep\"]
  old := values[0]
  mut new_owner := \"new\".clone()
  new_owner_view: str := new_owner
  values[1] = new_owner_view
  new_owner = \"replacement\".clone()
  if old == old_view { return 0 }
  return 1
}
";
    let diagnostics = check_diagnostics("l2b-a2-scalar-element-observer-control", src);
    assert!(
        diagnostics.is_empty(),
        "a scalar element view must not observe unrelated changes to its source collection:\n{diagnostics}"
    );
}

#[test]
fn borrow_mut_places_and_numeric_storage_use_argument_completion_snapshots() {
    let rebound_place = "\
fn touch(borrow mut dst: slice<i64>, ignored: i64) {
  dst[0] = ignored
}
fn main() -> i32 {
  mut first := [1]
  mut second := [2]
  mut destination: slice<i64> := first
  touch(destination, {
    destination = second
    3
  })
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-borrow-mut-place-snapshot", rebound_place);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "a later eager argument must not retarget an already-completed exclusive place:\n{diagnostics}"
    );

    let field_control = "\
Holder { values: slice<i64>, tag: i64 }
fn touch(borrow mut dst: slice<i64>, ignored: i64) {
  dst[0] = ignored
}
fn main() -> i32 {
  mut values := [1]
  mut holder := Holder { values: values, tag: 0 }
  touch(holder.values, {
    holder.tag = 1
    2
  })
  holder.values = values[..]
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-borrow-mut-disjoint-field-control", field_control);
    assert!(
        diagnostics.is_empty(),
        "a disjoint field write and a post-call rebind must not invalidate the reservation:\n{diagnostics}"
    );

    let alias_write = "\
fn touch(borrow mut dst: slice<i64>, ignored: i64) {
  dst[0] = ignored
}
fn main() -> i32 {
  mut values := [1]
  mut destination: slice<i64> := values
  mut alias: slice<i64> := destination
  touch(destination, {
    alias[0] = 4
    2
  })
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-borrow-mut-alias-write-snapshot", alias_write);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "a later write through a known alias must end the earlier exclusive reservation:\n{diagnostics}"
    );

    let numeric_storage = "\
State { values: slice<i64> }
fn retain(borrow mut state: State, borrow source: array<i64>, ignored: i64) {
  state.values = source[..]
}
fn main() -> i32 {
  mut state := State { values: [] }
  mut source := [1, 2, 3].to_array()
  retain(state, source, {
    source = [4, 5, 6].to_array()
    0
  })
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-numeric-storage-completion", numeric_storage);
    assert!(
        diagnostics.contains("value snapshot was invalidated"),
        "a non-borrowing owned array's completed storage must not be re-read after replacement:\n{diagnostics}"
    );
}

#[test]
fn nested_mutable_call_results_keep_completion_backing() {
    let src = "\
Holder { text: str, ignored: i64 }
fn extract(value: Holder) -> str = value.text
fn install(out dst: slice<str>, value: str) {
  dst[0] = value
}
fn main() -> i32 {
  outer_owner := \"outer\".clone()
  outer_view: str := outer_owner
  mut values := [\"old\"]
  arena {
    n := 42
    short := template \"short={n}\"
    mut selected: str := short
    install(values, extract(Holder {
      text: selected
      ignored: {
        selected = outer_view
        0
      }
    }))
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-nested-completion-result", src);
    assert!(
        diagnostics.contains("cannot retain a shorter-lived view through this mutable borrow"),
        "nested call/struct results must use their completed children, not live rebound syntax:\n{diagnostics}"
    );
}

#[test]
fn mutable_call_results_use_post_call_completion() {
    let src = "\
fn replace_and_return(
  borrow mut dst: slice<str>,
  source: slice<str>,
) -> slice<str> {
  dst = source
  return dst
}
fn leak(borrow mut seed: slice<str>) -> slice<str> {
  arena {
    mut destination: slice<str> := seed
    inner := [\"inner\"]
    return replace_and_return(destination, inner)
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-post-mutable-call-result", src);
    assert!(
        diagnostics.contains("cannot return a slice that views a local array"),
        "a returned borrow-mut destination must use its post-call storage snapshot:\n{diagnostics}"
    );

    let clean = "\
fn replace_and_return(
  borrow mut dst: slice<str>,
  source: slice<str>,
) -> slice<str> {
  dst = source
  return dst
}
fn select(borrow mut seed: slice<str>) -> slice<str> {
  arena {
    inner := [\"inner\"]
    mut destination: slice<str> := inner
    return replace_and_return(destination, seed)
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-post-mutable-call-result-clean", clean);
    assert!(
        diagnostics.is_empty(),
        "the post-call snapshot must also forget replaced local storage:\n{diagnostics}"
    );
}

#[test]
fn mutable_call_owned_dynamic_results_materialize_their_own_storage() {
    let dynamic = "\
fn collect(borrow mut stamp: i64, value: str) -> array<str> {
  stamp = stamp + 1
  return [value].to_array()
}
fn leak(value: str, borrow mut stamp: i64) -> slice<str> {
  return collect(stamp, value)[..]
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-mutable-call-owned-dynamic-result", dynamic);
    assert!(
        diagnostics.contains("cannot return a view that borrows local storage"),
        "a selected content owner must not become the fresh dynamic result buffer's storage:\n{diagnostics}"
    );
}

#[test]
fn mutable_call_result_backing_reaches_the_replacement_source() {
    let slice = "\
fn replace_return(
  borrow mut dst: slice<i64>,
  source: slice<i64>,
) -> slice<i64> {
  dst = source
  return dst
}
fn touch(borrow mut dst: slice<i64>, ignored: i64) {}
fn main() -> i32 {
  mut first := [1]
  mut second := [2]
  mut destination: slice<i64> := first
  mut second_view: slice<i64> := second
  touch(second_view, {
    mut alias := replace_return(destination, second_view)
    alias[0] = 3
    0
  })
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-mutable-call-slice-result-backing", slice);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "a returned replacement slice must retain the selected post-call backing:\n{diagnostics}"
    );

    let soa = "\
Point { x: i64, y: i64 }
fn replace_return(
  borrow mut dst: soa<Point>,
  source: soa<Point>,
) -> soa<Point> {
  dst = source
  return dst
}
fn touch(borrow mut rows: soa<Point>, ignored: i64) {}
fn main() -> i32 {
  arena {
    mut first := [Point { x: 1, y: 1 }].to_soa()
    mut second := [Point { x: 2, y: 2 }].to_soa()
    mut destination := first
    touch(second, {
      mut alias := replace_return(destination, second)
      alias[0].x = 3
      0
    })
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-mutable-call-soa-result-backing", soa);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "a returned replacement SoA must retain the selected post-call backing:\n{diagnostics}"
    );
}

#[test]
fn borrow_mut_slice_replacement_updates_backing_storage() {
    let src = "\
fn replace(borrow mut dst: slice<str>, source: slice<str>) {
  dst = source
}
fn main() -> i32 {
  mut outer_values := [\"outer\"]
  arena {
    mut inner_values := [\"inner\"]
    mut destination: slice<str> := inner_values
    replace(destination, outer_values)
    n := 42
    short := template \"short={n}\"
    destination[0] = short
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-borrow-mut-replacement-backing", src);
    assert!(
        diagnostics.contains("cannot be stored into a longer-lived array"),
        "a strong slice-header replacement must replace its backing fact as well:\n{diagnostics}"
    );

    let owner_tracking = "\
fn replace(borrow mut dst: slice<str>, source: slice<str>) {
  dst = source
}
fn main() -> i32 {
  mut original_values := [\"original\"]
  mut installed_values := [\"installed\"]
  mut destination: slice<str> := original_values
  replace(destination, installed_values)
  mut owner := \"source\".clone()
  view: str := owner
  destination[0] = view
  owner = \"replacement\".clone()
  if installed_values[0] == \"never\" { return 1 }
  return 0
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-borrow-mut-replacement-owner-tracking",
        owner_tracking,
    );
    assert!(
        diagnostics.contains("use of invalidated borrow 'installed_values'"),
        "MoveCheck must publish later indexed writes to the replacement backing:\n{diagnostics}"
    );
}

#[test]
fn borrow_mut_dynamic_array_heap_replacement_re_marks_storage() {
    let heap_replacement = "\
fn replace_with_heap(borrow mut dst: array<i64>) {
  dst = [4, 5, 6].to_array()
}
fn leak(out: region) -> slice<i64> {
  mut builder: array_builder<i64> := array_builder(out)
  builder.push(1)
  mut dst := builder.build()
  replace_with_heap(dst)
  return dst[..]
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics(
        "l2b-a2-borrow-mut-dynamic-replacement-heap",
        heap_replacement,
    );
    assert!(
        diagnostics.contains("cannot return a slice that views a local array"),
        "replacing caller-region storage with heap storage must re-mark the destination as local:\n{diagnostics}"
    );
}

#[test]
fn borrow_mut_dynamic_array_region_replacement_clears_local_storage() {
    let region_replacement = "\
fn replace_with_region(borrow mut dst: array<i64>, out: region) {
  mut builder: array_builder<i64> := array_builder(out)
  builder.push(7)
  builder.push(8)
  dst = builder.build()
}
fn make(out: region) -> slice<i64> {
  mut dst := [1, 2, 3].to_array()
  replace_with_region(dst, out)
  return dst[..]
}
fn main() -> i32 {
  arena out {
    values := make(out)
    if values.sum() == 15 { return 0 }
    return 1
  }
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-borrow-mut-dynamic-replacement-region",
        region_replacement,
    );
    assert!(
        diagnostics.is_empty(),
        "replacing heap storage with caller-region storage must clear obsolete local ownership:\n{diagnostics}"
    );
}

#[test]
fn region_builder_mutable_calls_keep_constructor_storage_separate_from_elements() {
    let src = "\
Item { name: str }
fn push_copy(
  borrow mut rows: array_builder<Item>,
  value: str,
  out: region,
) {
  rows.push(Item { name: value.clone_in(out) })
}
fn collect(value: str, out: region) -> array<Item> {
  mut rows: array_builder<Item> := array_builder(out)
  push_copy(rows, value, out)
  push_copy(rows, value, out)
  return rows.build()
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-region-builder-mutable-helper", src);
    assert!(
        diagnostics.is_empty(),
        "a region builder's storage is its constructor region, while copied element views keep their own provenance:\n{diagnostics}"
    );

    let retained_control = "\
fn main() -> i32 {
  arena out {
    owner := \"source\".clone()
    view: str := owner
    mut values: array_builder<str> := array_builder(out)
    values.push(view)
    values.push(\"later\")
    built := values.build()
    if built[0] == view { return 0 }
  }
  return 1
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-region-builder-reallocation-keeps-elements-valid",
        retained_control,
    );
    assert!(
        diagnostics.is_empty(),
        "later builder growth must not invalidate an earlier element whose owner remains live:\n{diagnostics}"
    );

    let retained_owner = "\
fn main() -> i32 {
  arena out {
    mut owner := \"source\".clone()
    view: str := owner
    mut values: array_builder<str> := array_builder(out)
    values.push(view)
    values.push(\"later\")
    owner = \"replacement\".clone()
    built := values.build()
    if built[0] == \"never\" { return 1 }
  }
  return 0
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-region-builder-reallocation-retains-elements",
        retained_owner,
    );
    assert!(
        diagnostics.contains("use of invalidated borrow 'values'")
            && diagnostics.contains("source 'owner'")
            && !diagnostics.contains("source 'out'"),
        "a later builder growth must not erase the owner retained by an earlier element:\n{diagnostics}"
    );
}

#[test]
fn soa_alias_reassignment_does_not_cross_publish_contents() {
    let src = "\
User { name: str, age: i64 }
fn select(rows: soa<User>) -> str {
  arena {
    mut first := rows
    alias := first
    other := [User { name: \"other\", age: 2 }].to_soa()
    first = other
    n := 42
    short := template \"short={n}\"
    first[0].name = short
    return alias[0].name
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics("l2b-a2-soa-alias-rebind-control", src);
    assert!(
        diagnostics.is_empty(),
        "reassigning a Copy SoA header must not alias its old and new column buffers:\n{diagnostics}"
    );
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
fn lifted_closure_capture_roots_drive_indirect_results() {
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
fn captured_indirect_results_resolve_to_outer_parameters() {
    let files = &[
        (
            "views.align",
            "\
module views
pub Holder { callback: fn() -> str }
pub fn captured(value: str) -> str {
  callback := fn { value }
  return callback()
}
pub fn joined(left: str, right: str, choose: bool) -> str {
  mut callback := fn { left }
  if choose { callback = fn { right } }
  return callback()
}
pub fn stored(value: str) -> str {
  holder := Holder { callback: fn { value } }
  return holder.callback()
}
",
        ),
        ("main.align", "import views\nfn main() -> i32 = 0\n"),
    ];
    let checked = assert_same_verdict(
        "l2b-captured-result-outer-summary",
        files,
        "main.align",
    );
    assert!(
        !checked.diags.has_errors(),
        "a captured caller-owned parameter may flow through an indirect result"
    );
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
    assert_eq!(find("captured").return_borrow, roots(&[0], &[]));
    assert_eq!(find("joined").return_borrow, roots(&[0, 1], &[]));
    assert_eq!(find("stored").return_borrow, roots(&[0], &[]));
}

#[test]
fn named_function_value_summaries_drive_indirect_results() {
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

#[test]
fn named_function_value_result_keeps_the_selected_owner_live() {
    let files = &[(
        "main.align",
        "\
fn identity(value: str) -> str = value
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"function value\".clone()
  view: str := owned
  f := identity
  result := f(view)
  consume(owned)
  return result.len() as i32
}
",
    )];
    let checked = assert_same_verdict("l2b-named-fn-value-owner", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "an indirect identity result must keep its selected argument owner live"
    );
}

#[test]
fn indirect_call_snapshots_the_callee_before_later_arguments() {
    let short_then_static = "\
fn leak() -> str {
  arena {
    n := 42
    short := template \"short={n}\"
    mut f := fn unused: i64 { short }
    return f({
      f = fn unused: i64 { \"static\" }
      0
    })
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics(
        "l2b-a2-indirect-callee-snapshot-short",
        short_then_static,
    );
    assert!(
        diagnostics.contains("cannot return a value allocated in an arena"),
        "the invoked closure is the short-lived callee evaluated before the rebinding argument:\n{diagnostics}"
    );

    let static_then_short = "\
fn select() -> str {
  arena {
    n := 42
    short := template \"short={n}\"
    mut f := fn unused: i64 { \"static\" }
    return f({
      f = fn unused: i64 { short }
      0
    })
  }
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics(
        "l2b-a2-indirect-callee-snapshot-static",
        static_then_short,
    );
    assert!(
        diagnostics.is_empty(),
        "a later argument must not replace the already-completed static callee capture:\n{diagnostics}"
    );
}

#[test]
fn closure_target_joins_keep_capture_slots_target_relative() {
    if !backend_available() {
        return;
    }
    let src = "\
fn consume(value: string) -> i64 = value.len()
fn main(args: array<str>) -> Result<(), Error> {
  left_owner := \"left\".clone()
  ignored_owner := \"ignored\".clone()
  right_owner := \"right hand\".clone()
  left: str := left_owner
  ignored: str := ignored_owner
  right: str := right_owner
  mut f := fn { left }
  if args.len() > 1 {
    f = fn { ignored.len(); right }
  }
  result := f()
  consume(ignored_owner)
  print(result.len())
  return Ok(())
}
";
    let left = build_and_run_args("l2b-target-relative-left", src, &[]);
    assert_eq!(left.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&left.stdout), "4\n");
    let right = build_and_run_args("l2b-target-relative-right", src, &["right"]);
    assert_eq!(right.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&right.stdout), "10\n");
}

#[test]
fn closure_target_join_keeps_every_selected_owner_live() {
    let files = &[(
        "main.align",
        "\
fn consume(value: string) -> i64 = value.len()
fn main(args: array<str>) -> i32 {
  left_owner := \"left\".clone()
  right_owner := \"right\".clone()
  left: str := left_owner
  right: str := right_owner
  mut f := fn { left }
  if args.len() > 1 { f = fn { right } }
  result := f()
  consume(left_owner)
  return result.len() as i32
}
",
    )];
    let checked = assert_same_verdict("l2b-closure-target-owner", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "a joined closure result must keep every runtime-selectable capture owner live"
    );
}

#[test]
fn closure_capture_roots_survive_struct_storage_and_projection() {
    let files = &[(
        "main.align",
        "\
Holder { callback: fn() -> str }
fn consume(value: string) -> i64 = value.len()
fn main() -> i32 {
  owned := \"stored closure\".clone()
  view: str := owned
  holder := Holder { callback: fn { view } }
  result := holder.callback()
  consume(owned)
  return result.len() as i32
}
",
    )];
    let checked = assert_same_verdict("l2b-closure-field-owner", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "a closure projected from a struct field must retain its captured owner"
    );
}

/// Frame-owned dynamic-array storage: every binding form marks the owner through the shared
/// storage-locality authority, so slices of it cannot escape by return, break, or reassignment,
/// while caller-region-built arrays stay returnable. One test per closure-matrix cell.
mod dyn_storage_locality {
    use super::*;

    #[test]
    fn return_of_dyn_array_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-return",
                "fn leak() -> slice<i64> {\n  source := [1, 2, 3].to_array()\n  return source[..]\n}\nfn main() -> i32 {\n  values := leak()\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a frame-owned dynamic array must not be returned",
        );
    }

    #[test]
    fn break_of_dyn_array_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-break",
                "fn leak() -> slice<i64> {\n  values := loop {\n    source := [1, 2, 3].to_array()\n    break source[..]\n  }\n  return values\n}\nfn main() -> i32 {\n  values := leak()\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a loop-local dynamic array must not escape through break",
        );
    }

    #[test]
    fn tuple_destructured_partition_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-lettuple",
                "fn leak() -> slice<i64> {\n  xs := [1, 2, 3, 4].to_array()\n  (evens, odds) := xs.partition(fn x: i64 { x % 2 == 0 })\n  return evens[..]\n}\nfn main() -> i32 {\n  values := leak()\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a tuple-destructured dynamic array must not be returned",
        );
    }

    #[test]
    fn match_bound_array_payload_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-match-binding",
                "Content { Empty, Data(array<i64>) }\nfn pick(fallback: slice<i64>) -> slice<i64> {\n  c := Content.Data([1, 2, 3].to_array())\n  return match c {\n    Data(arr) => arr[..]\n    Empty => fallback\n  }\n}\nfn main() -> i32 {\n  fixed := [9]\n  values := pick(fixed[..])\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a match-bound dynamic-array payload must not be returned",
        );
    }

    #[test]
    fn reassigned_dyn_array_re_marks_storage() {
        assert!(
            check_errs(
                "dyn-storage-reassign",
                "fn leak(out: region) -> slice<i64> {\n  mut builder: array_builder<i64> := array_builder(out)\n  builder.push(1)\n  mut arr := builder.build()\n  arr = [1, 2, 3].to_array()\n  return arr[..]\n}\nfn main() -> i32 {\n  arena out {\n    values := leak(out)\n    return values.sum() as i32\n  }\n}\n",
            ),
            "reassigning a caller-region array local to frame-owned storage must re-mark it",
        );
    }

    #[test]
    fn caller_region_built_slice_stays_returnable() {
        let diagnostics = check_diagnostics(
            "dyn-storage-caller-region-control",
            "fn make(out: region) -> slice<i64> {\n  mut builder: array_builder<i64> := array_builder(out)\n  builder.push(1)\n  arr := builder.build()\n  return arr[..]\n}\nfn main() -> i32 {\n  arena out {\n    values := make(out)\n    if values.sum() == 1 { return 0 }\n    return 1\n  }\n}\n",
        );
        assert!(
            diagnostics.is_empty(),
            "a slice of a caller-region-built array must stay returnable:\n{diagnostics}",
        );
    }

    #[test]
    fn struct_field_dyn_storage_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-struct-field",
                "Holder { items: array<i64> }\nfn leak() -> slice<i64> {\n  h := Holder { items: [1, 2, 3].to_array() }\n  return h.items[..]\n}\nfn main() -> i32 {\n  values := leak()\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a struct-owned dynamic array must not be returned",
        );
    }

    #[test]
    fn tuple_element_dyn_storage_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-tuple-element",
                "fn leak() -> slice<i64> {\n  t := ([1, 2, 3].to_array(), 1)\n  return t.0[..]\n}\nfn main() -> i32 {\n  values := leak()\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a tuple-owned dynamic array must not be returned",
        );
    }

    #[test]
    fn by_value_param_dyn_storage_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-by-value-param",
                "fn leak(xs: array<i64>) -> slice<i64> {\n  return xs[..]\n}\nfn main() -> i32 {\n  source := [1, 2, 3].to_array()\n  values := leak(source)\n  return values.sum() as i32\n}\n",
            ),
            "a slice of a by-value Move array parameter must not be returned",
        );
    }

    #[test]
    fn mixed_provenance_aggregate_slice_rejected() {
        assert!(
            check_errs(
                "dyn-storage-mixed-aggregate",
                "State { text: str }\nWrap { items: array<i64>, text: str }\nfn leak(borrow mut s: State) -> slice<i64> {\n  w := Wrap { items: [1, 2, 3].to_array(), text: s.text }\n  return w.items[..]\n}\nfn main() -> i32 {\n  mut s := State { text: \"\" }\n  values := leak(s)\n  if values.sum() == 6 { return 0 }\n  return 1\n}\n",
            ),
            "a caller view beside a frame-owned array must not launder the array's storage locality",
        );
    }

    #[test]
    fn region_built_aggregate_slice_stays_returnable() {
        let diagnostics = check_diagnostics(
            "dyn-storage-region-aggregate-control",
            "Wrap { items: array<i64> }\nfn make(out: region) -> slice<i64> {\n  mut builder: array_builder<i64> := array_builder(out)\n  builder.push(1)\n  w := Wrap { items: builder.build() }\n  return w.items[..]\n}\nfn main() -> i32 {\n  arena out {\n    values := make(out)\n    if values.sum() == 1 { return 0 }\n    return 1\n  }\n}\n",
        );
        assert!(
            diagnostics.is_empty(),
            "an aggregate built wholly from a caller region must stay returnable:\n{diagnostics}",
        );
    }

    #[test]
    fn outer_array_sliced_inside_arena_stays_valid() {
        let diagnostics = check_diagnostics(
            "dyn-storage-arena-slice-control",
            "State { values: slice<i64> }\nfn main() -> i32 {\n  xs := [1, 2, 3]\n  mut state := State { values: [] }\n  arena scratch {\n    state.values = xs[..]\n  }\n  values := state.values\n  if values.sum() == 6 { return 0 }\n  return 1\n}\n",
        );
        assert!(
            diagnostics.is_empty(),
            "slicing an outer-frame array inside an arena must keep its declaration scope:\n{diagnostics}",
        );
    }
}

#[test]
fn unknown_soa_call_results_preserve_source_backing_roots() {
    let forwarded = "\
User { name: str, age: i64 }
fn identity(rows: soa<User>) -> soa<User> = rows
fn main() -> i32 {
  old_owner := \"old\".clone()
  old_view: str := old_owner
  mut new_owner := \"new\".clone()
  new_view: str := new_owner
  arena {
    mut rows := [User { name: old_view, age: 1 }].to_soa()
    mut alias := identity(rows)
    alias[0].name = new_view
    new_owner = \"replacement\".clone()
    if rows[0].name == \"never\" { return 1 }
  }
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-unknown-soa-result-backing", forwarded);
    assert!(
        diagnostics.contains("use of invalidated borrow 'rows'"),
        "a call-returned SoA header must keep the source backing roots for later writes:\n{diagnostics}"
    );

    let fresh_control = "\
User { name: str, age: i64 }
fn main() -> i32 {
  old_owner := \"old\".clone()
  old_view: str := old_owner
  mut new_owner := \"new\".clone()
  new_view: str := new_owner
  arena {
    rows := [User { name: old_view, age: 1 }].to_soa()
    mut fresh := [User { name: \"fresh\", age: 2 }].to_soa()
    fresh[0].name = new_view
    new_owner = \"replacement\".clone()
    if rows[0].name == old_view { return 0 }
  }
  return 1
}
";
    let diagnostics = check_diagnostics("l2b-a2-fresh-soa-backing-control", fresh_control);
    assert!(
        diagnostics.is_empty(),
        "a direct to_soa allocation must keep a fresh backing unrelated to older rows:\n{diagnostics}"
    );
}

#[test]
fn rand_receiver_mutation_invalidates_an_earlier_borrow_mut_place() {
    let same_rng = "\
import std.rand
fn touch(borrow mut value: rng, ignored: i64) {}
fn main() -> i32 {
  mut r := rand.seed_with(1)
  touch(r, r.next())
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-rand-receiver-reservation", same_rng);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "advancing the same rng in a later argument must invalidate the earlier exclusive place:\n{diagnostics}"
    );
}

#[test]
fn mutating_a_distinct_rng_keeps_an_earlier_borrow_mut_place_valid() {
    let distinct_rng = "\
import std.rand
fn touch(borrow mut value: rng, ignored: i64) {}
fn main() -> i32 {
  mut first := rand.seed_with(1)
  mut second := rand.seed_with(2)
  touch(first, second.next())
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-rand-receiver-reservation-control", distinct_rng);
    assert!(
        diagnostics.is_empty(),
        "advancing a distinct rng must leave the earlier exclusive place valid:\n{diagnostics}"
    );
}

#[test]
fn shuffle_through_a_backing_alias_invalidates_an_earlier_borrow_mut_place() {
    let aliased = "\
import std.rand
fn touch(borrow mut dst: slice<i64>, ignored: ()) {}
fn main() -> i32 {
  mut values := [1, 2, 3]
  mut destination: slice<i64> := values
  mut alias: slice<i64> := values
  mut r := rand.seed_with(1)
  touch(destination, r.shuffle(alias))
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-shuffle-alias-reservation", aliased);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "shuffling through a backing alias must invalidate the earlier exclusive slice place:\n{diagnostics}"
    );
}

#[test]
fn shuffling_a_distinct_backing_keeps_an_earlier_borrow_mut_place_valid() {
    let distinct = "\
import std.rand
fn touch(borrow mut dst: slice<i64>, ignored: ()) {}
fn main() -> i32 {
  mut first := [1, 2, 3]
  mut second := [4, 5, 6]
  mut destination: slice<i64> := first
  mut shuffled: slice<i64> := second
  mut r := rand.seed_with(1)
  touch(destination, r.shuffle(shuffled))
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-shuffle-alias-reservation-control", distinct);
    assert!(
        diagnostics.is_empty(),
        "shuffling a distinct backing must leave the earlier exclusive slice valid:\n{diagnostics}"
    );
}

#[test]
fn map_into_through_a_backing_alias_invalidates_an_earlier_borrow_mut_place() {
    let aliased = "\
fn touch(borrow mut dst: slice<i64>, ignored: ()) {}
fn main() -> i32 {
  source := [4, 5, 6]
  mut values := [1, 2, 3]
  mut destination: slice<i64> := values
  mut alias: slice<i64> := values
  touch(destination, source.map_into(alias))
  return 0
}
";
    let diagnostics = check_diagnostics("l2b-a2-map-into-alias-reservation", aliased);
    assert!(
        diagnostics.contains("borrow mut argument place was invalidated"),
        "map_into through a backing alias must invalidate the earlier exclusive slice place:\n{diagnostics}"
    );
}

#[test]
fn nested_mutable_call_results_snapshot_post_call_owners() {
    let old_owner_control = "\
fn replace_and_return(
  borrow mut dst: slice<str>,
  source: slice<str>,
) -> slice<str> {
  dst = source
  return dst
}
fn observe(value: slice<str>, ignored: i32) -> i32 {
  if value.len() > 0 { return ignored }
  return 0
}
fn main() -> i32 {
  mut old_owner := \"old\".clone()
  old_view: str := old_owner
  new_owner := \"new\".clone()
  new_view: str := new_owner
  mut old_values := [old_view]
  mut new_values := [new_view]
  mut destination: slice<str> := old_values
  length := observe(
    replace_and_return(destination, new_values),
    {
      old_owner = \"replacement\".clone()
      0
    },
  )
  return length
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-post-call-result-old-owner-control",
        old_owner_control,
    );
    assert!(
        diagnostics.is_empty(),
        "a mutable-call result must forget the replaced destination's old owner:\n{diagnostics}"
    );

    let selected_owner = "\
fn replace_and_return(
  borrow mut dst: slice<str>,
  source: slice<str>,
) -> slice<str> {
  dst = source
  return dst
}
fn observe(value: slice<str>, ignored: i32) -> i32 {
  if value.len() > 0 { return ignored }
  return 0
}
fn main() -> i32 {
  old_owner := \"old\".clone()
  old_view: str := old_owner
  mut new_owner := \"new\".clone()
  new_view: str := new_owner
  mut old_values := [old_view]
  mut new_values := [new_view]
  mut destination: slice<str> := old_values
  length := observe(
    replace_and_return(destination, new_values),
    {
      new_owner = \"replacement\".clone()
      0
    },
  )
  return length
}
";
    let diagnostics = check_diagnostics(
        "l2b-a2-post-call-result-selected-owner",
        selected_owner,
    );
    assert!(
        diagnostics.contains("value snapshot was invalidated"),
        "a mutable-call result must retain the replacement source owner while a later argument is evaluated:\n{diagnostics}"
    );
}
