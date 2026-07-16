//! `chunks(n)` — split an array/slice of a primitive scalar into length-`n` sub-slices, yielding
//! an owned `array<slice<T>>` (the unit of chunk parallelism, `draft.md` §11). Each chunk is a
//! `slice<T>` borrowing the source; the chunk header buffer is freed at scope exit.


mod common;
use common::*;

#[test]
fn chunks_count_and_per_chunk_sum() {
    if !backend_available() {
        return;
    }
    // [1..5].chunks(2) → 3 chunks: [1,2], [3,4], [5]. count=3, sums 3/7/5.
    let src = "fn main() -> Result<(), Error> {\n  xs := [1, 2, 3, 4, 5]\n  cs := xs.chunks(2)\n  print(cs.len())\n  print(cs[0].sum())\n  print(cs[1].sum())\n  print(cs[2].sum())\n  return Ok(())\n}\n";
    let out = build_and_run("ch-basic", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n3\n7\n5\n");
}

#[test]
fn chunks_exact_division() {
    if !backend_available() {
        return;
    }
    // 6 elements / 3 → 2 chunks of 3: [10,20,30]=60, [40,50,60]=150.
    let src = "fn main() -> Result<(), Error> {\n  xs := [10, 20, 30, 40, 50, 60]\n  cs := xs.chunks(3)\n  print(cs.len())\n  print(cs[0].sum())\n  print(cs[1].sum())\n  return Ok(())\n}\n";
    let out = build_and_run("ch-exact", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n60\n150\n");
}

#[test]
fn chunks_each_chunk_len() {
    if !backend_available() {
        return;
    }
    // The last chunk is shorter: chunks(2) of 5 elements → lens 2, 2, 1.
    let src = "fn main() -> Result<(), Error> {\n  cs := [1, 2, 3, 4, 5].chunks(2)\n  print(cs[0].len())\n  print(cs[2].len())\n  return Ok(())\n}\n";
    let out = build_and_run("ch-len", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n1\n");
}

#[test]
fn chunks_over_owned_array() {
    if !backend_available() {
        return;
    }
    // chunks over an owned `array<i64>` (from `to_array`). [2,4,6,8].chunks(2) → [2,4]=6, [6,8]=14.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  ys := [1, 2, 3, 4].map(dbl).to_array()\n  cs := ys.chunks(2)\n  print(cs[0].sum())\n  print(cs[1].sum())\n  return Ok(())\n}\n";
    let out = build_and_run("ch-owned", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n14\n");
}

#[test]
fn direct_chunks_consumers_are_semantically_equivalent() {
    if !backend_available() {
        return;
    }
    let src = "fn count(n: i64) -> i64 {\n  xs := [1, 2, 3, 4, 5]\n  return xs.chunks(n).len()\n}\nfn id(x: i64) -> i64 = x\nfn none(x: i64) -> bool = false\nfn main() -> Result<(), Error> {\n  xs := [1, 2, 3, 4, 5]\n  print(count(2))\n  print(xs.chunks(2)[0].sum())\n  print(xs.chunks(2)[2].sum())\n  print(count(0))\n  print(count(-2))\n  print([1, 2, 3].map(id).to_array().chunks(2)[0].sum())\n  print([1, 2].where(none).to_array().chunks(2).len())\n  return Ok(())\n}\n";
    let out = build_and_run("ch-direct", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n3\n5\n0\n0\n3\n0\n");
}

#[test]
fn direct_chunks_zero_size_index_aborts() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  return [1, 2, 3].chunks(0)[0].len() as i32\n}\n";
    let out = build_and_run("ch-direct-zero-index", src);
    assert_ne!(out.status.code(), Some(0), "indexing the canonical empty chunks result must abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("index out of bounds"),
        "expected an out-of-bounds message, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// --- diagnostics ---

#[test]
fn chunks_wrong_arg_count_errors() {
    assert!(check_errs("ch-arity", "fn main() -> i32 {\n  cs := [1, 2, 3].chunks()\n  return 0\n}\n"));
}

#[test]
fn chunks_over_struct_array_errors() {
    let src = "Emp { pay: i32 }\nfn main() -> i32 {\n  cs := [Emp{pay: 1}].chunks(1)\n  return 0\n}\n";
    assert!(check_errs("ch-struct", src));
}
