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
fn mut_move_struct_array_is_rejected() {
    // Mutation needs per-element drop-of-old (Slice 4b); a `mut` binding is a clean error for now.
    assert!(check_errs(
        "ms-array-mut",
        "User { name: string }\nfn main() -> i32 {\n  mut us := [User{name: \"a\".clone()}]\n  return 0\n}\n"
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
