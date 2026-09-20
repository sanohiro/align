//! Whole-struct **value** semantics (Slice 2 of `docs/impl/08-nested-structs.md`): reading a whole
//! (inner) struct into a local, passing a struct by value, returning a struct by value, and
//! struct-to-struct assignment. Plain-data structs are Copy, so these are sound memcpys. Lifted into
//! working order by the Slice 1 field-path generalization (`Field`/`Load`/`Store` handle a struct
//! value); these tests lock the behavior in across the SysV by-value ABI (mixed widths, floats,
//! nesting).

mod common;
use common::*;

#[test]
fn read_whole_inner_struct() {
    if !backend_available() {
        return;
    }
    // `p := l.a` binds a copy of the inner struct; reading its fields gives 5 + 6 = 11.
    let src = concat!(
        "Point { x: i64, y: i64 }\nLine { a: Point, b: Point }\n",
        "fn main() -> i32 {\n",
        "  l := Line{a: Point{x:5,y:6}, b: Point{x:7,y:8}}\n",
        "  p := l.a\n",
        "  return (p.x + p.y) as i32\n}\n",
    );
    assert_eq!(build_and_run("read-inner", src).status.code(), Some(11));
}

#[test]
fn struct_param_and_return_by_value() {
    if !backend_available() {
        return;
    }
    // Pass a struct by value into `sum`, and build one by value in `mk`; 3 + 4 = 7, then mk(9) → 18.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn sum(p: Point) -> i64 = p.x + p.y\n",
        "fn mk(v: i64) -> Point = Point{x: v, y: v}\n",
        "fn main() -> i32 {\n",
        "  a := sum(Point{x:3,y:4})\n",
        "  b := sum(mk(9))\n",
        "  return (a + b) as i32\n}\n",
    );
    assert_eq!(
        build_and_run("struct-param-ret", src).status.code(),
        Some(25)
    );
}

#[test]
fn struct_to_struct_assignment() {
    if !backend_available() {
        return;
    }
    // `p = q` copies the whole struct value: 10 + 20 = 30.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut p := Point{x:1,y:2}\n",
        "  q := Point{x:10,y:20}\n",
        "  p = q\n",
        "  return (p.x + p.y) as i32\n}\n",
    );
    assert_eq!(build_and_run("struct-assign", src).status.code(), Some(30));
}

#[test]
fn copy_struct_construction_stops_after_early_return() {
    if !backend_available() {
        return;
    }
    // Once a field initializer returns, later fields must not be lowered into the terminated block.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn side() -> i64 { print(9); return 9 }\n",
        "fn build() -> i32 {\n",
        "  p := Point { x: { return 0; 0 }, y: side() }\n",
        "  return 1\n",
        "}\n",
        "fn main() -> i32 = build()\n",
    );
    let out = build_and_run("copy-struct-early-return", src);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stdout.is_empty(),
        "later field side effect must not run"
    );
}

#[test]
fn nested_struct_by_value_param() {
    if !backend_available() {
        return;
    }
    // A nested struct passed by value; the callee reads its nested fields: 9 - 2 = 7.
    let src = concat!(
        "Point { x: i64, y: i64 }\nLine { a: Point, b: Point }\n",
        "fn span(l: Line) -> i64 = l.b.x - l.a.x\n",
        "fn main() -> i32 {\n",
        "  return span(Line{a: Point{x:2,y:0}, b: Point{x:9,y:0}}) as i32\n}\n",
    );
    assert_eq!(build_and_run("nested-byval", src).status.code(), Some(7));
}

#[test]
fn mixed_width_and_float_struct_by_value() {
    if !backend_available() {
        return;
    }
    // Mixed-width fields (i8/i32/i64/i16) and a float struct exercise the by-value ABI classification.
    let src = concat!(
        "R { a: i8, b: i32, c: i64, d: i16 }\nV { x: f64, y: f64 }\n",
        "fn tot(r: R) -> i64 = (r.a as i64) + (r.b as i64) + r.c + (r.d as i64)\n",
        "fn add(a: V, b: V) -> V = V{x: a.x + b.x, y: a.y + b.y}\n",
        "fn main() -> i32 {\n",
        "  t := tot(R{a:1,b:2,c:3,d:4})\n",
        "  s := add(V{x:1.5,y:2.5}, V{x:3.0,y:4.0})\n",
        "  if s.x > 4.0 { if s.y > 6.0 { return t as i32 } }\n",
        "  return 0\n}\n",
    );
    assert_eq!(build_and_run("mixed-byval", src).status.code(), Some(10));
}

#[test]
fn struct_returned_then_mutated() {
    if !backend_available() {
        return;
    }
    // Return a struct by value, bind it to a mutable local, mutate a field, and read it: 10 + 5 = 15.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn mk(v: i64) -> Point = Point{x: v, y: v}\n",
        "fn main() -> i32 {\n",
        "  mut p := mk(5)\n",
        "  p.x = 10\n",
        "  return (p.x + p.y) as i32\n}\n",
    );
    assert_eq!(
        build_and_run("returned-mutated", src).status.code(),
        Some(15)
    );
}

const RESULT_TABLES: &str = r#"module tables
pub Big { value: i64, f0: str, f1: str, f2: str, f3: str, f4: str, f5: str, f6: str, f7: str, f8: str, f9: str, f10: str, f11: str, f12: str }
pub fn make(n: i64) -> Big = Big { value: n, f0: "field0", f1: "field1", f2: "field2", f3: "field3", f4: "field4", f5: "field5", f6: "field6", f7: "field7", f8: "field8", f9: "field9", f10: "field10", f11: "field11", f12: "field12" }
pub fn consume(borrow item: Big) -> i64 = item.value + item.f0.len() + item.f12.len()
pub fn consume_value(item: Big) -> i64 = item.value + item.f0.len() + item.f12.len()
"#;

#[test]
fn imported_large_value_parameters_share_target_byval_transport() {
    if !backend_available() {
        return;
    }
    let caller = r#"
import tables
fn main() -> i32 = tables.consume_value(tables.make(20)) as i32
"#;
    let files = [("tables.align", RESULT_TABLES), ("main.align", caller)];
    assert_eq!(
        build_and_run_multi("large-byval-whole", &files, "main.align")
            .status
            .code(),
        Some(33)
    );
    let built = build_per_unit_multi("large-byval-units", &files, "main.align");
    assert_eq!(built.link_and_run().status.code(), Some(33));
    let provider = align_driver::emit_llvm_ir(
        &built.unit("tables").mir,
        BuildTarget::Baseline,
        align_driver::Profile::Dev,
        false,
        &[],
        false,
    )
    .expect("provider LLVM");
    let consumer = align_driver::emit_llvm_ir(
        &built.unit("main").mir,
        BuildTarget::Baseline,
        align_driver::Profile::Dev,
        false,
        &[],
        false,
    )
    .expect("consumer LLVM");
    assert!(
        provider
            .lines()
            .any(|line| line.contains("define") && line.contains("byval(")),
        "provider definition must use target-selected byval: {provider}"
    );
    assert!(
        consumer
            .lines()
            .any(|line| line.contains("call") && line.contains("byval(")),
        "consumer call must use the same target-selected byval: {consumer}"
    );
}

#[test]
fn imported_large_results_materialize_once_and_preserve_indirect_calls() {
    if !backend_available() {
        return;
    }
    let caller = r#"
import tables
fn identity<T>(value: T) -> T = value
fn main() -> i32 {
    make := tables.make
    first := make(10)
    offset := 1
    captured := fn n: i64 { tables.make(n + offset) }
    second := identity(captured(10))
    a := tables.consume(first)
    b := tables.consume(second)
    return (a + b) as i32
}
"#;
    let files = [("tables.align", RESULT_TABLES), ("main.align", caller)];
    assert_eq!(
        build_and_run_multi("large-result-whole", &files, "main.align")
            .status
            .code(),
        Some(47)
    );
    let built = build_per_unit_multi("large-result-units", &files, "main.align");
    assert_eq!(built.link_and_run().status.code(), Some(47));
}

#[test]
fn imported_large_result_copy_elimination_keeps_two_live_records() {
    if !backend_available() {
        return;
    }
    let caller = r#"
import tables
pub fn probe(n: i64) -> i64 {
    first := tables.make(n)
    second := tables.make(n + 1)
    a := tables.consume(first)
    b := tables.consume(second)
    return a + b
}
fn main() -> i32 = probe(10) as i32
"#;
    let built = build_per_unit_multi(
        "large-result-placement",
        &[("tables.align", RESULT_TABLES), ("main.align", caller)],
        "main.align",
    );
    assert_eq!(built.link_and_run().status.code(), Some(47));
    let ir = align_driver::emit_llvm_ir(
        &built.unit("main").mir,
        BuildTarget::Baseline,
        align_driver::Profile::Release,
        true,
        &["probe".to_owned()],
        false,
    )
    .expect("optimized caller");
    let ir = ir
        .split("define internal fastcc i64 @\"align_fn$5$70726f6265\"(")
        .nth(1)
        .expect("specialized probe core")
        .split("\n}")
        .next()
        .expect("probe body");
    // External definitions are unavailable in this module: both calls must
    // survive, and the two records must be independent until both consumers.
    assert_eq!(
        ir.lines()
            .filter(|line| line.contains("call void") && line.contains("sret("))
            .count(),
        2,
        "{ir}"
    );
    assert_eq!(
        ir.lines().filter(|line| line.contains("alloca ")).count(),
        2,
        "{ir}"
    );
    assert!(
        !ir.contains("llvm.memcpy"),
        "caller transfers must disappear: {ir}"
    );
    assert!(
        !ir.contains("call.result.storage"),
        "only final records remain: {ir}"
    );
}
