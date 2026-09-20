//! Plan 74 — interface-carried concrete inline bodies.

mod common;
use common::*;

use align_interface::IFnBody;

fn encoded(name: &str) -> String {
    let hex = name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("align_fn${}${hex}", name.len())
}

const LIB: &str = "\
module tiny
pub fn add7(x: i64) -> i64 = x + 7
pub fn with_local(x: i64) -> i64 {
  y := x + 8
  return y
}
pub fn identity<T>(x: T) -> T = x
";

const MAIN: &str = "\
import tiny
fn main() {
  print(tiny.add7(35))
  call := tiny.add7
  print(call(35))
  print(tiny.with_local(34))
  print(tiny.identity(42))
}
";

const FLOAT_LIB: &str = "\
module floatlib
pub fn fused(a: f64, b: f64, c: f64) -> f64 = float(contract) { a * b + c }
pub fn generic<T: Num>(a: T, b: T) -> T = float(reassoc) { a + b }
pub fn move_value(values: array<i64>) -> array<i64> = float(reassoc) { values }
pub fn clone_text(borrow value: str) -> string = (float(reassoc) { value }).clone()
";

const FLOAT_MAIN: &str = "\
import floatlib
fn main() {
  print(floatlib.fused(2.0, 3.0, 4.0))
  print(floatlib.generic(2.0, 3.0))
  print(floatlib.generic(2, 3))
  values := [1, 2, 3].to_array()
  moved := floatlib.move_value(values)
  print(moved.sum())
  text := \"borrowed\"
  print(floatlib.clone_text(text))
}
";

#[test]
fn admitted_body_is_a_consumer_definition_and_other_body_kinds_stay_distinct() {
    let built = build_per_unit_multi(
        "interface-inline-kinds",
        &[("tiny.align", LIB), ("main.align", MAIN)],
        "main.align",
    );
    let producer = built.unit("tiny");
    let add7 = producer
        .summary
        .fns
        .iter()
        .find(|function| function.name == "add7")
        .expect("add7 interface row");
    assert!(
        matches!(add7.body, IFnBody::ConcreteInline { .. }),
        "summary={add7:#?}\nmir={:#?}",
        producer
            .mir
            .fns
            .iter()
            .find(|function| function.name.as_str() == "tiny$add7")
    );
    let with_local = producer
        .summary
        .fns
        .iter()
        .find(|function| function.name == "with_local")
        .expect("with_local interface row");
    assert_eq!(with_local.body, IFnBody::Absent);
    let identity = producer
        .summary
        .fns
        .iter()
        .find(|function| function.name == "identity")
        .expect("identity interface row");
    assert!(matches!(identity.body, IFnBody::GenericTemplate(_)));

    let consumer = built.unit("main");
    let transported = consumer
        .mir
        .fns
        .iter()
        .find(|function| function.name.as_str() == "tiny$add7")
        .expect("consumer MIR definition");
    assert!(transported.available_externally);
    assert!(!transported.exportable);
    assert!(
        consumer
            .mir
            .imported_fns
            .iter()
            .any(|function| function.name.as_str() == "tiny$with_local")
    );
}

#[test]
fn float_scope_source_survives_interface_rechecking_and_per_unit_codegen() {
    let files = [
        ("floatlib.align", FLOAT_LIB),
        ("main.align", FLOAT_MAIN),
    ];
    let built = build_per_unit_multi("interface-float-scope", &files, "main.align");
    let producer = built.unit("floatlib");
    let fused = producer
        .summary
        .fns
        .iter()
        .find(|function| function.name == "fused")
        .expect("fused interface row");
    let IFnBody::ConcreteInline { source, .. } = &fused.body else {
        panic!("fused body must be transported for consumer codegen: {fused:#?}");
    };
    assert!(source.contains("float(contract)"), "concrete source: {source}");

    let generic = producer
        .summary
        .fns
        .iter()
        .find(|function| function.name == "generic")
        .expect("generic interface row");
    let IFnBody::GenericTemplate(source) = &generic.body else {
        panic!("generic body must be transported for monomorphization: {generic:#?}");
    };
    assert!(
        source.contains("float(reassoc)"),
        "generic source: {source}",
    );

    if !backend_available() {
        return;
    }
    let consumer = built.unit("main");
    let llvm = emit_llvm_ir(
        &consumer.mir,
        BuildTarget::Baseline,
        align_driver::Profile::Release,
        false,
        &[],
        false,
    )
    .expect("consumer LLVM IR");
    assert!(llvm.contains("@llvm.fma.f64"), "consumer IR:\n{llvm}");
    assert!(
        llvm.lines()
            .any(|line| line.contains("fadd reassoc double")),
        "the floating generic instance lost reassoc during interface rechecking:\n{llvm}",
    );

    let whole = build_and_run_multi("interface-float-whole", &files, "main.align");
    let per_unit = built.link_and_run();
    assert_eq!(whole.status.code(), Some(0));
    assert_eq!(per_unit.status.code(), Some(0));
    assert_eq!(whole.stdout, per_unit.stdout);
}

#[test]
fn consumer_llvm_uses_available_externally_and_runtime_matches_whole_program() {
    let files = [("tiny.align", LIB), ("main.align", MAIN)];
    let whole = build_and_run_multi("interface-inline-whole", &files, "main.align");
    let built = build_per_unit_multi("interface-inline-unit", &files, "main.align");
    let per_unit = built.link_and_run();
    assert_eq!(whole.status.code(), Some(0));
    assert_eq!(per_unit.status.code(), Some(0));
    assert_eq!(whole.stdout, per_unit.stdout);
    assert_eq!(String::from_utf8_lossy(&per_unit.stdout), "42\n42\n42\n42\n");

    let consumer = built.unit("main");
    let llvm = emit_llvm_ir(
        &consumer.mir,
        BuildTarget::Baseline,
        align_driver::Profile::Dev,
        false,
        &[],
        false,
    )
    .expect("consumer LLVM IR");
    assert!(
        llvm.contains("define available_externally i64 @\"align_fn$9$74696e792461646437\""),
        "consumer IR:\n{llvm}"
    );
    let optimized = emit_llvm_ir(
        &consumer.mir,
        BuildTarget::Baseline,
        align_driver::Profile::Release,
        true,
        &[],
        false,
    )
    .expect("optimized consumer LLVM IR");
    assert!(
        !optimized.contains("call i64 @\"align_fn$9$74696e792461646437\""),
        "the tiny direct call should inline without ThinLTO:\n{optimized}"
    );

    let objects = built.emit_objects_with(align_driver::Profile::Dev, false);
    let main_index = built
        .walk
        .units
        .iter()
        .position(|unit| unit.unit == "main")
        .expect("main object index");
    if let Some(symbols) = nm_symbols(&objects[main_index]) {
        let symbol = encoded("tiny$add7");
        let matching = symbols
            .iter()
            .filter(|(_, name)| name.strip_prefix('_').unwrap_or(name) == symbol)
            .collect::<Vec<_>>();
        assert!(
            matching.iter().any(|(kind, _)| *kind == 'U')
                && matching.iter().all(|(kind, _)| !matches!(kind, 'T' | 't')),
            "consumer object must reference but not define {symbol}: {matching:?}"
        );
    }
}

#[test]
fn raw_and_c_extern_closures_are_rechecked_and_lowered_in_the_consumer() {
    let library = "\
module native
extern \"C\" fn abs(x: i32) -> i32
extern \"C\" fn labs(x: i64) -> i64
extern \"C\" link(\"m\") { fn acos(x: f64) -> f64 }
pub fn cabs(x: i32) -> i64 = unsafe { abs(x) as i64 }
pub fn mixed_extern_order(x: i64) -> i64 = unsafe { labs(x) + acos(1.0) as i64 }
pub fn null_is_null() -> bool = unsafe { raw.null().is_null() }
";
    let main = "\
import native
fn main() {
  print(native.cabs(-42))
  print(native.mixed_extern_order(-42))
  print(native.null_is_null())
}
";
    let files = [("native.align", library), ("main.align", main)];
    let built = build_per_unit_multi("interface-inline-native", &files, "main.align");
    let producer = built.unit("native");
    let cabs = producer
        .summary
        .fns
        .iter()
        .find(|function| function.name == "cabs")
        .expect("cabs interface row");
    let IFnBody::ConcreteInline { externs, .. } = &cabs.body else {
        panic!("cabs must carry a concrete body: {cabs:#?}");
    };
    assert_eq!(externs.len(), 1);
    assert_eq!(externs[0].name, "abs");
    assert!(externs[0].link.is_none());

    let consumer = built.unit("main");
    for name in [
        "native$cabs",
        "native$mixed_extern_order",
        "native$null_is_null",
    ] {
        assert!(
            consumer
                .mir
                .fns
                .iter()
                .any(|function| function.name.as_str() == name && function.available_externally),
            "missing transported definition for {name}"
        );
    }
    if backend_available() {
        let whole = build_and_run_multi("interface-inline-native-whole", &files, "main.align");
        let per_unit = built.link_and_run();
        assert_eq!(whole.stdout, per_unit.stdout);
        assert_eq!(String::from_utf8_lossy(&per_unit.stdout), "42\n42\ntrue\n");
    }
}

#[test]
fn transported_body_may_call_an_imported_public_dependency_but_not_a_same_unit_helper() {
    let base = "module base\npub fn inc(x: i64) -> i64 = x + 1\n";
    let bridge = "\
module bridge
import base
fn local(x: i64) -> i64 = x + 10
pub fn imported(x: i64) -> i64 = base.inc(x)
pub fn same_unit(x: i64) -> i64 = local(x)
";
    let main = "\
import bridge
fn main() {
  print(bridge.imported(41))
  print(bridge.same_unit(32))
}
";
    let files = [
        ("base.align", base),
        ("bridge.align", bridge),
        ("main.align", main),
    ];
    let built = build_per_unit_multi("interface-inline-transitive", &files, "main.align");
    let summary = &built.unit("bridge").summary;
    assert!(matches!(
        summary
            .fns
            .iter()
            .find(|function| function.name == "imported")
            .expect("imported row")
            .body,
        IFnBody::ConcreteInline { .. }
    ));
    assert_eq!(
        summary
            .fns
            .iter()
            .find(|function| function.name == "same_unit")
            .expect("same_unit row")
            .body,
        IFnBody::Absent
    );
    if backend_available() {
        let whole = build_and_run_multi("interface-inline-transitive-whole", &files, "main.align");
        let per_unit = built.link_and_run();
        assert_eq!(whole.stdout, per_unit.stdout);
        assert_eq!(String::from_utf8_lossy(&per_unit.stdout), "42\n42\n");
    }
}
