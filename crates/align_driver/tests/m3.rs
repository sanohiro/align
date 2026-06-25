//! M3 end-to-end: arena + heap box (allocation, `.get()`, bulk free, cleanup on
//! early exit). Requires LLVM/cc, so skip where they are absent.


mod common;
use common::*;

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
fn nested_arenas_run_correctly() {
    if !backend_available() {
        return;
    }
    // Inner arena's box value is copied out (scalar) before its arena ends; both
    // arenas are freed. 10 + 5 = 15.
    let src = "fn main() -> i32 {\n  arena {\n    a: box<i32> := heap.new(10)\n    inner: i32 := arena {\n      b: box<i32> := heap.new(5)\n      b.get()\n    }\n    a.get() + inner\n  }\n}\n";
    let out = build_and_run("nested-arena", src);
    assert_eq!(out.status.code(), Some(15));
}

#[test]
fn early_return_from_nested_arena() {
    if !backend_available() {
        return;
    }
    // `return` inside a nested arena frees both arenas first. f(1) = 1 + 2 = 3.
    let src = "fn f(x: i32) -> i32 {\n  arena {\n    a: box<i32> := heap.new(1)\n    arena {\n      b: box<i32> := heap.new(2)\n      if x > 0 { return a.get() + b.get() }\n      b.get()\n    }\n  }\n}\nfn main() -> i32 {\n  return f(1)\n}\n";
    let out = build_and_run("nested-early", src);
    assert_eq!(out.status.code(), Some(3));
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
