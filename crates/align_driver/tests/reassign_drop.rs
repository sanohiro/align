//! Reassigning an owned `mut` local drops (frees) the value being overwritten, so its heap buffer
//! is not leaked — a pre-existing gap for *all* owned types (`string`/`array<T>`/Move struct/...),
//! noted as deferred by Slice 3 of `docs/impl/08-nested-structs.md`.
//!
//! The decision is made by sema's move analysis (the authority on whether the RHS consumed the old
//! value) and carried to MIR on `Stmt::Assign::drop_old`: the old value is dropped *iff* the RHS did
//! not move it out. So `s = f(s)` / `s = s` (RHS consumes `s` — ownership transferred) emit no drop
//! (no double-free), while `s = "b".clone()` / `s = other` / `s = make(s.len())` (RHS does not move
//! `s`) do (no leak). A *leak* can't be observed from a return value, so the leak-fix direction is
//! pinned by asserting on the emitted MIR; the no-double-free direction is pinned by running the
//! program (a wrong/extra free corrupts the allocator and aborts the process).

mod common;
use common::*;

/// Lower `src` to MIR and render it as text (for drop-scheduling assertions).
fn mir_text(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "reassign.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

/// The `fn main` block of a rendered MIR program (slots are numbered per function, so a helper
/// function reuses `_0` for its own param — counting program-wide would conflate the two).
fn main_fn(text: &str) -> &str {
    let start = text.find("fn main").expect("no fn main in MIR");
    let body = &text[start..];
    let end = body.find("\n}").map(|i| i + 2).unwrap_or(body.len());
    &body[..end]
}

/// `drop _N` (the slot free) is distinct from `drop_init _N` (the null-initialise); only the former
/// contains `"drop _N"` (the latter is `"drop_init _N"`). Counting it within `fn main` gives the
/// number of real frees of that slot. The match requires a non-digit (or end-of-line) right after
/// the slot id, so `"drop _1"` does not also count `drop _10` / `_11` (one statement per line, so a
/// per-line count equals the occurrence count).
fn count_slot_drops(text: &str, slot: &str) -> usize {
    let target = format!("drop {slot}");
    main_fn(text)
        .lines()
        .filter(|line| match line.find(&target) {
            Some(idx) => line.as_bytes().get(idx + target.len()).is_none_or(|&c| !c.is_ascii_digit()),
            None => false,
        })
        .count()
}

#[test]
fn mir_reassign_drops_old_string() {
    // `s` is slot _0 (the only local). It is reassigned once, then dropped at the function exit:
    // two real `drop _0` (the reassign free of "aaaa", the exit free of "bbbb"). Without the fix the
    // reassign would silently overwrite the slot and leak "aaaa" — only one `drop _0` would appear.
    let text = mir_text("fn main() -> i32 {\n  mut s := \"aaaa\".clone()\n  s = \"bbbb\".clone()\n  return 0\n}\n");
    assert_eq!(count_slot_drops(&text, "_0"), 2, "reassign + exit drop expected:\n{text}");
}

#[test]
fn mir_no_reassign_drop_when_rhs_consumes_old() {
    // `s = id(s)` moves `s` into the call (ownership transferred), so the old value must NOT be
    // dropped at the reassign — only the exit drop remains (one `drop _0`). A reassign drop here
    // would double-free the buffer the callee now owns.
    let text = mir_text("fn id(s: string) -> string = s\nfn main() -> i32 {\n  mut s := \"aaaa\".clone()\n  s = id(s)\n  return 0\n}\n");
    assert_eq!(count_slot_drops(&text, "_0"), 1, "only the exit drop expected:\n{text}");
}

#[test]
fn mir_reassign_drops_old_when_rhs_borrows_self() {
    // `s = dup(s)` passes `s` as a `str` *borrow* (not a move) and stores a fresh `string` back, so
    // the old buffer is leaked unless dropped: the move analysis (not a structural "RHS mentions s?"
    // heuristic) sees `s` is unconsumed and the old value is freed — reassign + exit = two drops.
    let text = mir_text("fn dup(v: str) -> string = v.clone()\nfn main() -> i32 {\n  mut s := \"aaaa\".clone()\n  s = dup(\"zzzz\")\n  return 0\n}\n");
    // (Using a literal arg keeps `s` a drop-local; the self-borrow shape `dup(s)` hits a separate,
    // pre-existing region-demotion that drops `s` out of the drop set entirely — out of scope here.)
    assert_eq!(count_slot_drops(&text, "_0"), 2, "reassign + exit drop expected:\n{text}");
}

#[test]
fn mir_no_drop_for_scalar_reassign() {
    // A Copy (non-owned) local is never a drop-local: reassigning it emits no drop at all.
    let text = mir_text("fn main() -> i32 {\n  mut x := 1\n  x = 2\n  return x\n}\n");
    assert_eq!(count_slot_drops(&text, "_0"), 0, "no drop for a scalar local:\n{text}");
    assert!(!text.contains("drop_init"), "no drop_init for a scalar local:\n{text}");
}

#[test]
fn reassign_string_runtime_no_double_free() {
    if !backend_available() {
        return;
    }
    // Reassign a `string` local twice, then read its length. Each overwritten buffer is freed once
    // at its reassign and the final one at exit — a double-free would abort. len("cccc") = 4.
    let src = "fn main() -> i32 {\n  mut s := \"aaaa\".clone()\n  s = \"bbbbb\".clone()\n  s = \"cccc\".clone()\n  return s.len() as i32\n}\n";
    assert_eq!(build_and_run("reassign-string", src).status.code(), Some(4));
}

#[test]
fn reassign_owned_struct_runtime_no_double_free() {
    if !backend_available() {
        return;
    }
    // Reassigning a whole Move struct frees the old struct's owned `name` buffer (recursive struct
    // drop) before the store. Returns the new age = 2; no double-free of "aaaa".
    let src = "User { name: string, age: i64 }\nfn main() -> i32 {\n  mut u := User{name: \"aaaa\".clone(), age: 1}\n  u = User{name: \"bbbb\".clone(), age: 2}\n  return u.age as i32\n}\n";
    assert_eq!(build_and_run("reassign-struct", src).status.code(), Some(2));
}

#[test]
fn reassign_consuming_runtime_no_double_free() {
    if !backend_available() {
        return;
    }
    // `s = id(s)` round-trips the buffer through the call (no drop at the reassign). The buffer is
    // freed once, at the function exit. len("aaaa") = 4.
    let src = "fn id(s: string) -> string = s\nfn main() -> i32 {\n  mut s := \"aaaa\".clone()\n  s = id(s)\n  return s.len() as i32\n}\n";
    assert_eq!(build_and_run("reassign-consume", src).status.code(), Some(4));
}

/// Whether `src` is rejected by sema with the region-change reassignment diagnostic (Rule 1).
fn rejects_region_change(src: &str) -> bool {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "reassign.align", src);
    if !checked.diags.has_errors() {
        return false;
    }
    align_driver::format_diagnostics(&sm, &checked.diags).contains("changes the value's memory region")
}

#[test]
fn arena_owned_array_reassign_region_change_rejected() {
    // An arena-allocated owned array reassigned with a *heap* (`Static`) value is a region change
    // (`Arena -> Static`). The old value is arena-bump memory (bulk-freed with the arena, never
    // individually); flow-insensitively upgrading `xs` to `Static` would enter it into `drop_locals`
    // and — on a bypassed branch that still holds the arena pointer — double-free it (the observed
    // `double free or corruption`). Rule 1 pins a `mut` binding's region at init and rejects this.
    // Both the conditional shape (the actual crash) and the unconditional shape are rejected.
    let cond = "fn make() -> array<i64> = [1, 2, 3].to_array()\n\
fn main() -> i32 {\n  arena {\n    mut xs := [10, 20, 30, 40].to_array()\n    if xs[0] > 100 { xs = make() }\n  }\n  return 0\n}\n";
    let uncond = "fn make() -> array<i64> = [1, 2, 3].to_array()\n\
fn main() -> i32 {\n  arena {\n    mut xs := [10, 20, 30, 40].to_array()\n    xs = make()\n  }\n  return 0\n}\n";
    assert!(rejects_region_change(cond), "conditional Arena->Static reassign must be a sema error");
    assert!(rejects_region_change(uncond), "unconditional Arena->Static reassign must be a sema error");
}

#[test]
fn arena_owned_array_reassign_region_change_rejected_in_loop() {
    // Rule 1 fires the same inside a `loop` body: an arena binding reassigned with a heap value on
    // any iteration would be a region change (and, in a loop, an unbounded leak/double-free). The
    // two-pass loop-back MoveCheck (#402) does not exempt it.
    let src = "fn make() -> array<i64> = [1, 2, 3].to_array()\n\
fn main() -> i32 {\n  arena {\n    mut xs := [10, 20].to_array()\n    mut i := 0\n    loop {\n      if i >= 3 { break 0 }\n      xs = make()\n      i = i + 1\n    }\n  }\n  return 0\n}\n";
    assert!(rejects_region_change(src), "a region-changing reassign in a loop body must be a sema error");
}

#[test]
fn same_arena_owned_array_reassign_no_drop_in_mir() {
    // Same-region reassignment stays legal: an arena binding reassigned with another *same-arena*
    // owned array is not a region change (`Arena -> Arena`), so it keeps its `drop_old` suppression
    // (arena memory is bulk-freed) — no reassign-drop, and no exit drop either (the final value is
    // still arena memory, freed by the arena, never by `drop_locals`).
    let text = mir_text(
        "fn main() -> i32 {\n  arena {\n    mut xs := [1, 2].to_array()\n    xs = [3, 4, 5].to_array()\n    print(xs[0])\n  }\n  return 0\n}\n",
    );
    assert_eq!(count_slot_drops(&text, "_0"), 0, "no reassign-drop and no exit drop for arena memory:\n{text}");
}

#[test]
fn same_arena_owned_array_reassign_runtime_no_double_free() {
    if !backend_available() {
        return;
    }
    // The same-arena reassign runs clean (the arena bulk-frees once); the final value is `xs[0]` of
    // the last array. A stray individual free of arena memory would abort the process.
    let src = "fn main() -> i32 {\n  arena {\n    mut xs := [1, 2].to_array()\n    xs = [7, 8, 9].to_array()\n    return xs[0] as i32\n  }\n}\n";
    assert_eq!(build_and_run("arena-same-region-reassign", src).status.code(), Some(7));
}

#[test]
fn heap_owned_array_reassign_in_loop_runtime_no_double_free() {
    if !backend_available() {
        return;
    }
    // Heap -> heap is same-region (`Static -> Static`): legal, and the overwritten buffer is freed
    // once per reassign (a reassign-drop). Inside a `loop` body this exercises Rule 1's legal path
    // against the loop-back MoveCheck. After 3 iterations `xs` holds `make(2)`, so `xs[0]` = 2.
    let src = "fn make(n: i64) -> array<i64> = [n, n, n].to_array()\n\
fn main() -> i32 {\n  mut xs := make(1)\n  mut i := 0\n  loop {\n    if i >= 3 { break 0 }\n    xs = make(i)\n    i = i + 1\n  }\n  return xs[0] as i32\n}\n";
    assert_eq!(build_and_run("heap-loop-reassign", src).status.code(), Some(2));
}

#[test]
fn view_reassign_region_upgrade_escape_rejected() {
    // A Copy region-bearing *view* (`str`) has no drop, but the same flow-insensitive tracking would
    // let a conditional `Arena -> Static` upgrade slip an escape: a `mut v` holding an arena `str`,
    // reassigned with a `Static` literal on one branch, then returned — on the bypassed branch `v`
    // still borrows the arena buffer (a use-after-free). Rule 2 intersects the regions (keeps `v`
    // pinned to the shortest it can hold on any path), so the return-escape check fires.
    let mut sm = SourceMap::new();
    let src = "fn f(cond: bool) -> str {\n  arena {\n    mut v := \"a\" + \"b\"\n    if cond { v = \"static\" }\n    return v\n  }\n}\nfn main() -> i32 {\n  print(f(false).len())\n  return 0\n}\n";
    let checked = check(&mut sm, "view-reassign.align", src);
    assert!(checked.diags.has_errors(), "an arena view escaping via a region-upgraded reassign must be rejected");
    let msg = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(msg.contains("allocated in an arena"), "expected the arena-escape diagnostic, got:\n{msg}");
}

#[test]
fn struct_owned_field_reassign_no_leak_no_double_free() {
    if !backend_available() {
        return;
    }
    // A field-level owned reassign (`u.name = new`) frees the OLD field value before the store
    // (Slice 4b — Slice 3 only handled whole-struct reassign). Runs clean; len("new") = 3.
    let src = "User { name: string, age: i64 }\nfn main() -> i32 {\n  mut u := User{name: \"aaaa\".clone(), age: 1}\n  u.name = \"new\".clone()\n  return u.name.len() as i32\n}\n";
    assert_eq!(build_and_run("field-reassign", src).status.code(), Some(3));
}

#[test]
fn struct_owned_field_reassign_from_variable_stores_value_not_null() {
    if !backend_available() {
        return;
    }
    // Regression (gemini #283): the RHS is a *variable* — `store_value_at` lowers it internally, so
    // its moved source must be nulled *after* the store, not before (else null is stored). len = 3.
    let src = "User { name: string, age: i64 }\nfn main() -> i32 {\n  mut u := User{name: \"aaaa\".clone(), age: 1}\n  v := \"bee\".clone()\n  u.name = v\n  return u.name.len() as i32\n}\n";
    assert_eq!(build_and_run("field-reassign-var", src).status.code(), Some(3));
}
