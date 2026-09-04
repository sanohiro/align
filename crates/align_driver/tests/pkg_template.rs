//! `pkg.template` owner: canonical package admission, escaping, raw append, ownership transfer,
//! and exact runtime lowering in whole-program and per-unit modes.

mod common;
use common::*;
use align_mir::{Operand, Rvalue, Stmt};
use align_sema::Ty;

fn fixed_template_place_mut(
    program: &mut align_mir::Program,
) -> Option<&mut align_mir::BorrowedFixedElementPlace> {
    program.fns.iter_mut().find_map(|function| {
        function.blocks.iter_mut().find_map(|block| {
            block.stmts.iter_mut().find_map(|statement| {
                let Stmt::Let(_, Rvalue::Call(_, arguments)) = statement else {
                    return None;
                };
                arguments.iter_mut().find_map(|argument| match argument {
                    Operand::BorrowedFixedElementPlace(place) => Some(place.as_mut()),
                    _ => None,
                })
            })
        })
    })
}

fn null_element_field_mut(
    program: &mut align_mir::Program,
) -> Option<(&mut u32, &mut Operand, &mut Vec<u32>)> {
    program.fns.iter_mut().find_map(|function| {
        function.blocks.iter_mut().find_map(|block| {
            block.stmts.iter_mut().find_map(|statement| match statement {
                Stmt::NullElemField(slot, index, path) => Some((slot, index, path)),
                _ => None,
            })
        })
    })
}

fn template_source() -> &'static str {
    fixture("apps/template/pkg/template.align")
}

fn descriptor_source() -> &'static str {
    fixture("apps/template/pkg/template/internal/descriptor.align")
}

fn resource_source() -> &'static str {
    fixture("apps/template/pkg/template/internal/resource.align")
}

fn files(main: &str) -> [(&str, &str); 4] {
    [
        ("pkg/template.align", template_source()),
        (
            "pkg/template/internal/descriptor.align",
            descriptor_source(),
        ),
        ("pkg/template/internal/resource.align", resource_source()),
        ("main.align", main),
    ]
}

const RENDER: &str = r#"module main
import pkg.template

fn render(value: str) -> string {
  mut output := pkg.template.html()
  pkg.template.raw(output, "<p>")
  pkg.template.write(output, value)
  pkg.template.raw(output, "</p>")
  return pkg.template.to_string(output)
}

fn main() -> i32 {
  print(render("<&>\"'"))
  return 0
}
"#;

#[test]
fn escaped_and_raw_appends_run_whole_and_per_unit() {
    let files = files(RENDER);
    let checked = diff_check_multi("pkg-template-check", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    for output in [
        build_and_run_multi("pkg-template-whole", &files, "main.align"),
        build_per_unit_multi("pkg-template-units", &files, "main.align").link_and_run(),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "<p>&lt;&amp;&gt;&quot;&#39;</p>\n"
        );
    }
}

#[test]
fn checked_operations_lower_to_the_five_exact_v1_exports() {
    let files = files(RENDER);
    let mir = whole_mir_multi("pkg-template-mir", &files, "main.align");
    for operation in [
        "template_html_new",
        "template_html_write",
        "template_html_raw",
        "template_html_to_string",
    ] {
        assert!(mir.contains(operation), "missing {operation}: {mir}");
    }
    if !backend_available() {
        return;
    }
    let ir = emit_llvm_multi("pkg-template-llvm", &files, "main.align");
    for symbol in [
        "@align_rt_template_html_new_v1()",
        "@align_rt_template_html_write_v1(ptr",
        "@align_rt_template_html_raw_v1(ptr",
        "@align_rt_template_html_into_string_v1(ptr",
        "@align_rt_template_html_free_v1(ptr",
    ] {
        assert!(ir.contains(symbol), "missing {symbol}: {ir}");
    }
}

#[test]
fn resource_fields_in_fixed_move_record_arrays_borrow_finish_and_drop_once() {
    let main = r#"module main
import pkg.template

Nested { first: pkg.template.html_builder, second: pkg.template.html_builder }
Holder { nested: Nested, code: i32 }

fn main() -> i32 {
  mut holders := [
    Holder { nested: Nested { first: pkg.template.html(), second: pkg.template.html() }, code: 10 },
    Holder { nested: Nested { first: pkg.template.html(), second: pkg.template.html() }, code: 20 },
  ]
  pkg.template.write(holders[0].nested.first, "<one>")
  first := pkg.template.to_string(holders[0].nested.first)
  pkg.template.raw(holders[0].nested.second, "ok")
  second := pkg.template.to_string(holders[0].nested.second)
  holders[0] = Holder { nested: Nested { first: pkg.template.html(), second: pkg.template.html() }, code: 30 }
  pkg.template.raw(holders[0].nested.first, "fresh")
  third := pkg.template.to_string(holders[0].nested.first)
  if first != "&lt;one&gt;" { return 1 }
  if second != "ok" { return 2 }
  if third != "fresh" { return 3 }
  return holders[0].code + holders[1].code
}
"#;
    let project_files = files(main);
    let checked = diff_check_multi("pkg-template-fixed-record-array", &project_files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if backend_available() {
        for output in [
            build_and_run_multi(
                "pkg-template-fixed-record-array-whole",
                &project_files,
                "main.align",
            ),
            build_per_unit_multi(
                "pkg-template-fixed-record-array-units",
                &project_files,
                "main.align",
            )
            .link_and_run(),
        ] {
            assert_eq!(
                output.status.code(),
                Some(50),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let built = build_per_unit_multi(
            "pkg-template-fixed-record-array-mir",
            &project_files,
            "main.align",
        );
        let valid = built.unit("main").mir.clone();
        assert!(
            emit_llvm_ir(&valid, BuildTarget::Baseline, false, &[], false).is_ok(),
            "the producer-created fixed element place must reach LLVM"
        );
        for (name, mutate) in [
            (
                "index",
                (|place: &mut align_mir::BorrowedFixedElementPlace| place.index = u32::MAX)
                    as fn(&mut align_mir::BorrowedFixedElementPlace),
            ),
            ("path", |place| place.path.clear()),
            ("type", |place| place.ty = Ty::Raw),
            ("cleanup", |place| place.cleanup = None),
            ("base", |place| place.base = u32::MAX),
        ] {
            let mut malformed = valid.clone();
            let Some(place) = fixed_template_place_mut(&mut malformed) else {
                assert!(false, "fixture must contain a borrowed fixed element place");
                return;
            };
            mutate(place);
            assert!(
                emit_llvm_ir(&malformed, BuildTarget::Baseline, false, &[], false).is_err(),
                "malformed fixed element {name} must fail before LLVM pointer construction"
            );
        }

        for (name, mutate) in [
            (
                "slot",
                (|slot: &mut u32, _: &mut Operand, _: &mut Vec<u32>| *slot = u32::MAX)
                    as fn(&mut u32, &mut Operand, &mut Vec<u32>),
            ),
            ("index-type", |_, index, _| {
                *index = Operand::Const(align_mir::Const::Bool(false));
            }),
            ("index-range", |_, index, _| {
                let Operand::Const(align_mir::Const::Int(_, ty)) = index else {
                    return;
                };
                *index = Operand::Const(align_mir::Const::Int(i128::MAX, *ty));
            }),
            ("empty-path", |_, _, path| path.clear()),
            ("missing-field", |_, _, path| *path = vec![u32::MAX]),
            ("non-record-intermediate", |_, _, path| *path = vec![1, 0]),
            ("non-resource-leaf", |_, _, path| *path = vec![1]),
        ] {
            let mut malformed = valid.clone();
            let Some((slot, index, path)) = null_element_field_mut(&mut malformed) else {
                assert!(false, "fixture must contain a fixed element nulling statement");
                return;
            };
            mutate(slot, index, path);
            assert!(
                emit_llvm_ir(&malformed, BuildTarget::Baseline, false, &[], false).is_err(),
                "malformed fixed element nulling {name} must fail before LLVM construction"
            );
        }
    }

    let duplicate = r#"module main
import pkg.template

Nested { output: pkg.template.html_builder }
Holder { nested: Nested }

fn main() -> i32 {
  mut holders := [Holder { nested: Nested { output: pkg.template.html() } }]
  first := pkg.template.to_string(holders[0].nested.output)
  second := pkg.template.to_string(holders[0].nested.output)
  return 0
}
"#;
    let duplicate_files = files(duplicate);
    let rejected = diff_check_multi(
        "pkg-template-nested-fixed-record-double-move",
        &duplicate_files,
        "main.align",
    );
    assert!(
        rejected.whole_errors
            && rejected.per_unit_errors
            && rejected.whole_diags.contains("use of moved element field")
            && rejected.per_unit_diags.contains("use of moved element field"),
        "whole:\n{}\nper-unit:\n{}",
        rejected.whole_diags,
        rejected.per_unit_diags,
    );
}

#[test]
fn generic_interface_and_record_field_paths_preserve_the_template_owner() {
    let carrier = r#"module carrier
import pkg.template

pub Holder { output: pkg.template.html_builder }

pub fn keep<T>(value: T) -> T = value

pub fn render(output: pkg.template.html_builder) -> string {
  mut holder := Holder { output: keep(output) }
  pkg.template.raw(holder.output, "<q>")
  pkg.template.write(holder.output, "<&>")
  pkg.template.raw(holder.output, "</q>")
  return pkg.template.to_string(holder.output)
}
"#;
    let main = r#"module main
import carrier
import pkg.template

fn main() -> i32 {
  result := carrier.render(pkg.template.html())
  if result == "<q>&lt;&amp;&gt;</q>" { return 42 }
  return 1
}
"#;
    let project = [
        ("pkg/template.align", template_source()),
        (
            "pkg/template/internal/descriptor.align",
            descriptor_source(),
        ),
        ("pkg/template/internal/resource.align", resource_source()),
        ("carrier.align", carrier),
        ("main.align", main),
    ];
    let checked = diff_check_multi("pkg-template-generic-record", &project, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        checked.whole_diags,
        checked.per_unit_diags,
    );
    if backend_available() {
        assert_eq!(
            build_and_run_multi("pkg-template-generic-record-whole", &project, "main.align")
                .status
                .code(),
            Some(42),
        );
        assert_eq!(
            build_per_unit_multi("pkg-template-generic-record-units", &project, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42),
        );
    }
}

#[test]
fn canonical_package_topology_and_forbidden_resource_carriers_fail_closed() {
    let main = "module main\nimport pkg.template\nfn main() -> i32 = 0\n";
    let altered_root = template_source().replacen("pub fn raw", "fn raw", 1);
    let nonempty_descriptor = "module pkg.template.internal.descriptor\nfn extra() {}\n";
    let altered_resource = resource_source().replacen(
        "align_rt_template_html_free_v1(state)",
        "raw.free(state)",
        1,
    );
    for (name, root, descriptor, resource) in [
        (
            "root",
            altered_root.as_str(),
            descriptor_source(),
            resource_source(),
        ),
        (
            "descriptor",
            template_source(),
            nonempty_descriptor,
            resource_source(),
        ),
        (
            "resource",
            template_source(),
            descriptor_source(),
            altered_resource.as_str(),
        ),
    ] {
        let project = [
            ("pkg/template.align", root),
            ("pkg/template/internal/descriptor.align", descriptor),
            ("pkg/template/internal/resource.align", resource),
            ("main.align", main),
        ];
        let checked = diff_check_multi(
            &format!("pkg-template-canonical-{name}"),
            &project,
            "main.align",
        );
        assert!(
            checked.whole_errors
                && checked.per_unit_errors
                && checked
                    .whole_diags
                    .contains("must match the canonical package definition")
                && checked
                    .per_unit_diags
                    .contains("must match the canonical package definition"),
            "{name}: whole:\n{}\nper-unit:\n{}",
            checked.whole_diags,
            checked.per_unit_diags,
        );
    }

    for (name, source) in [
        (
            "direct-fixed-array",
            "module main\nimport pkg.template\nfn main() -> i32 { values := [pkg.template.html(), pkg.template.html()]; return 0 }\n",
        ),
        (
            "dynamic-array",
            "module main\nimport pkg.template\nfn main() -> i32 { mut values: array_builder<pkg.template.html_builder> := array_builder(); values.push(pkg.template.html()); built := values.build(); return 0 }\n",
        ),
        (
            "capture",
            "module main\nimport pkg.template\nfn main() -> i32 { output := pkg.template.html(); closure := fn() { pkg.template.to_string(output) }; return 0 }\n",
        ),
        (
            "temporary-borrow",
            "module main\nimport pkg.template\nfn main() -> i32 { pkg.template.write(pkg.template.html(), \"x\"); return 0 }\n",
        ),
        (
            "double-finish",
            "module main\nimport pkg.template\nfn main() -> i32 { output := pkg.template.html(); first := pkg.template.to_string(output); second := pkg.template.to_string(output); return 0 }\n",
        ),
        (
            "internal-import",
            "module main\nimport pkg.template.internal.descriptor\nfn main() -> i32 = 0\n",
        ),
    ] {
        let project = files(source);
        let checked = diff_check_multi(
            &format!("pkg-template-forbidden-{name}"),
            &project,
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{name}: whole:\n{}\nper-unit:\n{}",
            checked.whole_diags,
            checked.per_unit_diags,
        );
    }
}
