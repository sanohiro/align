//! Reassigning an owned `mut` local drops (frees) the value being overwritten, so its heap buffer
//! is not leaked — a pre-existing gap for *all* owned types (`string`/`array<T>`/Move struct/...),
//! noted as deferred by Slice 3 of `docs/impl/08-nested-structs.md`.
//!
//! The decision is made by sema's move analysis (the authority on whether the RHS consumed the old
//! value) and carried to MIR on `Stmt::Assign::drop_old`. MIR conditionally drops the old value when
//! its path-local flag says the slot is individually owned; arena-owned, moved, and uninitialised
//! paths skip it. The replacement updates that flag, and direct moves transfer it to the destination.
//! A *leak* can't be observed from a return value, so the leak-fix direction is pinned by asserting
//! on the emitted MIR; the no-double-free direction is pinned by running the program.

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
fn region_free_move_resource_reassign_keeps_the_new_flag_live() {
    // A builder has no escape Region, but it is still an individually owned Move resource. Both
    // its declaration and replacement must set the flag so the unfinished replacement is dropped.
    let text = mir_text("fn main() -> i32 {\n  mut b := builder()\n  b.write(\"old\")\n  b = builder()\n  b.write(\"new\")\n  return 0\n}\n");
    let main = main_fn(&text);
    assert!(
        main.lines().filter(|line| line.contains("_1 <- true")).count() >= 2,
        "both builder writes must be individually owned:\n{main}"
    );
}

#[test]
fn owned_call_result_stays_individual_with_an_arena_borrow_argument() {
    // Escape Region conservatively follows the arena-backed argument, but the callee cannot return
    // arena storage across its function boundary. The returned string is heap-owned on both writes.
    let text = mir_text("fn copy(s: str) -> string = s.clone()\nfn main() -> i32 {\n  arena {\n    source := \"a\" + \"b\"\n    mut out := copy(source)\n    out = copy(source)\n    return out.len() as i32\n  }\n}\n");
    let main = main_fn(&text);
    assert!(
        main.lines().filter(|line| line.contains("_2 <- true")).count() >= 2,
        "owned call results must not inherit an argument's arena allocation mode:\n{main}"
    );
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
fn arena_to_heap_reassign_sets_a_path_local_drop_flag() {
    // `xs` starts arena-owned (flag false). Only the conditional reassign installs a heap value
    // (flag true), so the shared exit conditionally drops it without ever freeing the arena pointer.
    let text = mir_text("fn make() -> array<i64> = [1, 2, 3].to_array()\nfn main() -> i32 {\n  arena {\n    mut xs := [10, 20, 30, 40].to_array()\n    if xs[0] > 100 { xs = make() }\n  }\n  return 0\n}\n");
    let main = main_fn(&text);
    assert!(main.contains("_1 <- false"), "arena path must clear the flag:\n{main}");
    assert!(main.contains("_1 <- true"), "heap replacement must set the flag:\n{main}");
    assert!(main.contains("drop _0"), "the true flag edge must drop the slot:\n{main}");
}

#[test]
fn arena_to_heap_reassign_runs_on_taken_and_bypassed_paths() {
    if !backend_available() {
        return;
    }
    let src = |cond| format!("fn make() -> array<i64> = [7, 8, 9].to_array()\nfn main() -> i32 {{\n  arena {{\n    mut xs := [3, 4].to_array()\n    if {cond} {{ xs = make() }}\n    return xs[0] as i32\n  }}\n}}\n");
    assert_eq!(build_and_run("arena-heap-taken", &src("true")).status.code(), Some(7));
    assert_eq!(build_and_run("arena-heap-bypassed", &src("false")).status.code(), Some(3));
}

#[test]
fn joined_region_move_transfers_the_runtime_drop_flag() {
    if !backend_available() {
        return;
    }
    // After the `if`, escape analysis conservatively joins `xs` to Arena, but its runtime value may
    // be heap-owned. Moving it into `ys` must copy the flag before clearing `xs`, not recompute the
    // destination from that joined region.
    let src = |cond| format!("fn make() -> array<i64> = [7, 8, 9].to_array()\nfn main() -> i32 {{\n  arena {{\n    mut xs := [3, 4].to_array()\n    if {cond} {{ xs = make() }}\n    ys := xs\n    return ys[0] as i32\n  }}\n}}\n");
    assert_eq!(build_and_run("joined-move-heap", &src("true")).status.code(), Some(7));
    assert_eq!(build_and_run("joined-move-arena", &src("false")).status.code(), Some(3));
}

#[test]
fn match_payload_binding_inherits_a_joined_scrutinee_flag() {
    if !backend_available() {
        return;
    }
    // Match payload bindings are another ownership-transfer site: the bound `xs` must inherit the
    // mixed Result local's runtime bit before the scrutinee is cleared.
    let src = |cond| format!("fn make() -> array<i64> = [7, 8, 9].to_array()\nfn main() -> i32 {{\n  arena {{\n    mut r: Result<array<i64>, i64> := Ok([3, 4].to_array())\n    if {cond} {{ r = Ok(make()) }}\n    return match r {{\n      Ok(xs) => xs[0] as i32\n      Err(_) => 0\n    }}\n  }}\n}}\n");
    assert_eq!(build_and_run("joined-match-heap", &src("true")).status.code(), Some(7));
    assert_eq!(build_and_run("joined-match-arena", &src("false")).status.code(), Some(3));
}

#[test]
fn arena_to_heap_reassign_in_loop_drops_each_heap_replacement() {
    if !backend_available() {
        return;
    }
    // The first iteration replaces arena memory (skip old drop); later iterations replace heap
    // memory (drop old), and the final heap value is dropped at function exit.
    let src = "fn make() -> array<i64> = [1, 2, 3].to_array()\n\
fn main() -> i32 {\n  arena {\n    mut xs := [10, 20].to_array()\n    mut i := 0\n    loop {\n      if i >= 3 { break 0 }\n      xs = make()\n      i = i + 1\n    }\n    return xs[0] as i32\n  }\n}\n";
    assert_eq!(build_and_run("arena-heap-loop", src).status.code(), Some(1));
}

#[test]
fn same_arena_owned_array_keeps_drop_flag_clear() {
    // Both writes are arena-owned. Conditional drop edges exist for the slot, but no path sets its
    // flag, so neither the overwrite nor exit executes an individual free.
    let text = mir_text(
        "fn main() -> i32 {\n  arena {\n    mut xs := [1, 2].to_array()\n    xs = [3, 4, 5].to_array()\n    print(xs[0])\n  }\n  return 0\n}\n",
    );
    let main = main_fn(&text);
    assert!(!main.contains("_1 <- true"), "arena writes must never set the flag:\n{main}");
    assert!(main.contains("_1 <- false"), "arena writes must keep the flag clear:\n{main}");
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
fn heap_to_arena_reassign_drops_old_heap_and_skips_final_arena_value() {
    if !backend_available() {
        return;
    }
    // A binding declared inside the arena may start with a callee-returned heap array and then hold
    // arena memory. The old heap value is individually freed; the replacement is bulk-freed only.
    let src = "fn make() -> array<i64> = [1, 2, 3].to_array()\nfn main() -> i32 {\n  arena {\n    mut xs := make()\n    xs = [9, 8].to_array()\n    return xs[0] as i32\n  }\n}\n";
    assert_eq!(build_and_run("heap-arena-reassign", src).status.code(), Some(9));
}

#[test]
fn owned_self_assign_preserves_the_value_and_drop_flag() {
    if !backend_available() {
        return;
    }
    // Lowering captures the RHS before clearing the moved source, then restores both the value and
    // its ownership flag. Nulling after the store would replace the destination with `{null, 0}`.
    let src = "fn main() -> i32 {\n  mut s := \"hello\".clone()\n  s = s\n  return s.len() as i32\n}\n";
    assert_eq!(build_and_run("owned-self-assign", src).status.code(), Some(5));
}

#[test]
fn heap_owned_array_reassign_in_loop_runtime_no_double_free() {
    if !backend_available() {
        return;
    }
    // Heap -> heap is same-region (`Static -> Static`): legal, and the overwritten buffer is freed
    // once per reassign (a conditional reassign-drop). Inside a `loop` body this exercises flag
    // updates against the loop-back MoveCheck. After 3 iterations `xs` holds `make(2)`, so `xs[0]` = 2.
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
