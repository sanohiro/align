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

Holder { output: pkg.template.html_builder, code: i32 }

fn main() -> i32 {
  mut holders := [
    Holder { output: pkg.template.html(), code: 10 },
    Holder { output: pkg.template.html(), code: 20 },
  ]
  pkg.template.write(holders[0].output, "<one>")
  first := pkg.template.to_string(holders[0].output)
  pkg.template.raw(holders[1].output, "ok")
  second := pkg.template.to_string(holders[1].output)
  if first != "&lt;one&gt;" { return 1 }
  if second != "ok" { return 2 }
  return holders[0].code + holders[1].code
}
"#;
    let files = files(main);
    let checked = diff_check_multi("pkg-template-fixed-record-array", &files, "main.align");
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
                &files,
                "main.align",
            ),
            build_per_unit_multi(
                "pkg-template-fixed-record-array-units",
                &files,
                "main.align",
            )
            .link_and_run(),
        ] {
            assert_eq!(
                output.status.code(),
                Some(30),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let built = build_per_unit_multi(
            "pkg-template-fixed-record-array-mir",
            &files,
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
    }
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
