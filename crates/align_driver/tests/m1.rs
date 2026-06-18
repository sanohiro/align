//! M1 end-to-end: the builtin `print` reaches the runtime and writes to stdout
//! (`docs/impl/07-roadmap.md` M1). Requires LLVM/cc, so skip where they are absent.

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);

    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    std::process::Command::new(&exe).output().expect("run")
}

#[test]
fn print_outputs_integer_and_newline() {
    if !backend_available() {
        eprintln!("skip: LLVM backend not wired");
        return;
    }
    let out = build_and_run("print", "fn main() -> i32 {\n  print(42)\n  return 0\n}\n");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

#[test]
fn print_runs_multiple_times_in_order() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  print(1)\n  print(2)\n  print(3)\n  return 0\n}\n";
    let out = build_and_run("print-seq", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n3\n");
}

#[test]
fn print_widens_a_narrow_integer() {
    if !backend_available() {
        return;
    }
    // id returns i32; print widens it to i64 for the runtime call.
    let src = "fn id(n: i32) -> i32 {\n  return n\n}\nfn main() -> i32 {\n  print(id(7))\n  return 0\n}\n";
    let out = build_and_run("print-i32", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}

#[test]
fn float_arithmetic_and_comparison() {
    if !backend_available() {
        return;
    }
    // area(2.0) = 12.56636, which is in (12, 13): exit 1.
    let src = "fn area(r: f64) -> f64 = r * r * 3.14159\nfn main() -> i32 {\n  a := area(2.0)\n  if a > 12.0 { if a < 13.0 { return 1 } }\n  return 0\n}\n";
    let out = build_and_run("circle", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn char_literals_and_comparison() {
    if !backend_available() {
        return;
    }
    // '5' is a digit → 2.
    let src = "fn classify(c: char) -> i32 {\n  if c == 'a' { return 1 }\n  if c >= '0' { if c <= '9' { return 2 } }\n  return 0\n}\nfn main() -> i32 {\n  return classify('5')\n}\n";
    let out = build_and_run("classify", src);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn float_unary_neg_with_f32_typed_let() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  x: f32 := 7.5\n  y := -x\n  if y < 0.0 { return 1 }\n  return 0\n}\n";
    let out = build_and_run("f32neg", src);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn struct_construct_read_and_field_assign() {
    if !backend_available() {
        return;
    }
    // Construct, mutate a field, read fields, and combine them.
    let src = "Point {\n  x: i32,\n  y: i32,\n}\nfn main() -> i32 {\n  mut p := Point { x: 3, y: 4 }\n  p.y = 10\n  print(p.x)\n  print(p.y)\n  return p.x + p.y\n}\n";
    let out = build_and_run("point", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n10\n");
    assert_eq!(out.status.code(), Some(13), "3 + 10 = 13");
}
