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
    // Drop, and a move (not a silent Copy) transfers it. Slice 4a: a fixed *array* of such a Move
    // struct is now supported too (dropped element-by-element), so both check clean.
    assert!(!check_errs("si-owned-string", "U { name: string }\nfn main() -> i32 = 0\n"));
    assert!(!check_errs(
        "si-owned-string-arr",
        "U { name: string }\nfn main() -> i32 {\n  us := [U{name: \"a\".clone()}, U{name: \"b\".clone()}]\n  return 0\n}\n"
    ));
    // REST-gateway runway Slice C (#529) lifted the owned-collection-field restriction: an owned
    // `array<T>` field is now allowed (the struct becomes a Move type with a recursive Drop that frees
    // the array). Construct + index + drop round-trips (`items: array<i64>`).
    assert!(!check_errs("si-owned-array", "U { items: array<i64> }\nfn main() -> i32 = 0\n"));
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

// --- Nested element-field write (`arr[i].a.x = v`, Slice 4): the write counterpart of the
// `arr[i].a.x` read. The `StoreElemField` field became a `Vec<u32>` path, symmetric with the read
// side's `ElemField.path` and the local-field-path `StoreField`. ---

#[test]
fn elem_nested_field_assign_writes_leaf() {
    if !backend_available() {
        return;
    }
    // Depth-2 write→read: `ls[k].a.x` and `ls[0].b.y` are overwritten; the other leaves stay put.
    // ls[0]={a:{1,2}, b:{3,4}}, ls[1]={a:{10,0}, b:{0,0}}. Write ls[0].b.y=40, ls[1].a.x=100.
    // Read ls[0].a.x(1) + ls[0].b.y(40) + ls[1].a.x(100) = 141.
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "Line { a: Point, b: Point }\n",
        "fn main() -> i32 {\n",
        "  mut ls := [Line{a: Point{x: 1, y: 2}, b: Point{x: 3, y: 4}}, Line{a: Point{x: 10, y: 0}, b: Point{x: 0, y: 0}}]\n",
        "  mut k := 1\n",
        "  ls[0].b.y = 40\n",
        "  ls[k].a.x = 100\n",
        "  return (ls[0].a.x + ls[0].b.y + ls[k].a.x) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("elem-nested-write", src).status.code(), Some(141));
}

#[test]
fn elem_nested_field_assign_depth_three() {
    if !backend_available() {
        return;
    }
    // Depth-3 element write then read back: `cs[0].b.a.v = 42`.
    let src = "A { v: i64 }\nB { a: A }\nC { b: B }\nfn main() -> i32 {\n  mut cs := [C{b: B{a: A{v: 1}}}]\n  cs[0].b.a.v = 42\n  return cs[0].b.a.v as i32\n}\n";
    assert_eq!(build_and_run("elem-nested-write3", src).status.code(), Some(42));
}

#[test]
fn elem_nested_field_assign_reorder_mix() {
    if !backend_available() {
        return;
    }
    // #307 field reordering at *every* level: both the outer and the nested struct mix widths, so
    // the descending-alignment physical layout is permuted. A correct nested write must route each
    // path segment through the logical→physical `pfield` map. Writes: q.a=7, q.b=100, q.c=20, p=3.
    // Sum q.a(7) + q.b(100) + q.c(20) + p(3) + r(5, untouched) = 135.
    let src = concat!(
        "Inner { a: i8, b: i64, c: i16 }\n",
        "Outer { p: i32, q: Inner, r: i8 }\n",
        "fn main() -> i32 {\n",
        "  mut arr := [Outer{p: 0, q: Inner{a: 0, b: 0, c: 0}, r: 5}]\n",
        "  arr[0].q.a = 7\n",
        "  arr[0].q.b = 100\n",
        "  arr[0].q.c = 20\n",
        "  arr[0].p = 3\n",
        "  return ((arr[0].q.a as i64) + arr[0].q.b + (arr[0].q.c as i64) + (arr[0].p as i64) + (arr[0].r as i64)) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("elem-nested-write-reorder", src).status.code(), Some(135));
}

#[test]
fn elem_nested_field_assign_immutable_rejected() {
    // Writing a nested field of an element still requires the array local to be `mut`.
    assert!(check_errs(
        "elem-nested-write-immut",
        "Point { x: i64, y: i64 }\nLine { a: Point, b: Point }\nfn main() -> i32 {\n  ls := [Line{a: Point{x: 1, y: 2}, b: Point{x: 3, y: 4}}]\n  ls[0].a.x = 9\n  return 0\n}\n",
    ));
}

#[test]
fn elem_nested_field_assign_through_scalar_rejected() {
    // A non-final field in the write path must be a struct: `arr[i].a.z = v` where `a` is a scalar.
    assert!(check_errs(
        "elem-nested-write-scalar",
        "Line { a: i64 }\nfn main() -> i32 {\n  mut ls := [Line{a: 5}]\n  ls[0].a.z = 9\n  return 0\n}\n",
    ));
}

#[test]
fn elem_nested_field_assign_moved_value_rejected() {
    // MoveCheck still sees the RHS through the (now path-carrying) element-field store: writing a
    // moved-out `string` into a nested leaf is a use-after-move.
    assert!(check_errs(
        "elem-nested-write-moved",
        "Addr { name: string }\nUser { addr: Addr }\nfn main() -> i32 {\n  mut us := [User{addr: Addr{name: \"a\".clone()}}]\n  s := \"b\".clone()\n  t := s\n  us[0].addr.name = s\n  return (t.len()) as i32\n}\n",
    ));
}

#[test]
fn whole_elem_assign_struct_value() {
    if !backend_available() {
        return;
    }
    // `arr[i] = structval` — the write counterpart of the `arr[i]` whole-element read: one element
    // is replaced (a single aggregate store), others untouched. arr[0]={7,8}, arr[1] stays {3,4} →
    // 7 + 8 + 3 = 18.
    let src = concat!(
        "P { a: i64, b: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut arr := [P{a: 1, b: 2}, P{a: 3, b: 4}]\n",
        "  arr[0] = P{a: 7, b: 8}\n",
        "  return (arr[0].a + arr[0].b + arr[1].a) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("whole-elem-assign", src).status.code(), Some(18));
}

#[test]
fn whole_elem_assign_from_struct_local() {
    if !backend_available() {
        return;
    }
    // The value can be any POD-struct expression, not just a literal: copy element 1 over element 0.
    let src = concat!(
        "P { a: i64, b: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut arr := [P{a: 1, b: 2}, P{a: 3, b: 4}]\n",
        "  u := arr[1]\n",
        "  arr[0] = u\n",
        "  return (arr[0].a + arr[0].b) as i32\n",
        "}\n",
    );
    assert_eq!(build_and_run("whole-elem-assign-var", src).status.code(), Some(7));
}

#[test]
fn whole_elem_assign_immutable_rejected() {
    // Replacing an element requires the array local to be `mut`.
    assert!(check_errs(
        "whole-elem-assign-immut",
        "P { a: i64, b: i64 }\nfn main() -> i32 {\n  arr := [P{a: 1, b: 2}]\n  arr[0] = P{a: 9, b: 9}\n  return 0\n}\n",
    ));
}

#[test]
fn whole_elem_assign_str_field_rejected() {
    // First cut: whole-element assignment is plain-old-data only — a `str` field (region-tied) is
    // a deferred case, rejected cleanly rather than silently storing a dangling view.
    assert!(check_errs(
        "whole-elem-assign-str",
        "Pt { x: i64, s: str }\nfn main() -> i32 {\n  mut arr := [Pt{x: 1, s: \"a\"}]\n  arr[0] = Pt{x: 2, s: \"b\"}\n  return 0\n}\n",
    ));
}

// --- Dynamic `array<Struct>` element-field write: `arr[i].field = v` on an owned `{ptr,len}` view
// (`DynStructArray`), lowered by the pointer-based `StoreElemFieldPtr` (the write dual of the
// `IndexFieldPtr` read). Scalar fields only; str/owned fields are gated off. ---

#[test]
fn dyn_elem_field_assign_all_widths() {
    if !backend_available() {
        return;
    }
    // A json-decoded `array<Rec>` (dynamic AoS) written field-by-field across every scalar width,
    // then read back. Also exercises `#307` field reordering: the physical layout is permuted, so a
    // correct write/read must go through the logical→physical `pfield` map. `h` is set true so its
    // `hv` term is 0; the integer fields sum to 9 + 99 + 999 + 9999 + 250 = 11356.
    let src = concat!(
        "import core.json\n",
        "Rec { a: i8, b: i16, c: i32, d: i64, e: u8, h: bool }\n",
        "fn main() -> Result<(), Error> {\n",
        "  mut rs: array<Rec> := json.decode(\"[{\\\"a\\\":1,\\\"b\\\":2,\\\"c\\\":3,\\\"d\\\":4,\\\"e\\\":5,\\\"h\\\":false}]\")?\n",
        "  rs[0].a = 9\n",
        "  rs[0].b = 99\n",
        "  rs[0].c = 999\n",
        "  rs[0].d = 9999\n",
        "  rs[0].e = 250\n",
        "  rs[0].h = true\n",
        "  hv := if rs[0].h { 0 } else { 1000 }\n",
        "  print((rs[0].a as i64) + (rs[0].b as i64) + (rs[0].c as i64) + rs[0].d + (rs[0].e as i64) + hv)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("dyn-elem-field-widths", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "11356\n");
}

#[test]
fn dyn_elem_field_assign_dynamic_index() {
    if !backend_available() {
        return;
    }
    // A runtime index (not a constant) into the dynamic-array element-field store; only the targeted
    // element is written. arr[1].y = 77 → read arr[0].y (still 2) + arr[1].y (77) = 79.
    let src = concat!(
        "import core.json\n",
        "Point { x: i64, y: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  mut pts: array<Point> := json.decode(\"[{\\\"x\\\":1,\\\"y\\\":2},{\\\"x\\\":3,\\\"y\\\":4}]\")?\n",
        "  mut i := 1\n",
        "  pts[i].y = 77\n",
        "  print(pts[0].y + pts[1].y)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("dyn-elem-field-dyn-index", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "79\n");
}

#[test]
fn dyn_elem_field_assign_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // The write is bounds-checked against the view's runtime length: an out-of-range index aborts
    // (never a clean exit), just like the scalar `arr[i] = v` store.
    let src = concat!(
        "import core.json\n",
        "Point { x: i64, y: i64 }\n",
        "fn main() -> Result<(), Error> {\n",
        "  mut pts: array<Point> := json.decode(\"[{\\\"x\\\":1,\\\"y\\\":2}]\")?\n",
        "  pts[5].x = 99\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("dyn-elem-field-oob", src);
    assert_ne!(out.status.code(), Some(0), "an out-of-bounds element-field store must abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("index out of bounds"),
        "expected a bounds-check abort, stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn dyn_elem_field_assign_immutable_rejected() {
    // Writing a field of a dynamic-array element requires the array local to be `mut`.
    assert!(check_errs(
        "dyn-elem-field-immut",
        "import core.json\nPoint { x: i64, y: i64 }\nfn main() -> Result<(), Error> {\n  pts: array<Point> := json.decode(\"[{\\\"x\\\":1,\\\"y\\\":2}]\")?\n  pts[0].x = 9\n  return Ok(())\n}\n",
    ));
}

#[test]
fn dyn_elem_field_assign_moved_array_rejected() {
    // Writing an element field is a use of the array; if it has been moved away, that is caught as a
    // use-after-move (the `whole_moved` check on the base local).
    assert!(check_errs(
        "dyn-elem-field-moved",
        "import core.json\nPoint { x: i64, y: i64 }\nfn take(p: array<Point>) -> i64 = p.len()\nfn main() -> Result<(), Error> {\n  mut pts: array<Point> := json.decode(\"[{\\\"x\\\":1,\\\"y\\\":2}]\")?\n  n := take(pts)\n  pts[0].x = 9\n  print(n)\n  return Ok(())\n}\n",
    ));
}

#[test]
fn dyn_elem_field_assign_str_field_rejected() {
    // The dynamic-array element-field write goes through a buffer pointer with no per-element drop
    // of the overwritten field, so a `str`/owned field is gated off with a clear diagnostic (a
    // scalar sibling field of the same struct still writes fine).
    let mut sm = SourceMap::new();
    let src = "import core.json\nUser { id: i64, name: str }\nfn main() -> Result<(), Error> {\n  mut us: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"name\\\":\\\"ann\\\"}]\")?\n  us[0].name = \"bob\"\n  return Ok(())\n}\n";
    let checked = check(&mut sm, "dyn-elem-field-str", src);
    assert!(checked.diags.has_errors(), "a str element-field write must be rejected");
    let msg = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(
        msg.contains("primitive fields only for now"),
        "expected the deferred-str diagnostic, got:\n{msg}"
    );
}
