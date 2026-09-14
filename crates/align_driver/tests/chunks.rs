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

#[test]
fn index_receivers_evaluate_before_indices() {
    if !backend_available() {
        return;
    }
    let src = "fn source() -> array<i64> {\n  print(1)\n  return [1, 2, 3].to_array()\n}\nfn width() -> i64 {\n  print(2)\n  return 2\n}\nfn chunk_index() -> i64 {\n  print(3)\n  return 0\n}\nfn scalar_source() -> array<i64> {\n  print(4)\n  return [9].to_array()\n}\nfn scalar_index() -> i64 {\n  print(5)\n  return 0\n}\nfn main() -> Result<(), Error> {\n  print(source().chunks(width())[chunk_index()].sum())\n  print(scalar_source()[scalar_index()])\n  return Ok(())\n}\n";
    let out = build_and_run("ch-eval-order", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n3\n3\n4\n5\n9\n");
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

/// `scalar_to_prim` admits owned `string` (the one Move `PrimScalar`), so `chunks` accepted an
/// `array<string>` while the MIR boundary's `scalar_copy_ok` refused it — the chunk views would
/// alias elements the source still owns and deep-drops. That gap was an internal error after a
/// clean `check`, not a diagnostic.
///
/// Both reachable sources are covered: a built `array<string>`, and a `slice<string>` parameter —
/// a type that is declarable and passable even though no expression can build such a value.
/// `align_mir`'s `move_copy_positions_are_refused_by_the_producer_not_the_boundary` owns the
/// build-stops-here property.
#[test]
fn chunks_over_a_move_element_array_is_diagnosed() {
    for (label, src) in [
        (
            "built-array",
            "fn main() -> Result<(), Error> {\n  mut b: array_builder<string> := array_builder()\n  b.push(\"a\".clone())\n  built := b.build()\n  print(built.chunks(2).len())\n  return Ok(())\n}\n",
        ),
        (
            "slice-param",
            "fn f(xs: slice<string>) -> i64 = xs.chunks(2).len()\nfn main() -> i32 = 0\n",
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("ch-move-elem-{label}"), src);
        assert!(
            diagnostics.contains("'chunks' cannot view a Move element"),
            "a Move-element source must be diagnosed ({label}):\n{diagnostics}",
        );
    }
}

#[test]
fn virtual_sequential_consumer_matrix() {
    if !backend_available() { return; }
    let functions = r#"
fn source() -> array<i64> { print(1); return [1, 2, 3, 4, 5].to_array() }
fn width() -> i64 { print(2); return 2 }
fn size_generic<T>(xs: slice<T>) -> i64 = xs.len()
fn size(xs: slice<i64>) -> i64 = size_generic(xs)
fn sum(xs: slice<i64>) -> i64 = xs.sum()
fn positive(x: i64) -> bool { print(x); return x > 0 }
fn add(a: i64, b: i64) -> i64 = a + b
fn reduce(xs: slice<i64>, n: i64) -> i64 = xs.chunks(n).map(sum).sum()
"#;
    let main = r#"
fn main() -> Result<(), Error> {
  xs := [1, 2, 3, 4, 5]
  print(source().chunks(width()).map(sum).sum())
  print(reduce(xs, 0))
  print(reduce(xs, -1))
  print(reduce(xs, 9223372036854775807))
  print(xs.chunks(2).map(size).any(positive))
  print(xs.chunks(2).map(size).all(positive))
  print(xs.chunks(2).map(sum).to_array().sum())
  print(xs.chunks(2).map(sum).scan(0, add).sum())
  print(xs.chunks(2).map(sum).min())
  print(xs.chunks(2).map(sum).max())
  print(xs.chunks(2).map(sum).reduce(0, add))
  print(xs.chunks(2).map(sum).sort()[0])
  (large, small) := xs.chunks(2).map(sum).partition(fn value { value > 4 })
  print(large.sum())
  print(small.sum())
  return Ok(())
}
"#;
    let src = format!("{functions}{main}");
    let ir = emit_llvm(&src);
    assert!(!ir.contains("call ptr @align_rt_chunks"), "{ir}");
    assert!(!ir.lines().any(|line| line.contains("call ") && line.contains("@align_rt_chunks")), "{ir}");
    let out = build_and_run("virtual-sequential", &src);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n15\n0\n0\n15\n2\n2\n1\ntrue\n2\n2\n1\ntrue\n15\n28\n3\n7\n15\n3\n12\n3\n");
    let library = format!("module chunks_lib\n{}", functions.replace("fn ", "pub fn "));
    let caller = "import chunks_lib\nfn main() -> Result<(), Error> { print(chunks_lib.reduce([1, 2, 3, 4, 5], 2)); return Ok(()) }\n";
    let unit = build_per_unit_multi("virtual-chunks-units", &[("chunks_lib.align", &library), ("main.align", caller)], "main.align");
    let out = unit.link_and_run();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n");
}

#[test]
fn virtual_chunks_binary_tail_matrix() {
    if !backend_available() { return; }
    for (label, terminal, succeeds) in [
        ("sum", ".map(fn word { word.u32_le(0) }).sum()", false),
        ("any", ".any(fn word { word.u32_le(0) > 0 })", false),
        ("all", ".all(fn word { word.u32_le(0) == 0 })", false),
        ("filtered", ".where(fn word { word.len() == 4 }).map(fn word { word.u32_le(0) }).sum()", true),
        ("conditional", ".any(fn word { word.len() == 4 && word.u32_le(0) > 0 })", true),
    ] {
        let src = format!("fn main() -> Result<(), Error> {{ mut b := buffer(0); b.put_u32_le(1); b.put_u8(9); print(b.bytes().chunks(4){terminal}); return Ok(()) }}\n");
        let ir = emit_llvm(&src);
        assert!(!ir.lines().any(|line| line.contains("call ") && line.contains("@align_rt_chunks")), "{ir}");
        let out = build_and_run(&format!("virtual-tail-{label}"), &src);
        assert_eq!(out.status.success(), succeeds, "{label}: {}", String::from_utf8_lossy(&out.stderr));
        if !succeeds {
            assert!(String::from_utf8_lossy(&out.stderr).contains("out of bounds"));
        }
    }
}
