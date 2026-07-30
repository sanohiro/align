//! MIR eager-child continuation gates.
//!
//! A nested `return` supplies only an unreachable placeholder operand to recursive lowering.
//! Eager parents must stop before evaluating a later sibling or emitting their own action.

mod common;
use common::*;

use align_mir::{Rvalue, Stmt, Term};

const SOURCE: &str = "\
Holder<T> { callback: T }
Wrap<T> { callback: T }
Named { name: string }
fn later() -> i64 {
  print(99)
  return 99
}
fn quiet(x: i64) -> i64 = x + 1
fn invoke(holder: Holder<fn(i64) -> i64>, x: i64) -> i64 =
  holder.callback(x)
fn source_compatible() -> i64 {
  indirect := invoke
  return indirect(Holder { callback: quiet }, 41)
}
fn string_field() -> i64 {
  rows := [Named { name: \"align\".clone() }]
  return rows[0].name.len()
}
fn keep_error(
  wrap: Wrap<fn(i64) -> i64>,
) -> Wrap<fn(i64) -> i64> = wrap
fn fail_with<E>(error: E) -> Result<i64, E> = Err(error)
fn source_compatible_map_err() -> i64 {
  mapped := fail_with(Wrap { callback: quiet }).map_err(keep_error)
  return match mapped {
    Ok(value) => value
    Err(wrap) => wrap.callback(41)
  }
}
fn unary() -> i64 = -{ return 1; 0 }
fn binary() -> i64 = { return 2; 0 } + later()
fn call(a: i64, b: i64) -> i64 = a + b
fn arguments() -> i64 = call({ return 3; 0 }, later())
fn selected(flag: bool) -> i64 =
  (if flag { return 4; 0 } else { return 5; 0 }) + later()
fn fixed_index() -> i64 = [{ return 6; 0 }, later()][0]
fn dynamic_index() -> i64 = [1].to_array()[{ return 7; 0 }]
fn native() -> i64 = { return 8; 0 }.abs()
fn aggregate() -> i64 {
  pair := ({ return 9; 0 }, later())
  return pair.0
}
fn pipeline() -> i64 =
  [1, 2].map(fn x { x + 1 }).reduce({ return 10; 0 }, fn acc, x { acc + x })
fn binding() -> i64 {
  value := { return 11; 0 }
  print(later())
  return value
}
fn builder_arg() -> i64 {
  b := builder()
  b.write_int({ return 12; 0 })
  b.write_int(later())
  return 0
}
fn fail() -> Result<i64, Error> = Err(error(13))
fn templated() -> Result<i64, Error> {
  value := template \"x={fail()?} y={later()}\"
  return Ok(value.len())
}
fn template_result() -> i64 {
  value := templated() else { return 13 }
  return value
}
fn main() -> i32 {
  print(unary())
  print(binary())
  print(arguments())
  print(selected(true))
  print(fixed_index())
  print(dynamic_index())
  print(native())
  print(aggregate())
  print(pipeline())
  print(binding())
  print(builder_arg())
  print(template_result())
  print(source_compatible())
  print(string_field())
  print(source_compatible_map_err())
  return 0
}
";

#[test]
fn eager_children_stop_before_parent_actions() {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "mir-continuation.align", SOURCE);
    assert!(
        !checked.diags.has_errors(),
        "fixture must check:\n{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    let program = lower_to_mir(&checked.hir);
    for name in [
        "unary",
        "binary",
        "arguments",
        "selected",
        "fixed_index",
        "dynamic_index",
        "native",
        "aggregate",
        "pipeline",
        "binding",
        "builder_arg",
    ] {
        let function = program
            .fns
            .iter()
            .find(|function| function.name == name)
            .unwrap_or_else(|| panic!("{name} function"));
        let emitted_forbidden_action = |statement: &Stmt| match name {
            "unary" => matches!(statement, Stmt::Let(_, Rvalue::Un(..))),
            "binary" | "selected" => matches!(statement, Stmt::Let(_, Rvalue::Bin(..))),
            "arguments" => {
                matches!(statement, Stmt::Let(_, Rvalue::Call(callee, _)) if callee == "call")
            }
            "fixed_index" => matches!(statement, Stmt::Let(_, Rvalue::Index(..))),
            "dynamic_index" => matches!(statement, Stmt::Let(_, Rvalue::SliceIndex(..))),
            "native" => matches!(statement, Stmt::Let(_, Rvalue::MathOp { .. })),
            "aggregate" => matches!(statement, Stmt::Let(_, Rvalue::MakeTuple { .. })),
            "pipeline" => matches!(statement, Stmt::Let(_, Rvalue::Call(..))),
            "binding" => matches!(statement, Stmt::Store(..)),
            "builder_arg" => matches!(
                statement,
                Stmt::Let(
                    _,
                    Rvalue::BuilderWriteStr(..)
                        | Rvalue::BuilderWriteInt(..)
                        | Rvalue::BuilderWriteBool(..)
                        | Rvalue::BuilderWriteChar(..)
                        | Rvalue::BuilderWriteFloat(..)
                        | Rvalue::BuilderWriteStrIntStr(..)
                )
            ),
            _ => unreachable!("matrix function"),
        };
        assert!(
            function
                .blocks
                .iter()
                .flat_map(|block| &block.stmts)
                .all(|statement| {
                    !matches!(
                        statement,
                        Stmt::Let(_, Rvalue::Call(callee, _)) if callee == "later"
                    ) && !emitted_forbidden_action(statement)
                        && !matches!(
                            statement,
                            Stmt::Store(
                                _,
                                align_mir::Operand::Const(align_mir::Const::Unit)
                            )
                        )
                }),
            "{name} emitted a later sibling, parent action, or placeholder store: {function:#?}"
        );
        assert!(
            function
                .blocks
                .iter()
                .any(|block| matches!(block.term, Term::Return(Some(_)))),
            "{name} must preserve its nested return edge: {function:#?}"
        );
    }

    let source_compatible = program
        .fns
        .iter()
        .find(|function| function.name == "source_compatible")
        .expect("source-compatible indirect call");
    assert!(
        source_compatible
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .any(|statement| matches!(statement, Stmt::Let(_, Rvalue::CallIndirect { .. }))),
        "source-compatible origin-specific nominal ids must retain the indirect call: \
         {source_compatible:#?}"
    );

    let string_field = program
        .fns
        .iter()
        .find(|function| function.name == "string_field")
        .expect("owned-string element field");
    assert!(
        string_field
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .any(|statement| {
                matches!(
                    statement,
                    Stmt::Let(
                        _,
                        Rvalue::IndexField(..)
                            | Rvalue::IndexFieldPtr { .. }
                            | Rvalue::IndexColumn { .. }
                    )
                )
            }),
        "a checked string field exposed as str must retain its field action: {string_field:#?}"
    );

    let source_compatible_map_err = program
        .fns
        .iter()
        .find(|function| function.name == "source_compatible_map_err")
        .expect("source-compatible map_err");
    assert!(
        source_compatible_map_err
            .blocks
            .iter()
            .flat_map(|block| &block.stmts)
            .any(|statement| matches!(statement, Stmt::Let(_, Rvalue::CallIndirect { .. }))),
        "source-compatible origin-specific nominal ids must retain the map_err call: \
         {source_compatible_map_err:#?}"
    );
}

#[test]
fn eager_termination_codegen_preserves_results() {
    if !backend_available() {
        return;
    }
    let output = build_and_run("mir-continuation", SOURCE);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "1\n2\n3\n4\n6\n7\n8\n9\n10\n11\n12\n13\n42\n5\n42\n"
    );
}
