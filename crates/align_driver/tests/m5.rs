//! M5 end-to-end: strings (literals + `print`). Requires LLVM/cc, so skip where absent.

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
fn print_string_literal_and_returned_str() {
    if !backend_available() {
        return;
    }
    let src = "fn greet() -> str = \"hello, align\"\nfn main() -> i32 {\n  print(\"strings work!\")\n  print(greet())\n  print(7)\n  return 0\n}\n";
    let out = build_and_run("strings", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "strings work!\nhello, align\n7\n"
    );
}

#[test]
fn string_escapes() {
    if !backend_available() {
        return;
    }
    // \t and \n inside a literal are decoded by the lexer.
    let src = "fn main() -> i32 {\n  print(\"a\\tb\")\n  return 0\n}\n";
    let out = build_and_run("str-escape", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\tb\n");
}
