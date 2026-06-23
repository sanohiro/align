//! Whole-struct element indexing: `arr[i]` on a struct array yields the struct by value (a copy).
//! Closes the consistency gap (struct-by-value worked for params/return/locals but not indexing).
//! A struct is Copy (primitive / `str` fields); a `str`-bearing struct is region-tied to the array.

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
    let out = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    out
}

fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

#[test]
fn fixed_struct_array_whole_element() {
    if !backend_available() {
        return;
    }
    // `us[1]` copies the whole struct out; its fields read fine.
    let src = "User { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  us := [User{id: 1, score: 10}, User{id: 2, score: 20}]\n  u := us[1]\n  print(u.score)\n  print(u.id)\n  return Ok(())\n}\n";
    let out = build_and_run("si-fixed", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "20\n2\n");
}

#[test]
fn dynamic_struct_array_whole_element() {
    if !backend_available() {
        return;
    }
    // A json-decoded `array<User>` (DynStructArray, `{ptr,len}`): `us[0]` loads the whole struct.
    let src = "User { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  arena {\n    us: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"score\\\":10},{\\\"id\\\":2,\\\"score\\\":20}]\")?\n    u := us[1]\n    print(u.score)\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("si-dyn", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "20\n");
}

#[test]
fn primitive_struct_element_returns_freely() {
    if !backend_available() {
        return;
    }
    // A struct with only primitive fields is Static (no region tie) — its element is returnable
    // and usable. `ps[0]` then `.x` reads 7.
    let src = "P { x: i32, y: i32 }\nfn main() -> i32 {\n  ps := [P{x: 7, y: 8}, P{x: 9, y: 10}]\n  q := ps[0]\n  return q.x\n}\n";
    let out = build_and_run("si-prim", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn primitive_struct_element_is_returnable_from_a_function() {
    // Sema: a primitive-only struct element (region `Static`) may be returned out of a function.
    let src = "P { x: i32, y: i32 }\nfn first(ps: array<P>) -> P = ps[0]\nfn main() -> i32 = 0\n";
    assert!(!check_errs("si-fn-return", src));
}

#[test]
fn struct_with_owned_field_rejected() {
    // A struct is Copy (no per-binding Drop), and `arr[i]` / `:=` copy it by value — so a Move
    // (owned) field would double-free across copies. Such fields are rejected at declaration,
    // which keeps whole-struct indexing (and struct copy in general) sound.
    assert!(check_errs("si-owned-string", "U { name: string }\nfn main() -> i32 = 0\n"));
    assert!(check_errs("si-owned-array", "U { items: array<i64> }\nfn main() -> i32 = 0\n"));
}

#[test]
fn str_bearing_struct_element_cannot_escape_arena() {
    // A `str`-bearing struct is region-tied to the array; indexing one out of an arena-decoded
    // array and letting it escape the arena is rejected (the `str` view would dangle).
    let src = "U { id: i64, name: str }\nfn bad(j: str) -> i64 {\n  mut keep := U{id: 0, name: \"\"}\n  arena {\n    us: array<U> := json.decode(j)?\n    keep = us[0]\n  }\n  return keep.id\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("si-escape", src));
}
