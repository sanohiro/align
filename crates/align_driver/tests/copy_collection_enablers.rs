//! Copy collection enablers used by pkg.web middleware: homogeneous function-value slices and
//! pipeline materialization of Copy structs.

mod common;
use common::*;

#[test]
fn function_value_array_coerces_to_slice_and_calls_in_order() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn add1(n: i64) -> i64 = n + 1\n",
        "fn twice(n: i64) -> i64 = n * 2\n",
        "fn apply(fs: slice<fn(i64) -> i64>, start: i64) -> i64 {\n",
        "  mut n := start\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    if i >= fs.len() { break n }\n",
        "    f := fs[i]\n",
        "    n = f(n)\n",
        "    i = i + 1\n",
        "  }\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  print(apply([add1, twice], 10))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("fn-array-slice", src);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "22\n");
}

#[test]
fn capturing_function_value_array_is_safe_for_a_downward_slice_call() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn apply(fs: slice<fn(i64) -> i64>, n: i64) -> i64 {\n",
        "  f := fs[0]\n",
        "  f(n)\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  k := 9\n",
        "  print(apply([fn n: i64 { n + k }], 4))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("capturing-fn-array-slice", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "13\n");
}

#[test]
fn map_to_array_materializes_copy_structs() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Pair { n: i64, label: str }\n",
        "fn make(n: i64) -> Pair = Pair { n: n * 3, label: \"ok\" }\n",
        "fn main() -> Result<(), Error> {\n",
        "  xs := [2, 4, 6].map(make).to_array()\n",
        "  p := xs[1]\n",
        "  print(p.n)\n",
        "  print(p.label)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("copy-struct-to-array", src);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\nok\n");
}

#[test]
fn temporary_owned_struct_array_borrows_as_a_slice_for_the_call() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Pair { n: i64 }\n",
        "fn make(n: i64) -> Pair = Pair { n: n * 2 }\n",
        "fn total(xs: slice<Pair>) -> i64 = xs.map(fn p { p.n }).sum()\n",
        "fn main() -> Result<(), Error> {\n",
        "  print(total([3, 4].map(make).to_array()))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("temporary-struct-array-slice", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "14\n");
}

#[test]
fn struct_value_array_cannot_hide_an_escaping_slice_field() {
    let src = concat!(
        "View { xs: slice<i64> }\n",
        "fn wrap(xs: slice<i64>) -> View = View { xs: xs }\n",
        "fn leak() -> View {\n",
        "  local := [1, 2]\n",
        "  views := [wrap(local)]\n",
        "  views[0]\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  v := leak()\n",
        "  print(v.xs[0])\n",
        "  return Ok(())\n",
        "}\n",
    );
    assert!(check_errs("struct-value-array-slice-escape", src));
}

#[test]
fn move_struct_to_array_is_rejected() {
    let src = concat!(
        "Owned { s: string }\n",
        "fn make(n: i64) -> Owned = Owned { s: \"x\".clone() }\n",
        "fn main() -> Result<(), Error> {\n",
        "  xs := [1, 2].map(make).to_array()\n",
        "  print(xs.len())\n",
        "  return Ok(())\n",
        "}\n",
    );
    assert!(check_errs("move-struct-to-array", src));
}
