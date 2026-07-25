//! Unit is a real source-language value even though a `()`-returning call uses LLVM `void`.
//! MIR must keep the call for its effects while giving every value context `Const::Unit`.

mod common;
use common::*;

#[test]
fn direct_unit_calls_work_in_value_contexts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn unit(n: i32) {
  print(n)
}
fn take(x: ()) {
  print(9)
}
fn tail() {
  return unit(5)
}
fn borrow(s: str) {}
fn main() -> i32 {
  a := unit(1)
  take(unit(2))
  b := { unit(3) }
  c := arena { unit(4) }
  borrow(\"temporary\".clone())
  tail()
  return 0
}
";
    let out = build_and_run("unit-direct-value", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n9\n3\n4\n5\n");
}

#[test]
fn indirect_unit_calls_work_in_value_contexts() {
    if !backend_available() {
        return;
    }
    let src = "\
fn unit() {
  print(7)
}
fn take(x: ()) {
  print(8)
}
fn tail(f: fn() -> ()) {
  return f()
}
fn main() -> i32 {
  f := unit
  a := f()
  take(f())
  b := { f() }
  c := arena { f() }
  tail(f)
  return 0
}
";
    let out = build_and_run("unit-indirect-value", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n7\n8\n7\n7\n7\n");
}

#[test]
fn map_err_unit_converter_uses_the_indirect_unit_rule() {
    if !backend_available() {
        return;
    }
    let src = "\
fn inner() -> Result<i32, ()> = Err(())
fn convert(e: ()) {
  print(6)
}
fn main() -> i32 {
  mapped := inner().map_err(convert)
  return 0
}
";
    let out = build_and_run("unit-map-err", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n");
}

#[test]
fn empty_blocks_and_loops_produce_typed_unit_values() {
    if !backend_available() {
        return;
    }
    let src = "\
fn unit() {
  print(6)
}
fn take(x: ()) {
  print(9)
}
fn main() -> i32 {
  empty := {}
  take(empty)
  looped := loop { break unit() }
  take(looped)
  return 0
}
";
    let out = build_and_run("unit-block-loop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9\n6\n9\n");
}

#[test]
fn terminated_unit_arguments_do_not_emit_outer_calls() {
    if !backend_available() {
        return;
    }
    let direct = "\
fn make() -> string = \"x\".clone()
fn take(s: string, x: ()) {
  print(9)
}
fn main() -> i32 {
  take(make(), { return 0 })
  return 1
}
";
    let direct_out = build_and_run("unit-return-direct-arg", direct);
    assert_eq!(direct_out.status.code(), Some(0));
    assert!(direct_out.stdout.is_empty(), "the direct outer call must not execute");

    let indirect = "\
fn make() -> string = \"x\".clone()
fn take(s: string, x: ()) {
  print(9)
}
fn main() -> i32 {
  f := take
  f(make(), { return 0 })
  return 1
}
";
    let indirect_out = build_and_run("unit-return-indirect-arg", indirect);
    assert_eq!(indirect_out.status.code(), Some(0));
    assert!(indirect_out.stdout.is_empty(), "the indirect outer call must not execute");

    let wrapped_direct = "\
fn make() -> string = \"x\".clone()
fn take(t: (string, i64), x: ()) {
  print(9)
}
fn main() -> i32 {
  s := make()
  take((s, 1), { return 0 })
  return 1
}
";
    let wrapped_direct_out = build_and_run("unit-return-wrapped-direct-arg", wrapped_direct);
    assert_eq!(wrapped_direct_out.status.code(), Some(0));
    assert!(
        wrapped_direct_out.stdout.is_empty(),
        "the wrapped direct argument must be dropped exactly once without calling take"
    );

    let partial_tuple = "\
fn make() -> string = \"x\".clone()
fn take(t: (string, i64)) {
  print(9)
}
fn main() -> i32 {
  take((make(), { return 0 }))
  return 1
}
";
    let partial_tuple_out = build_and_run("unit-return-partial-tuple-arg", partial_tuple);
    assert_eq!(partial_tuple_out.status.code(), Some(0));
    assert!(
        partial_tuple_out.stdout.is_empty(),
        "a partially built tuple argument must clean up without calling take"
    );

    let partial_struct = "\
Pair { text: string, n: i64 }
fn make() -> string = \"x\".clone()
fn take(p: Pair) {
  print(9)
}
fn main() -> i32 {
  take(Pair { text: make(), n: { return 0 } })
  return 1
}
";
    let partial_struct_out = build_and_run("unit-return-partial-struct-arg", partial_struct);
    assert_eq!(partial_struct_out.status.code(), Some(0));
    assert!(
        partial_struct_out.stdout.is_empty(),
        "a partially built struct argument must clean up without calling take"
    );
}

#[test]
fn completed_struct_arguments_transfer_bound_owned_fields_once() {
    if !backend_available() {
        return;
    }
    let direct = "\
Pair { text: string }
fn make() -> string = \"x\".clone()
fn take(p: Pair) -> i64 = p.text.len()
fn main() -> i32 {
  s := make()
  n := take(Pair { text: s })
  return n as i32
}
";
    let direct_out = build_and_run("unit-complete-struct-bound-field", direct);
    assert_eq!(direct_out.status.code(), Some(1));

    let nested = "\
Inner { text: string }
Outer { inner: Inner }
fn make() -> string = \"xy\".clone()
fn take(p: Outer) -> i64 = p.inner.text.len()
fn main() -> i32 {
  s := make()
  n := take(Outer { inner: Inner { text: s } })
  return n as i32
}
";
    let nested_out = build_and_run("unit-complete-nested-struct-bound-field", nested);
    assert_eq!(nested_out.status.code(), Some(2));
}

#[test]
fn pipeline_unit_callables_use_unit_values() {
    if !backend_available() {
        return;
    }
    // Pipeline callables are lowered directly into fused MIR loops rather than through the ordinary
    // expression-call helper. They must apply the same LLVM-void / Align-Unit normalization.
    let src = "\
fn effect(x: i64) {
  print(x)
}
fn truth(x: ()) -> bool = true
fn fold(acc: (), x: i64) {
  print(x)
}
fn main() -> i32 {
  all := [1, 2].map(effect).any(truth)
  reduced := [3, 4].reduce((), fold)
  units := [5, 6].map(effect).to_array()
  prefix := [7, 8].scan((), fold)
  if !all { return 1 }
  return (units.len() + prefix.len()) as i32
}
";
    let out = build_and_run("unit-pipeline-callables", src);
    assert_eq!(out.status.code(), Some(4));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1\n2\n3\n4\n5\n6\n7\n8\n"
    );
}
