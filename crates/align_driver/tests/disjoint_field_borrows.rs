//! Tests for disjoint struct field borrowing and path-scoped borrow invalidation (#1050).

mod common;
use common::*;

#[test]
fn disjoint_struct_field_borrow_call() {
    let src = "\
pub State {
  owner: i64,
  slot: buffer,
  key: string,
}

fn worker(borrow owner: i64, borrow mut slot: slice<u8>) {
  slot.set_u8(0, 1)
}

pub fn run(borrow mut state: State) {
  mut view := state.slot.bytes()
  worker(state.owner, view)
}

pub fn run_with_mutation(borrow mut state: State) {
  mut view := state.slot.bytes()
  state.key = \"new\".clone()
  worker(state.owner, view)
}

fn main() -> i32 {
  mut state := State {
    owner: 42,
    slot: buffer.filled(16, 0),
    key: \"init\".clone(),
  }
  run(state)
  run_with_mutation(state)
  if state.slot.bytes()[0] == 1 { return 0 }
  return 1
}
";
    let output = build_and_run("disjoint-field-borrow-basic", src);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn disjoint_nested_struct_field_borrow_call() {
    let src = "\
pub Inner {
  count: i64,
  data: buffer,
}

pub Container {
  inner: Inner,
  tag: i64,
}

fn mutate_data(borrow mut data: slice<u8>, borrow count: i64) {
  data.set_u8(0, count as u8)
}

pub fn run_nested(borrow mut c: Container) {
  mut view := c.inner.data.bytes()
  mutate_data(view, c.inner.count)
}

fn main() -> i32 {
  mut c := Container {
    inner: Inner {
      count: 7,
      data: buffer.filled(8, 0),
    },
    tag: 99,
  }
  run_nested(c)
  if c.inner.data.bytes()[0] == 7 { return 0 }
  return 1
}
";
    let output = build_and_run("disjoint-nested-field-borrow", src);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn reject_prefix_overlap_struct_and_field() {
    let src = "\
pub State {
  owner: i64,
  slot: buffer,
}

fn call_both(borrow mut s: State, borrow o: i64) {}

pub fn run(borrow mut state: State) {
  call_both(state, state.owner)
}

fn main() {}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "prefix-overlap", src);
    assert!(checked.diags.has_errors());
    let formatted = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        formatted.contains("aliases argument"),
        "expected alias error for prefix overlap, got:\n{formatted}"
    );
}

#[test]
fn reject_same_field_alias() {
    let src = "\
pub State {
  slot: buffer,
}

fn call_views(borrow mut a: slice<u8>, borrow b: slice<u8>) {}

pub fn run(borrow mut state: State) {
  mut view := state.slot.bytes()
  call_views(view, view)
}

fn main() {}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "same-field-alias", src);
    assert!(checked.diags.has_errors());
    let formatted = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        formatted.contains("aliases argument"),
        "expected alias error for same field passed twice, got:\n{formatted}"
    );
}

#[test]
fn reject_shared_slice_fields_alias() {
    let src = "\
pub StateSlices {
  s1: slice<u8>,
  s2: slice<u8>,
}

fn call_slices(borrow mut a: slice<u8>, borrow b: slice<u8>) {}

pub fn run(borrow mut state: StateSlices) {
  call_slices(state.s1, state.s2)
}

fn main() {}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "shared-slice-fields", src);
    assert!(checked.diags.has_errors());
    let formatted = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        formatted.contains("aliases argument"),
        "expected alias error for potentially shared slice fields, got:\n{formatted}"
    );
}

#[test]
fn reject_rebound_view_alias() {
    let src = "\
pub State {
  slot: buffer,
}

fn call_views(borrow mut a: slice<u8>, borrow b: slice<u8>) {}

pub fn run(borrow mut state: State) {
  mut v1 := state.slot.bytes()
  mut v2 := v1
  call_views(v1, v2)
}

fn main() {}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "rebound-view-alias", src);
    assert!(checked.diags.has_errors());
    let formatted = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        formatted.contains("aliases argument"),
        "expected alias error for rebound view alias, got:\n{formatted}"
    );
}

#[test]
fn reject_rebound_view_via_call_alias() {
    let src = "\
pub State {
  a: buffer,
  b: buffer,
}

fn replace_view(borrow mut v: slice<u8>, borrow new_source: slice<u8>) {
  v = new_source
}

fn call_views(borrow mut a: slice<u8>, borrow b: slice<u8>) {}

pub fn run(borrow mut state: State) {
  mut v := state.a.bytes()
  mut vb := state.b.bytes()
  replace_view(v, vb)
  call_views(vb, v)
}

fn main() {}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "rebound-view-via-call-alias", src);
    assert!(checked.diags.has_errors());
    let formatted = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        formatted.contains("aliases argument"),
        "expected alias error when mutable call replaced view descriptor, got:\n{formatted}"
    );
}

#[test]
fn independent_str_descriptor_borrow_mut_and_by_value() {
    let src = "\
fn replace_str(borrow mut dst: str, src: str) {
  dst = src
}

pub fn probe_str(owner: str) {
  mut x := owner
  y := owner
  replace_str(x, y)
}

fn main() -> i32 {
  probe_str(\"hello\")
  return 0
}
";
    let output = build_and_run("independent-str-descriptors", src);
    assert_eq!(output.status.code(), Some(0));
}
