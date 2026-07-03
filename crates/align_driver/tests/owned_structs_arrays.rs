//! Slice 4a: a fixed array of a **Move** struct (a struct that owns a `string`/owned field). The
//! array is dropped **element-by-element** at scope exit — each element's owned fields freed once,
//! no leak, no double-free. Construction + scalar-field read are supported; mutation (reassign /
//! element store) and reading an owned field out of an element are deferred (Slice 4b).

mod common;
use common::*;

#[test]
fn construct_read_and_drop_runs_clean() {
    if !backend_available() {
        return;
    }
    // Two elements, each owning a heap `string`. Reading scalar fields works; the per-element drop
    // at scope exit frees both `string` buffers exactly once (a wrong/extra free aborts the process).
    let src = "\
User { name: string, age: i64 }
fn main() -> i32 {
  us := [User{name: \"alice\".clone(), age: 3}, User{name: \"bob\".clone(), age: 4}]
  return (us[0].age + us[1].age) as i32
}
";
    let out = build_and_run("ms-array-drop", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn per_element_drop_is_scheduled_in_mir() {
    // The array slot is null-initialised and dropped once at the function exit (the drop lowers to
    // a per-element free loop in codegen). Pins that a Move-struct array is drop-scheduled at all.
    let mut sm = SourceMap::new();
    let src = "\
User { name: string, age: i64 }
fn main() -> i32 {
  us := [User{name: \"a\".clone(), age: 1}]
  return us[0].age as i32
}
";
    let checked = check(&mut sm, "ms-array-mir", src);
    assert!(!checked.diags.has_errors(), "unexpected errors:\n{}", align_driver::format_diagnostics(&sm, &checked.diags));
    let text = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    let main = &text[text.find("fn main").unwrap()..];
    assert!(main.contains("drop_init _0"), "array slot must be null-initialised:\n{main}");
    assert!(main.lines().any(|l| l.trim_start().starts_with("drop _0")), "array must be dropped at exit:\n{main}");
}

#[test]
fn element_replace_drops_old_and_moves_in() {
    if !backend_available() {
        return;
    }
    // Slice 4b: `us[i] = newStruct` frees the *old* element's owned fields (drop-of-old) and moves
    // the new value in; the source struct's slot is nulled so it isn't double-freed. us[0] "alice"
    // is freed, "carol"(9) moves in; us[1] "bob"(2) untouched → 9 + 2 = 11. No leak / double-free.
    let src = "\
User { name: string, age: i64 }
fn main() -> i32 {
  mut us := [User{name: \"alice\".clone(), age: 1}, User{name: \"bob\".clone(), age: 2}]
  us[0] = User{name: \"carol\".clone(), age: 9}
  return (us[0].age + us[1].age) as i32
}
";
    assert_eq!(build_and_run("ms-elem-replace", src).status.code(), Some(11));
}

#[test]
fn element_replace_from_a_variable_nulls_the_source() {
    if !backend_available() {
        return;
    }
    // The RHS is a struct *variable*: moving it into the element must null its slot so its own exit
    // drop is a no-op (no double-free of "new").
    let src = "\
User { name: string, age: i64 }
fn main() -> i32 {
  mut us := [User{name: \"a\".clone(), age: 1}]
  v := User{name: \"new\".clone(), age: 7}
  us[0] = v
  return us[0].age as i32
}
";
    assert_eq!(build_and_run("ms-elem-replace-var", src).status.code(), Some(7));
}

#[test]
fn whole_array_reassignment_is_rejected() {
    // A fixed array can't be *wholly* reassigned (array values aren't materialized) — assign
    // elements individually. Clean error for a Move-struct array (and a scalar array alike).
    assert!(check_errs(
        "ms-array-whole-reassign",
        "User { name: string }\nfn main() -> i32 {\n  mut us := [User{name: \"a\".clone()}]\n  us = [User{name: \"b\".clone()}]\n  return 0\n}\n"
    ));
    assert!(check_errs(
        "scalar-array-whole-reassign",
        "fn main() -> i32 {\n  mut xs := [1, 2, 3]\n  xs = [4, 5, 6]\n  return 0\n}\n"
    ));
}

#[test]
fn reading_string_field_of_element_as_a_view() {
    if !backend_available() {
        return;
    }
    // Slice 4b: reading a `string` field of an element (`us[i].name`) yields a borrowed `str` view —
    // usable for `.len()`, comparison, `str` args, and `.clone()` for an owned copy. No ownership
    // transfer (a runtime index can't track which element gave up its buffer).
    let src = "\
User { name: string, age: i64 }
fn main() -> i32 {
  us := [User{name: \"alice\".clone(), age: 1}, User{name: \"bobby\".clone(), age: 2}]
  n := us[0].name.clone()
  return (us[1].name.len() + n.len()) as i32
}
";
    assert_eq!(build_and_run("ms-array-field-view", src).status.code(), Some(10)); // 5 + 5
}

#[test]
fn string_field_view_of_element_cannot_escape_the_array() {
    // The view borrows the array's storage (freed at the array's scope exit), so returning it — or
    // storing it past the array's scope — is rejected; `.clone()` is the escape hatch.
    assert!(check_errs(
        "ms-array-view-escape",
        "User { name: string }\nfn bad() -> str {\n  us := [User{name: \"x\".clone()}]\n  return us[0].name\n}\nfn main() -> i32 = 0\n"
    ));
    // Binding it to an owned `string` (a move) is a type mismatch — use `.clone()`.
    assert!(check_errs(
        "ms-array-view-move",
        "User { name: string }\nfn main() -> i32 {\n  us := [User{name: \"x\".clone()}]\n  n: string := us[0].name\n  return 0\n}\n"
    ));
    // The view of an array literal indexed *directly* (not bound to a variable) is likewise
    // frame-local — returning it must be rejected too (the temporary's buffer dies within the frame).
    assert!(check_errs(
        "ms-array-lit-view-escape",
        "User { name: string }\nfn bad() -> str {\n  return [User{name: \"x\".clone()}][0].name\n}\nfn main() -> i32 = 0\n"
    ));
}

#[test]
fn copy_struct_array_still_works() {
    if !backend_available() {
        return;
    }
    // A Copy struct array (no owned fields) is unaffected — no per-element drop, indexes as before.
    let src = "\
Point { x: i64, y: i64 }
fn main() -> i32 {
  ps := [Point{x: 1, y: 2}, Point{x: 3, y: 4}]
  return (ps[1].x + ps[1].y) as i32
}
";
    let out = build_and_run("copy-array", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn element_owned_field_reassign_drops_old() {
    if !backend_available() {
        return;
    }
    // Slice 4b: `us[i].name = new` frees the OLD field value before storing the new one (else it
    // leaks) and nulls a moved-in variable's slot. Two elements untouched apart from us[0].name.
    let src = "\
User { name: string, age: i64 }
fn main() -> i32 {
  mut us := [User{name: \"aaaa\".clone(), age: 1}, User{name: \"bee\".clone(), age: 2}]
  us[0].name = \"cc\".clone()
  return (us[0].name.len() + us[1].name.len()) as i32
}
";
    // "aaaa" freed (drop-of-old), "cc"(2) stored; us[1] "bee"(3) untouched → 2 + 3 = 5.
    assert_eq!(build_and_run("ms-elem-field-reassign", src).status.code(), Some(5));
}

#[test]
fn element_nested_owned_field_reassign_drops_old() {
    if !backend_available() {
        return;
    }
    // Slice 4 (nested element write): `us[i].addr.name = new` overwrites a `string` reached through
    // a *nested* struct field. The path-carrying `DropElemField` frees the OLD buffer at
    // `[0, i, addr, name]` before storing the new one (else it leaks); the RHS `.clone()` temporary
    // is stored in. At exit the per-element drop frees the surviving nested buffers. No leak, no
    // double-free — verified by a clean exit code (and the read-back of the nested `string` view).
    let src = "\
Addr { name: string }
User { addr: Addr, age: i64 }
fn main() -> i32 {
  mut us := [User{addr: Addr{name: \"aaaa\".clone()}, age: 1}, User{addr: Addr{name: \"bee\".clone()}, age: 2}]
  us[0].addr.name = \"cc\".clone()
  return (us[0].addr.name.len() + us[1].addr.name.len()) as i32
}
";
    // "aaaa" freed (nested drop-of-old), "cc"(2) stored; us[1] "bee"(3) untouched → 2 + 3 = 5.
    assert_eq!(build_and_run("ms-elem-nested-field-reassign", src).status.code(), Some(5));
}
