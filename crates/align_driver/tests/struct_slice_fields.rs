//! `slice<T>` **view** struct fields (F1③ of the pkg.web plan — the request `Ctx`'s captured param
//! slots). A slice field is a Copy `{ptr,len}` borrow: the enclosing struct owns nothing (stays
//! non-Move, no drop) but is **region-tied** to the buffer the slice views, so a struct holding a
//! slice that borrows a *local* array cannot escape that array's scope (use-after-free prevention).

mod common;
use common::*;

#[test]
fn slice_str_field_build_and_read() {
    if !backend_available() {
        return;
    }
    // Build a struct from a local array (array→slice coercion at the field, like a call arg), then
    // read the slice field's length and an element — a zero-copy `str` view into the array.
    let src = concat!(
        "Ctx { names: slice<str> }\n",
        "fn main() -> Result<(), Error> {\n",
        "  ns := [\"alpha\", \"beta\", \"gamma\"]\n",
        "  c := Ctx { names: ns }\n",
        "  print(c.names.len())\n",
        "  print(c.names[1])\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("slicefield-read", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\nbeta\n");
}

#[test]
fn slice_str_field_from_param_across_call() {
    if !backend_available() {
        return;
    }
    // The router shape: a helper builds the `Ctx` from a caller-provided `slice<str>` param (which
    // outlives the call), so returning the struct is sound — the borrow's source is the caller's.
    let src = concat!(
        "Ctx { names: slice<str> }\n",
        "fn build(ns: slice<str>) -> Ctx = Ctx { names: ns }\n",
        "fn main() -> Result<(), Error> {\n",
        "  a := [\"x\", \"y\", \"z\"]\n",
        "  c := build(a)\n",
        "  print(c.names[2])\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("slicefield-param", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "z\n");
}

#[test]
fn slice_field_struct_is_copy_not_moved() {
    if !backend_available() {
        return;
    }
    // A struct with only a slice field owns nothing → it is Copy: reading it (and its field) twice,
    // and passing it by value, must not be a use-after-move.
    let src = concat!(
        "Ctx { names: slice<str> }\n",
        "fn count(c: Ctx) -> i64 = c.names.len()\n",
        "fn main() -> Result<(), Error> {\n",
        "  ns := [\"a\", \"b\"]\n",
        "  c := Ctx { names: ns }\n",
        "  print(count(c))\n",
        "  print(count(c))\n",
        "  print(c.names[0])\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("slicefield-copy", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n2\na\n");
}

#[test]
fn slice_field_beside_owned_field_drops_cleanly() {
    if !backend_available() {
        return;
    }
    // A **Move** struct (it owns a `string`) that also carries a `slice<str>` view field and a
    // scalar: the slice field is a Copy borrow and owns nothing, so `drop_struct_fields` must free
    // only the `string` and skip the slice. A clean exit (0) proves the drop freed exactly once.
    let src = concat!(
        "Rec { title: string, tags: slice<str>, n: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  ts := [\"red\", \"blue\"]\n",
        "  r := Rec { title: \"hi\".clone(), tags: ts, n: 9 }\n",
        "  print(r.tags[1])\n",
        "  print(r.n)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("slicefield-owned-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "blue\n9\n");
}

#[test]
fn slice_field_escaping_local_array_rejected() {
    // A struct whose slice field views a function-local array cannot be returned — the array is
    // freed at return, so the view would dangle. The escape check (`region_of(StructLit)` folds the
    // field's region) must reject it, a clean diagnostic (no miscompile / use-after-free).
    let src = concat!(
        "Ctx { names: slice<str> }\n",
        "fn make() -> Ctx {\n",
        "  arena {\n",
        "    ns := [\"a\", \"b\"]\n",
        "    Ctx { names: ns }\n",
        "  }\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  c := make()\n",
        "  print(c.names.len())\n",
        "  return Ok(())\n",
        "}\n",
    );
    assert!(check_errs("slicefield-escape", src));
}
