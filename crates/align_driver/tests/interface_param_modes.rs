//! L2a gate: parameter modes and explicit empty return-provenance summaries survive whole-program
//! and per-unit checking, HIR-to-MIR lowering, interface rendering, and imported declarations.

mod common;
use common::*;

use align_ast::ParamMode;
use align_interface::{IType, ReturnBorrowSummary, ReturnRegionSummary};
use align_mir::{Rvalue, Stmt};

const LIB: &str = "\
module buffer
pub Callbacks {
  write: fn(out slice<i64>, i64) -> ()
}
pub fn put(out dst: slice<i64>, value: i64) {
  dst[0] = value
}
pub fn named_borrow(borrow: i64) -> i64 = borrow
";

const MAIN: &str = "\
import buffer
fn main() -> i32 {
  mut values := [0, 0]
  buffer.put(values, buffer.named_borrow(42))
  return values[0] as i32
}
";

fn files() -> &'static [(&'static str, &'static str)] {
    &[("buffer.align", LIB), ("main.align", MAIN)]
}

#[test]
fn whole_and_per_unit_interfaces_preserve_modes_and_explicit_none_summaries() {
    let checked = assert_same_verdict("l2a-interface-modes", files(), "main.align");
    assert!(!checked.diags.has_errors(), "L2a source must check");

    let buffer = checked
        .summaries
        .iter()
        .find(|summary| summary.unit == "buffer")
        .expect("buffer summary");
    let put = buffer
        .fns
        .iter()
        .find(|function| function.name == "put")
        .expect("put signature");
    assert_eq!(
        put.params
            .iter()
            .map(|param| param.mode)
            .collect::<Vec<_>>(),
        vec![ParamMode::Out, ParamMode::ByValue]
    );
    assert_eq!(put.return_borrow, ReturnBorrowSummary::None);
    assert_eq!(put.return_region, ReturnRegionSummary::None);

    let named_borrow = buffer
        .fns
        .iter()
        .find(|function| function.name == "named_borrow")
        .expect("named_borrow signature");
    assert_eq!(named_borrow.params[0].mode, ParamMode::ByValue);

    let callbacks = buffer
        .structs
        .iter()
        .find(|structure| structure.name == "Callbacks")
        .expect("Callbacks interface");
    let IType::Fn {
        params,
        return_borrow,
        return_region,
        ..
    } = &callbacks.fields[0].1
    else {
        panic!("Callbacks.write must remain a function type");
    };
    assert_eq!(
        params.iter().map(|param| param.mode).collect::<Vec<_>>(),
        vec![ParamMode::Out, ParamMode::ByValue]
    );
    assert_eq!(*return_borrow, ReturnBorrowSummary::None);
    assert_eq!(*return_region, ReturnRegionSummary::None);
}

#[test]
fn out_mode_type_disagreement_is_rejected_in_function_types() {
    let files = &[
        ("bad.align", "module bad\npub Callbacks { invalid: fn(out i64) -> () }\n"),
        ("main.align", "import bad\nfn main() -> i32 = 0\n"),
    ];
    let checked = assert_same_verdict("l2a-mode-type-mismatch", files, "main.align");
    assert!(
        checked.diags.has_errors(),
        "Out on a non-slice function-type parameter must fail"
    );
}

#[test]
fn per_unit_mir_preserves_defining_and_imported_signature_facts() {
    let built = build_per_unit_multi("l2a-mir-modes", files(), "main.align");
    assert!(
        !built.walk.diags.has_errors(),
        "per-unit lowering must check"
    );

    let buffer = built
        .walk
        .units
        .iter()
        .find(|unit| unit.unit == "buffer")
        .expect("buffer artifact");
    let put = buffer
        .mir
        .fns
        .iter()
        .find(|function| function.name == "buffer$put")
        .expect("put MIR");
    assert_eq!(put.param_modes, vec![ParamMode::Out, ParamMode::ByValue]);
    assert_eq!(put.return_borrow, ReturnBorrowSummary::None);
    assert_eq!(put.return_region, ReturnRegionSummary::None);

    let main = built
        .walk
        .units
        .iter()
        .find(|unit| unit.unit == "main")
        .expect("main artifact");
    let imported = main
        .mir
        .imported_fns
        .iter()
        .find(|function| function.name == "buffer$put")
        .expect("imported put declaration");
    assert_eq!(
        imported.param_modes,
        vec![ParamMode::Out, ParamMode::ByValue]
    );
    assert_eq!(imported.return_borrow, ReturnBorrowSummary::None);
    assert_eq!(imported.return_region, ReturnRegionSummary::None);
}

#[test]
fn whole_and_per_unit_execution_keep_existing_out_behavior() {
    if !backend_available() {
        return;
    }
    let whole = build_and_run_multi("l2a-modes-whole", files(), "main.align");
    let per_unit = build_per_unit_multi("l2a-modes-per-unit", files(), "main.align");
    let per_unit = per_unit.link_and_run();
    assert_eq!(whole.status.code(), Some(42));
    assert_eq!(per_unit.status.code(), Some(42));
}

#[test]
fn malformed_mir_signature_facts_fail_before_llvm_emission() {
    if !backend_available() {
        return;
    }
    let built = build_per_unit_multi("l2a-malformed-mir", files(), "main.align");
    assert!(!built.walk.diags.has_errors());
    let buffer = built
        .walk
        .units
        .iter()
        .find(|unit| unit.unit == "buffer")
        .expect("buffer artifact");

    let mut wrong_arity = buffer.mir.clone();
    let put = wrong_arity
        .fns
        .iter_mut()
        .find(|function| function.name == "buffer$put")
        .expect("put MIR");
    put.param_modes.pop();
    let error = emit_llvm_ir(&wrong_arity, BuildTarget::Baseline, false, &[], false)
        .expect_err("mode arity mismatch must fail");
    assert!(
        error.contains("parameter modes"),
        "unexpected diagnostic: {error}"
    );

    let mut disabled_mode = buffer.mir.clone();
    let put = disabled_mode
        .fns
        .iter_mut()
        .find(|function| function.name == "buffer$put")
        .expect("put MIR");
    put.param_modes[0] = ParamMode::Borrow;
    let error = emit_llvm_ir(&disabled_mode, BuildTarget::Baseline, false, &[], false)
        .expect_err("disabled mode must fail");
    assert!(
        error.contains("before its ABI is enabled"),
        "unexpected diagnostic: {error}"
    );

    let mut premature_roots = buffer.mir.clone();
    let put = premature_roots
        .fns
        .iter_mut()
        .find(|function| function.name == "buffer$put")
        .expect("put MIR");
    put.return_borrow = ReturnBorrowSummary::Roots {
        params: vec![0],
        captures: vec![],
    };
    let error = emit_llvm_ir(&premature_roots, BuildTarget::Baseline, false, &[], false)
        .expect_err("premature roots must fail");
    assert!(
        error.contains("before L2b"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn function_value_mir_carries_modes_and_explicit_none_summaries() {
    let src = "\
fn apply(f: fn(i64) -> i64, value: i64) -> i64 = f(value)
fn increment(value: i64) -> i64 = value + 1
fn main() -> i32 = apply(increment, 41) as i32
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "function-signatures.align", src);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let rendered = align_mir::print::program_to_string(&mir);
    assert!(
        rendered.contains("fn_addr increment signature=FnSignatureFacts"),
        "function-address facts must remain visible in human MIR:\n{rendered}"
    );
    assert!(
        rendered.contains("call_indirect") && rendered.contains("signature=FnSignatureFacts"),
        "indirect-call facts must remain visible in human MIR:\n{rendered}"
    );

    let mut saw_address = false;
    let mut saw_indirect = false;
    for function in &mir.fns {
        for block in &function.blocks {
            for statement in &block.stmts {
                match statement {
                    Stmt::Let(_, Rvalue::FnAddr { signature, .. }) => {
                        saw_address = true;
                        assert_eq!(signature.param_modes, vec![ParamMode::ByValue]);
                        assert_eq!(signature.return_borrow, ReturnBorrowSummary::None);
                        assert_eq!(signature.return_region, ReturnRegionSummary::None);
                    }
                    Stmt::Let(_, Rvalue::CallIndirect { signature, .. }) => {
                        saw_indirect = true;
                        assert_eq!(signature.param_modes, vec![ParamMode::ByValue]);
                        assert_eq!(signature.return_borrow, ReturnBorrowSummary::None);
                        assert_eq!(signature.return_region, ReturnRegionSummary::None);
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        saw_address,
        "named function value must carry signature facts"
    );
    assert!(saw_indirect, "indirect call must carry signature facts");

    let mut malformed = mir;
    'functions: for function in &mut malformed.fns {
        for block in &mut function.blocks {
            for statement in &mut block.stmts {
                if let Stmt::Let(_, Rvalue::FnAddr { signature, .. }) = statement {
                    signature.param_modes.clear();
                    break 'functions;
                }
            }
        }
    }
    let error = emit_llvm_ir(&malformed, BuildTarget::Baseline, false, &[], false)
        .expect_err("function address arity mismatch must fail");
    assert!(
        error.contains("parameter modes") || error.contains("disagree"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn capturing_closure_signature_facts_are_visible_in_human_mir() {
    let src = "\
fn main() -> i32 {
  offset: i32 := 1
  add := fn value: i32 { value + offset }
  return add(41)
}
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "closure-signatures.align", src);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let rendered = align_mir::print::program_to_string(&mir);
    assert!(
        rendered.contains("closure ") && rendered.contains("signature=FnSignatureFacts"),
        "closure facts must remain visible in human MIR:\n{rendered}"
    );
}
