//! M3 end-to-end: arena + heap box (allocation, `.get()`, bulk free, cleanup on
//! early exit). Requires LLVM/cc, so skip where they are absent.

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
fn arena_box_alloc_and_get() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  r: i32 := arena {\n    p: box<i32> := heap.new(7)\n    q: box<i32> := heap.new(35)\n    p.get() + q.get()\n  }\n  return r\n}\n";
    let out = build_and_run("arena-box", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn arena_freed_on_early_return() {
    if !backend_available() {
        return;
    }
    // `return` inside the arena must still free it (cleanup on every exit).
    let src = "fn pick(b: bool) -> i32 {\n  arena {\n    p: box<i32> := heap.new(9)\n    if b { return p.get() }\n    p.get()\n  }\n}\nfn main() -> i32 {\n  return pick(true)\n}\n";
    let out = build_and_run("arena-early", src);
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn clone_keeps_original_usable() {
    if !backend_available() {
        return;
    }
    // p.clone() deep-copies; both p and q stay valid. 7 + 7 = 14.
    let src = "fn main() -> i32 {\n  arena {\n    p: box<i32> := heap.new(7)\n    q: box<i32> := p.clone()\n    p.get() + q.get()\n  }\n}\n";
    let out = build_and_run("clone", src);
    assert_eq!(out.status.code(), Some(14));
}

#[test]
fn arena_emits_begin_and_end() {
    let mut sm = SourceMap::new();
    let src = "fn main() -> i32 {\n  r: i32 := arena {\n    p: box<i32> := heap.new(1)\n    p.get()\n  }\n  return r\n}\n";
    let checked = check(&mut sm, "a.align", src);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("arena_begin"), "no arena_begin:\n{text}");
    assert!(text.contains("arena_end"), "no arena_end (bulk free):\n{text}");
}
