//! Whole-struct element indexing: `arr[i]` on a struct array yields the struct by value (a copy).
//! Closes the consistency gap (struct-by-value worked for params/return/locals but not indexing).
//! A struct is Copy (primitive / `str` fields); a `str`-bearing struct is region-tied to the array.


mod common;
use common::*;

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
    let src = "import core.json\nUser { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  arena {\n    us: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"score\\\":10},{\\\"id\\\":2,\\\"score\\\":20}]\")?\n    u := us[1]\n    print(u.score)\n  }\n  return Ok(())\n}\n";
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
fn struct_with_owned_field() {
    // Slice 3: a `string` field is now allowed — the struct becomes a Move type with a recursive
    // Drop, and a move (not a silent Copy) transfers it. An *array* of such a Move struct is still
    // rejected (per-element drop = a later slice), so whole-struct indexing stays sound.
    assert!(!check_errs("si-owned-string", "U { name: string }\nfn main() -> i32 = 0\n"));
    assert!(check_errs(
        "si-owned-string-arr",
        "U { name: string }\nfn main() -> i32 {\n  us := [U{name: \"a\".clone()}, U{name: \"b\".clone()}]\n  return 0\n}\n"
    ));
    // An owned *collection* (`array<T>`) field is still rejected (only `string` owned fields so far).
    assert!(check_errs("si-owned-array", "U { items: array<i64> }\nfn main() -> i32 = 0\n"));
}

#[test]
fn str_bearing_struct_element_cannot_escape_arena() {
    // A `str`-bearing struct is region-tied to the array; indexing one out of an arena-decoded
    // array and letting it escape the arena is rejected (the `str` view would dangle).
    let src = "import core.json\nU { id: i64, name: str }\nfn bad(j: str) -> i64 {\n  mut keep := U{id: 0, name: \"\"}\n  arena {\n    us: array<U> := json.decode(j)?\n    keep = us[0]\n  }\n  return keep.id\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("si-escape", src));
}

// --- Nested field of a struct-array element (`arr[i].a.x`), Slice 4 ---
// Direct nested access on an indexed element: previously only `arr[i].field` (depth-1) worked, and a
// nested read needed an intermediate `p := arr[i].a; p.x`. The first field loads the sub-struct; the
// remaining path projects out of it (the pipeline's single-field seam is untouched).

#[test]
fn elem_nested_field_read() {
    if !backend_available() {
        return;
    }
    // `ls[0].a.x` (= 1) and `ls[0].b.y` (= 4) → 5; runtime index `ls[k].a.x` over two elements.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "Line { a: Point, b: Point }\n",
        "fn main() -> i32 {\n",
        "  ls := [Line{a: Point{x: 1, y: 2}, b: Point{x: 3, y: 4}}, Line{a: Point{x: 10, y: 0}, b: Point{x: 0, y: 0}}]\n",
        "  mut k := 1\n",
        "  return (ls[0].a.x + ls[0].b.y + ls[k].a.x) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("elem-nested", src).status.code(), Some(15)); // 1 + 4 + 10
}

#[test]
fn elem_nested_field_depth_three() {
    if !backend_available() {
        return;
    }
    // Depth-3 path on an element: `cs[0].b.a.v`.
    let src = "A { v: i64 }\nB { a: A }\nC { b: B }\nfn main() -> i32 {\n  cs := [C{b: B{a: A{v: 42}}}]\n  return cs[0].b.a.v as i32\n}\n";
    assert_eq!(build_and_run("elem-nested3", src).status.code(), Some(42));
}

#[test]
fn elem_field_through_scalar_rejected() {
    // A non-final field in the path must be a struct: `arr[i].a.z` where `a` is a scalar is an error.
    assert!(check_errs(
        "elem-scalar-path",
        "Line { a: i64 }\nfn main() -> i32 {\n  ls := [Line{a: 5}]\n  return ls[0].a.z as i32\n}\n",
    ));
}

#[test]
fn elem_field_assign_writes_one_field() {
    if !backend_available() {
        return;
    }
    // `arr[i].field = v` — the write counterpart of the `arr[i].field` read: one field of one
    // element is stored, others untouched. arr[0].a=77, arr[1].b=8, arr[0].b still 2 → 77+8+2 = 87.
    let src = concat!(
        "P { a: i64, b: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut arr := [P{a: 1, b: 2}, P{a: 3, b: 4}]\n",
        "  arr[0].a = 77\n",
        "  arr[1].b = 8\n",
        "  return (arr[0].a + arr[1].b + arr[0].b) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("elem-field-assign", src).status.code(), Some(87));
}

#[test]
fn elem_field_assign_dynamic_index() {
    if !backend_available() {
        return;
    }
    // A runtime index into the element-field store (not a constant), bounds-checked at the write.
    let src = concat!(
        "P { a: i64, b: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut arr := [P{a: 1, b: 2}, P{a: 3, b: 4}, P{a: 5, b: 6}]\n",
        "  mut i := 2\n",
        "  arr[i].a = 40\n",
        "  return arr[2].a as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("elem-field-assign-dyn", src).status.code(), Some(40));
}

#[test]
fn elem_field_assign_immutable_rejected() {
    // Writing a field of an element requires the array local to be `mut`.
    assert!(check_errs(
        "elem-field-assign-immut",
        "P { a: i64, b: i64 }\nfn main() -> i32 {\n  arr := [P{a: 1, b: 2}]\n  arr[0].a = 9\n  return 0\n}\n",
    ));
}
