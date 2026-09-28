//! Tests for `array<T>.truncate(new_len: i64) -> ()` (Plan 66 ledger T).

mod common;
use common::*;

#[test]
fn array_truncate_reduces_length_and_preserves_prefix() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut arr := [10, 20, 30, 40, 50].to_array()
  arr.truncate(3)
  if arr.len() != 3 { return 1 }
  if arr[0] != 10 { return 2 }
  if arr[1] != 20 { return 3 }
  if arr[2] != 30 { return 4 }

  // Truncate again down to 1
  arr.truncate(1)
  if arr.len() != 1 { return 5 }
  if arr[0] != 10 { return 6 }

  // Truncate to 0
  arr.truncate(0)
  if arr.len() != 0 { return 7 }

  // Same-length truncate is a no-op
  arr.truncate(0)
  if arr.len() != 0 { return 8 }

  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-prefix", src).status.code(), Some(0));
}

#[test]
fn array_truncate_same_length_preserves_elements() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut arr := [100, 200, 300].to_array()
  arr.truncate(3)
  if arr.len() != 3 { return 1 }
  if arr[0] != 100 || arr[1] != 200 || arr[2] != 300 { return 2 }
  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-same-len", src).status.code(), Some(0));
}

#[test]
fn array_truncate_field_path() {
    if !backend_available() {
        return;
    }
    let src = "\
Container {
  id: i64,
  data: array<i64>
}
Wrapper {
  tag: i64,
  c: Container
}
fn main() -> i32 {
  mut w := Wrapper {
    tag: 1,
    c: Container {
      id: 42,
      data: [1, 2, 3, 4].to_array()
    }
  }
  w.c.data.truncate(2)
  if w.c.data.len() != 2 { return 1 }
  if w.c.data[0] != 1 || w.c.data[1] != 2 { return 2 }
  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-field-path", src).status.code(), Some(0));
}

#[test]
fn array_truncate_drops_suffix_strings() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut b: array_builder<string> := array_builder()
  b.push(\"alpha\".clone())
  b.push(\"beta\".clone())
  b.push(\"gamma\".clone())
  mut arr := b.build()
  arr.truncate(1)
  if arr.len() != 1 { return 1 }
  if arr[0] != \"alpha\" { return 2 }
  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-strings", src).status.code(), Some(0));
}

#[test]
fn array_truncate_drops_suffix_move_structs() {
    if !backend_available() {
        return;
    }
    let src = "\
Item {
  name: string,
  val: i64
}
fn main() -> i32 {
  mut b: array_builder<Item> := array_builder()
  b.push(Item { name: \"first\".clone(), val: 1 })
  b.push(Item { name: \"second\".clone(), val: 2 })
  b.push(Item { name: \"third\".clone(), val: 3 })
  mut arr := b.build()
  arr.truncate(2)
  if arr.len() != 2 { return 1 }
  if arr[0].val != 1 || arr[0].name != \"first\" { return 2 }
  if arr[1].val != 2 || arr[1].name != \"second\" { return 3 }
  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-move-structs", src).status.code(), Some(0));
}

#[test]
fn array_truncate_out_of_bounds_negative_aborts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut arr := [1, 2, 3].to_array()
  arr.truncate(-1)
  return 0
}
";
    let output = build_and_run("arr-trunc-oob-neg", src);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("align: panic: slice range out of bounds: 0..-1 is not within length 3"),
        "expected range fail panic in stderr, got:\n{stderr}"
    );
}

#[test]
fn array_truncate_out_of_bounds_excess_aborts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  mut arr := [1, 2, 3].to_array()
  arr.truncate(5)
  return 0
}
";
    let output = build_and_run("arr-trunc-oob-excess", src);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("align: panic: slice range out of bounds: 0..5 is not within length 3"),
        "expected range fail panic in stderr, got:\n{stderr}"
    );
}

#[test]
fn array_truncate_sema_diagnostics() {
    // Immutable receiver
    let diag = check_diagnostics(
        "trunc-immutable",
        "fn main() { arr := [1, 2, 3].to_array()\n arr.truncate(1) }\n",
    );
    assert!(diag.contains("cannot truncate immutable 'arr' (declare with `mut`)"));

    // Fixed array receiver
    let diag = check_diagnostics(
        "trunc-fixed-arr",
        "fn main() { mut arr := [1, 2, 3]\n arr.truncate(1) }\n",
    );
    assert!(diag.contains("'truncate' operates on dynamic arrays, got array<i64>[3]"));

    // Slice receiver
    let diag = check_diagnostics(
        "trunc-slice",
        "fn main() { mut sl := [1, 2, 3][0..2]\n sl.truncate(1) }\n",
    );
    assert!(diag.contains("'truncate' operates on dynamic arrays, got slice<i64>"));

    // Non-place receiver
    let diag = check_diagnostics(
        "trunc-non-place",
        "fn main() { [1, 2, 3].to_array().truncate(1) }\n",
    );
    assert!(diag.contains("'truncate' needs a mutable place"));

    // Non-i64 length
    let diag = check_diagnostics(
        "trunc-bad-len",
        "fn main() { mut arr := [1, 2].to_array()\n arr.truncate(\"not_len\") }\n",
    );
    assert!(diag.contains("'truncate' length must be i64, got str"));

    // Wrong arity
    let diag = check_diagnostics(
        "trunc-wrong-arity",
        "fn main() { mut arr := [1, 2].to_array()\n arr.truncate(1, 2) }\n",
    );
    assert!(diag.contains("'truncate' takes 1 argument (new_len: i64), got 2"));
}

#[test]
fn array_truncate_borrow_mut_param() {
    if !backend_available() {
        return;
    }
    let src = "\
fn truncate_helper(borrow mut a: array<i64>) {
  a.truncate(2)
}
fn main() -> i32 {
  mut arr := [10, 20, 30, 40].to_array()
  truncate_helper(arr)
  if arr.len() != 2 { return 1 }
  if arr[0] != 10 || arr[1] != 20 { return 2 }
  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-borrow-mut", src).status.code(), Some(0));
}

#[test]
fn array_truncate_extreme_bounds_abort() {
    if !backend_available() {
        return;
    }
    let src_min = "\
fn main() -> i32 {
  mut arr := [1, 2, 3].to_array()
  arr.truncate(-9223372036854775808)
  return 0
}
";
    let out = build_and_run("arr-trunc-i64-min", src_min);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("align: panic: slice range out of bounds: 0..-9223372036854775808 is not within length 3"));

    let src_max = "\
fn main() -> i32 {
  mut arr := [1, 2, 3].to_array()
  arr.truncate(9223372036854775807)
  return 0
}
";
    let out = build_and_run("arr-trunc-i64-max", src_max);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("align: panic: slice range out of bounds: 0..9223372036854775807 is not within length 3"));
}

#[test]
fn array_truncate_in_pure_context() {
    if !backend_available() {
        return;
    }
    let src = "\
fn truncate_sum(n: i64) -> i64 {
  mut arr := [n, n * 2, n * 3].to_array()
  arr.truncate(2)
  return arr.sum()
}
fn main() -> i32 {
  xs := [1, 2, 3, 4]
  ys := xs.par_map(truncate_sum)
  if ys[0] != 3 { return 1 }
  if ys[1] != 6 { return 2 }
  if ys[2] != 9 { return 3 }
  if ys[3] != 12 { return 4 }
  return 0
}
";
    assert_eq!(build_and_run("arr-trunc-par-map", src).status.code(), Some(0));
}

#[test]
fn truncate_excludes_old_observations_in_both_checking_modes() {
    // Negative ownership witnesses are checked only, never executed.
    let cases = [
        ("record-sibling-alias", "mut a := [10, 20, 30].to_array()\n v := a[..]\n mut p := Aliased { view: v, data: a }\n p.data.truncate(1)\n return p.view[0] as i32"),
        ("exclusive-slice-alias", "mut a := [10, 20, 30].to_array()\n mut v := a[..]\n old := a[..]\n touch(v)\n return old[0] as i32"),
        ("slice", "mut a := [10, 20, 30].to_array()\n v := a[..]\n a.truncate(1)\n return v[0] as i32"),
        ("branch", "mut a := [10, 20, 30].to_array()\n v := a[..]\n if a.len() == 3 { a.truncate(1) }\n return v[0] as i32"),
        ("loop", "mut a := [10, 20, 30].to_array()\n v := a[..]\n loop { a.truncate(1)\n break }\n return v[0] as i32"),
        ("same-length", "mut a := [10, 20, 30].to_array()\n v := a[..]\n a.truncate(3)\n return v[0] as i32"),
        ("string", "mut b: array_builder<string> := array_builder()\n b.push(\"alpha\".clone())\n b.push(\"beta\".clone())\n mut a := b.build()\n v := a[1]\n a.truncate(1)\n return v.len() as i32"),
        ("nested-view", "mut a := [10, 20, 30].to_array()\n v := Some(a[..])\n a.truncate(1)\n s := v else { return 0 }\n return s[0] as i32"),
        ("eager", "mut a := [10, 20, 30].to_array()\n return observe(a[..], { a.truncate(1)\n 0 })"),
        ("moved-receiver", "mut a := [10, 20, 30].to_array()\n a.truncate(consume(a))\n return 0"),
        ("replaced-receiver", "mut a := [10, 20, 30].to_array()\n a.truncate({ a = [1, 2].to_array()\n 1 })\n return 0"),
        ("mutated-receiver", "mut a := [10, 20, 30].to_array()\n a.truncate({ a.truncate(2)\n 1 })\n return 0"),
    ];
    for (name, body) in cases {
        let source = format!("Aliased {{ view: slice<i64>, data: array<i64> }}\nfn touch(borrow mut v: slice<i64>) {{}}\nfn consume(a: array<i64>) -> i64 = 0\nfn observe(a: slice<i64>, n: i64) -> i32 = a[0] as i32\nfn main() -> i32 {{\n {body}\n}}\n");
        let checked = diff_check_multi(name, &[("main.align", &source)], "main.align");
        for (failed, diagnostics) in [(checked.whole_errors, checked.whole_diags), (checked.per_unit_errors, checked.per_unit_diags)] {
            assert!(failed, "{name}: old observation unexpectedly accepted");
            assert!(diagnostics.contains("invalidated") || diagnostics.contains("borrow") || diagnostics.contains("moved"), "{name}: unrelated diagnostic: {diagnostics}");
        }
    }
}

#[test]
fn truncate_rejects_borrowed_record_view_field_aliases() {
    // A caller can store a view beside its owner. Distinct symbolic field paths inside the
    // callee do not prove that the view addresses independent storage.
    for (name, body) in [
        ("saved", "old := p.view\n p.data.truncate(1)\n return old[2] as i32"),
        ("field", "p.data.truncate(1)\n return p.view[2] as i32"),
        ("eager", "return observe(p.view, { p.data.truncate(1)\n 0 })"),
    ] {
        let source = format!("Aliased {{ view: slice<i64>, data: array<i64> }}\nfn observe(v: slice<i64>, n: i64) -> i32 = v[2] as i32\nfn bad(borrow mut p: Aliased) -> i32 {{\n {body}\n}}\nfn main() -> i32 {{\n mut a := [10, 20, 30].to_array()\n v := a[..]\n mut p := Aliased {{ view: v, data: a }}\n return bad(p)\n}}\n");
        let checked = diff_check_multi(name, &[("main.align", &source)], "main.align");
        for (failed, diagnostics) in [(checked.whole_errors, checked.whole_diags), (checked.per_unit_errors, checked.per_unit_diags)] {
            assert!(failed, "{name}: aliased borrowed field unexpectedly accepted");
            assert!(diagnostics.contains("invalidated borrow") || diagnostics.contains("value snapshot was invalidated"), "{name}: unrelated diagnostic: {diagnostics}");
        }
    }
}

#[test]
fn truncate_in_imported_helper_excludes_callers_old_view() {
    let library = "module lib\npub fn shorten(borrow mut a: array<i64>) { a.truncate(1) }\n";
    let source = "import lib\nfn main() -> i32 {\n mut a := [1, 2, 3].to_array()\n v := a[..]\n lib.shorten(a)\n return v[0] as i32\n}\n";
    let checked = diff_check_multi("trunc-import", &[("main.align", source), ("lib.align", library)], "main.align");
    assert!(checked.whole_errors, "{}", checked.whole_diags);
    assert!(checked.per_unit_errors, "{}", checked.per_unit_diags);
    for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
        assert!(diagnostics.contains("invalid") || diagnostics.contains("borrow"), "{diagnostics}");
        assert!(!diagnostics.contains("module file"), "{diagnostics}");
    }
    let positive = source.replace("v := a[..]\n lib.shorten(a)", "lib.shorten(a)\n v := a[..]");
    let checked = diff_check_multi("trunc-import-fresh", &[("main.align", &positive), ("lib.align", library)], "main.align");
    assert!(!checked.whole_errors, "{}", checked.whole_diags);
    assert!(!checked.per_unit_errors, "{}", checked.per_unit_diags);
}

#[test]
fn truncate_preserves_fresh_views_siblings_and_terminated_operands() {
    let source = r#"
Pair { left: array<i64>, right: array<i64> }
fn touch(borrow mut view: slice<i64>) {}
fn preserve_sibling(borrow mut p: Pair) -> i32 {
    saved := p.right[..]
    p.left.truncate(1)
    fresh := p.right[..]
    return (saved[1] + fresh[1]) as i32
}
fn stop() -> i32 {
    mut a := [1, 2, 3].to_array()
    a.truncate({ unused := consume(a)
        return 7
    })
    return 0
}
fn consume(a: array<i64>) -> i64 = 0
fn main() -> i32 {
    mut p := Pair { left: [1, 2, 3].to_array(), right: [4, 5].to_array() }
    old := p.left[..]
    if old[2] != 3 { return 1 }
    sibling := p.right[..]
    p.left.truncate(1)
    fresh := p.left[..]
    if fresh[0] != 1 || sibling[1] != 5 { return 2 }
    p.left.truncate(0)
    if p.left.len() != 0 { return 3 }
    mut data := [1, 2].to_array()
    mut view := data[..]
    touch(view)
    if data[0] != 1 || view[1] != 2 { return 4 }
    p.left = [1, 2].to_array()
    if preserve_sibling(p) != 10 { return 5 }
    return stop() - 7
}
"#;
    let checked = diff_check_multi("trunc-positive", &[("main.align", source)], "main.align");
    assert!(!checked.whole_errors, "{}", checked.whole_diags);
    assert!(!checked.per_unit_errors, "{}", checked.per_unit_diags);
    if backend_available() {
        assert_eq!(build_and_run("trunc-positive", source).status.code(), Some(0));
    }
}
