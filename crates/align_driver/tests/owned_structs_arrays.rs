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
fn reading_owned_field_out_of_element_is_rejected() {
    // Moving/reading an owned (`string`) field out of an array element is deferred (Slice 4b);
    // scalar fields are fine.
    assert!(check_errs(
        "ms-array-fieldout",
        "User { name: string }\nfn main() -> i32 {\n  us := [User{name: \"a\".clone()}]\n  n := us[0].name\n  return 0\n}\n"
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
