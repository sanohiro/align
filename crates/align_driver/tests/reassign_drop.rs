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

#[test]
fn arena_owned_array_reassign_no_double_free() {
    if !backend_available() {
        return;
    }
    // 1-3: `mut xs := […].to_array()` allocated in an arena, reassigned to another owned array.
    // The overwritten value is arena-bump memory (bulk-freed by the arena), so it must NOT get a
    // reassign-drop — freeing an interior arena pointer individually corrupts the allocator
    // (the observed `double free detected in tcache`). Running to completion proves it doesn't.
    let src = "fn id64(x: i64) -> i64 = x\n\
fn make() -> array<i64> {\n  ys := [7, 8, 9, 10, 11, 12, 13, 14].map(id64).to_array()\n  return ys\n}\n\
fn main() -> i32 {\n  arena {\n    mut xs := [1, 2, 3, 4, 5, 6, 7, 8].map(id64).to_array()\n    xs = make()\n    xs = make()\n    print(xs[0])\n  }\n  return 0\n}\n";
    assert_eq!(build_and_run("arena-reassign", src).status.code(), Some(0));
}

#[test]
fn arena_owned_array_reassign_suppresses_reassign_drop_in_mir() {
    // The two reassigns of the arena-allocated `xs` must emit no reassign-drop (arena memory is
    // bulk-freed); only the single exit drop of the final, heap-owned value remains.
    let text = mir_text(
        "fn id64(x: i64) -> i64 = x\n\
fn make() -> array<i64> {\n  ys := [7, 8, 9].map(id64).to_array()\n  return ys\n}\n\
fn main() -> i32 {\n  arena {\n    mut xs := [1, 2].map(id64).to_array()\n    xs = make()\n    print(xs[0])\n  }\n  return 0\n}\n",
    );
    assert_eq!(count_slot_drops(&text, "_0"), 1, "only the exit drop, no reassign-drop:\n{text}");
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
