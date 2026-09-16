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
