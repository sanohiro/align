//! `par_map(f)` — apply a Pure function to each element, materializing an owned `array<R>`
//! (`draft.md` §11). The Pure requirement is enforced by effect/purity inference. A direct source
//! lowers to a generated whole-range kernel scheduled across the process-resident worker pool;
//! Copy-capturing forms use the same range kernel through an immutable call-scoped context;
//! primitive-scalar/AoS length-preserving map/filter/projection stages are fused into that range
//! kernel; unsupported layouts such as SoA, chunks, and string-search retain the sequential
//! fallback. Move captures are rejected by ownership checks.


mod common;
use common::*;

#[test]
fn par_map_pure_function() {
    if !backend_available() {
        return;
    }
    // par_map a pure function over an array, then sum: 2 + 4 + 6 = 12.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  doubled := [1, 2, 3].par_map(dbl)\n  print(doubled.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-pure", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn par_map_capturing_lambda_uses_parallel_range_kernel() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  k := 10\n  ys := [1, 2, 3].par_map(fn x { x + k })\n  print(ys.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");

    let ir = emit_llvm(src);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parkernel")))
        .unwrap_or_else(|| panic!("no capturing par_map range kernel in IR:\n{ir}"));
    let kernel = kernel.split_once("\n}\n").map_or(kernel, |(body, _)| body);
    assert!(
        kernel.lines().next().is_some_and(|line| {
            line.matches("ptr readonly captures(none)").count() >= 2
                && line.contains("ptr noalias writeonly captures(none)")
        }),
        "capture, input, and output pointer contracts must be present:\n{kernel}"
    );
    assert!(kernel.contains("load i64"), "the kernel must load the captured value from its context:\n{kernel}");
    assert!(
        kernel.contains("call i64 @\"main$lambda") && kernel.contains(", i64 %parcapv"),
        "the direct body call must receive the capture value:\n{kernel}"
    );

    // Cross the runtime's range threshold too: the context must remain valid when the runtime
    // takes the pool-eligible path, not only on the caller-only small-input path. The IR
    // assertions above pin the generated range kernel; this run pins context correctness for
    // both paths. A one-worker host legitimately executes the single range on the caller; the
    // runtime's forced-multi-worker nested-par_map test covers helper-worker scheduling.
    let large_src = "fn main() -> Result<(), Error> {\n  mut b: array_builder<i64> := array_builder()\n  mut i := 0\n  loop {\n    b.push(i)\n    i = i + 1\n    if i >= 65537 { break }\n  }\n  xs := b.build()\n  k := 10\n  ys := xs.par_map(fn x { x + k })\n  print(ys[0])\n  print(ys[65536])\n  return Ok(())\n}\n";
    let large = build_and_run("pm-capture-large", large_src);
    assert_eq!(large.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&large.stdout), "10\n65546\n");
}

#[test]
fn par_map_copy_array_capture_uses_the_context_abi() {
    if !backend_available() {
        return;
    }
    // Fixed arrays are Copy captures even though they are not range-element layouts. The capture
    // context must preserve the whole by-value array rather than applying the narrower element gate.
    let src = "fn main() -> Result<(), Error> {\n  offsets := [4, 5]\n  ys := [1, 2, 3].par_map(fn x { x + offsets[0] })\n  print(ys.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-copy-array-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
    assert!(emit_llvm(src).contains("$parkernel"), "a Copy array capture should stay on the range-kernel path");
}

#[test]
fn par_map_borrowed_call_source_is_not_freed() {
    if !backend_available() {
        return;
    }
    // A function call returning `slice<T>` lends the caller's storage. The parallel path must not
    // classify every call expression as an owned temporary and free the stack-array pointer.
    let src = "fn whole(xs: slice<i64>) -> slice<i64> = xs\nfn twice(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  a := [1, 2, 3]\n  out := whole(a).par_map(twice)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-borrowed-call", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(!text.contains("drop_value"), "the borrowed par_map source must not be freed:\n{text}");
}

#[test]
fn par_map_inside_arena_frees_runtime_output() {
    if !backend_available() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  arena {\n    ys := [1, 2, 3].par_map(dbl)\n    print(ys.sum())\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("pm-arena-output", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-arena-output", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.lines().any(|line| line.trim_start().starts_with("drop ")), "the malloc-backed par_map output must be dropped inside the arena:\n{text}");

    let ir = emit_llvm(src);
    assert!(ir.lines().any(|line| line.contains("call void @align_rt_free(")), "the par_map output must be freed through the runtime:\n{ir}");
}

#[test]
fn par_map_after_where() {
    if !backend_available() {
        return;
    }
    // Stable compaction preserves source order: keep >2, then *10 → [30, 40, 50].
    let src = "fn big(x: i64) -> bool = x > 2\nfn dec(x: i64) -> i64 = x * 10\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3, 4, 5].where(big).par_map(dec)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-where", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");

    let src = "fn big(x: i64) -> bool = x > 2\nfn dec(x: i64) -> i64 = x * 10\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3, 4, 5].where(big).par_map(dec)\n  print(out.len())\n  print(out[0])\n  print(out[2])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-where-order", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n30\n50\n");
}

#[test]
fn par_map_over_struct_field() {
    if !backend_available() {
        return;
    }
    // par_map a struct-consuming pure function (multi-field) → array<i32>; sum = (10+5)+(20+7)=42.
    let src = "Emp { base: i32, bonus: i32 }\nfn net(e: Emp) -> i32 = e.base + e.bonus\nfn main() -> Result<(), Error> {\n  ns := [Emp{base: 10, bonus: 5}, Emp{base: 20, bonus: 7}].par_map(net)\n  print(ns.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-struct", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");

    let ir = emit_llvm(src);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parkernel")))
        .unwrap_or_else(|| panic!("an AoS struct source should use the parallel range kernel:\n{ir}"));
    assert!(kernel.contains("call i32 @net") || kernel.contains("call i32 @\"net\""), "the kernel must call the struct-consuming body directly:\n{kernel}");
}

#[test]
fn par_map_over_dynamic_struct_array_uses_aos_stride_kernel() {
    if !backend_available() {
        return;
    }
    let src = "Emp { base: i32, bonus: i32 }\nfn net(e: Emp) -> i32 = e.base + e.bonus\nfn main() -> Result<(), Error> {\n  xs: array<Emp> := [Emp{base: 10, bonus: 5}, Emp{base: 20, bonus: 7}].to_array()\n  ys := xs.par_map(net)\n  print(ys.len())\n  print(ys[0])\n  print(ys[1])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-dynamic-struct", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n15\n27\n");

    let ir = emit_llvm(src);
    assert!(ir.contains("$parkernel"), "a dynamic AoS struct source should use the parallel range kernel:\n{ir}");
}

#[test]
fn par_map_over_padded_aos_uses_abi_stride() {
    if !backend_available() {
        return;
    }
    let src = "Row { active: bool, amount: i64 }\nfn amount(row: Row) -> i64 = row.amount\nfn main() -> Result<(), Error> {\n  rows: array<Row> := [Row{active: true, amount: 3}, Row{active: false, amount: 7}].to_array()\n  out := rows.par_map(amount)\n  print(out[0])\n  print(out[1])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-padded-aos", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n7\n");

    let ir = emit_llvm(src);
    let call = ir
        .lines()
        .find(|line| line.contains("@align_rt_par_map("))
        .unwrap_or_else(|| panic!("no par_map runtime call in IR:\n{ir}"));
    assert!(call.contains(", i64 16, i64 8,"), "the padded AoS input must use its ABI stride:\n{call}");
    assert!(
        ir.contains("@llvm.umul.with.overflow.i64(i64 2, i64 16)"),
        "the padded AoS source buffer must multiply its count by the 16-byte ABI row size:\n{ir}"
    );
}

#[test]
fn par_map_round_trips_json_aos_with_abi_stride() {
    if !backend_available() {
        return;
    }
    // JSON AoS decode/encode and the range kernel must agree on the ABI allocation size. The
    // natural field order gives this row a tail-padded 16-byte stride (bool + i64), while the
    // logical descriptor still reports fields in source order.
    let src = "import core.json\nRow { active: bool, amount: i64 }\nBatch { rows: array<Row> }\nfn amount(row: Row) -> i64 = row.amount\nfn main() -> Result<(), Error> {\n  rows: array<Row> := json.decode(\"[{\\\"active\\\":true,\\\"amount\\\":3},{\\\"active\\\":false,\\\"amount\\\":7}]\")?\n  batch: Batch := json.decode(\"{\\\"rows\\\":[{\\\"active\\\":true,\\\"amount\\\":3},{\\\"active\\\":false,\\\"amount\\\":7}]}\")?\n  out := rows.par_map(amount)\n  print(out[0])\n  print(out[1])\n  print(json.encode(batch))\n  return Ok(())\n}\n";
    let out = build_and_run("pm-json-aos", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n7\n{\"rows\":[{\"active\":true,\"amount\":3},{\"active\":false,\"amount\":7}]}\n");
}

#[test]
fn par_map_rejects_move_aos_elements_before_codegen() {
    let src = "User { name: string, age: i32 }\nfn age(u: User) -> i32 = u.age\nfn main() -> i32 {\n  users := [User{name: \"a\".clone(), age: 7}]\n  out := users.par_map(age)\n  return out[0]\n}\n";
    assert!(check_errs("pm-move-struct", src), "par_map must reject a Move struct element by value");
    assert!(
        check_diagnostics("pm-move-struct", src).contains("cannot pass a Move element"),
        "the ownership diagnostic should explain why the AoS range ABI is unavailable"
    );
}

#[test]
fn par_map_rejects_owned_fixed_array_capture() {
    let src = "fn main() -> Result<(), Error> {\n  names := [\"a\".clone(), \"b\".clone()]\n  ys := [1, 2].par_map(fn x { x + names.len() })\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-array-string-capture", src), "par_map must reject an owned fixed array capture");
    assert!(
        check_diagnostics("pm-array-string-capture", src).contains("cannot capture the owned value 'names'"),
        "the ownership diagnostic should identify the fixed array capture"
    );
}

#[test]
fn par_map_rejects_region_bearing_result() {
    let src = "fn identity(x: str) -> str = x\nfn main() -> i32 {\n  xs := [\"a\"].par_map(identity)\n  return xs.len() as i32\n}\n";
    assert!(check_errs("pm-region-result", src), "par_map must reject a borrowed str result");
    assert!(
        check_diagnostics("pm-region-result", src).contains("'par_map' result must be a primitive scalar"),
        "the diagnostic should explain the non-owning primitive result contract"
    );
}

#[test]
fn par_map_after_struct_map_keeps_struct_abi_in_the_range_kernel() {
    if !backend_available() {
        return;
    }
    let src = "Emp { base: i32, bonus: i32 }\nfn net(e: Emp) -> i32 = e.base + e.bonus\nfn twice(x: i32) -> i32 = x * 2\nfn main() -> Result<(), Error> {\n  out := [Emp{base: 10, bonus: 5}, Emp{base: 20, bonus: 7}].map(net).par_map(twice)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-struct-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "84\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-struct-map", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map[net -> twice]"), "a struct map stage should stay in the parallel node:\n{text}");

    let ir = emit_llvm(src);
    assert!(ir.contains("call i32 @net") || ir.contains("call i32 @\"net\""), "the range kernel must call the aggregate map body directly:\n{ir}");
}

#[test]
fn par_map_after_struct_projection_uses_range_kernel() {
    if !backend_available() {
        return;
    }
    let src = "Emp { active: bool, base: i32 }\nfn twice(x: i32) -> i32 = x * 2\nfn main() -> Result<(), Error> {\n  out := [Emp{active: true, base: 10}, Emp{active: false, base: 50}, Emp{active: true, base: 11}].base.par_map(twice)\n  print(out.len())\n  print(out[0])\n  print(out[2])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-struct-project", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n20\n22\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-struct-project", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map[field#1 -> twice]"), "the projection and terminal should share one parallel MIR node:\n{text}");

    let ir = emit_llvm(src);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parmapchain$")))
        .unwrap_or_else(|| panic!("no projected par_map range kernel in IR:\n{ir}"));
    assert!(kernel.contains("extractvalue"), "the projected kernel must extract the AoS field in the range loop:\n{kernel}");
    assert!(kernel.contains("call i32 @twice") || kernel.contains("call i32 @\"twice\""), "the projected kernel must call the terminal body directly:\n{kernel}");
}

#[test]
fn par_map_after_struct_field_filter_uses_stable_compaction() {
    if !backend_available() {
        return;
    }
    let src = "Emp { active: bool, base: i32 }\nfn twice(x: i32) -> i32 = x * 2\nfn main() -> Result<(), Error> {\n  out := [Emp{active: true, base: 10}, Emp{active: false, base: 50}, Emp{active: true, base: 11}].where(.active).base.par_map(twice)\n  print(out.len())\n  print(out[0])\n  print(out[1])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-struct-filter-project", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n20\n22\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-struct-filter-project", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map[where field#0 -> field#1 -> twice]"), "field filtering and projection should remain one ordered parallel node:\n{text}");

    let ir = emit_llvm(src);
    assert!(ir.contains("$parfilter$count$wherefield$0$field$1"), "the field-filter count kernel must be generated:\n{ir}");
    assert!(ir.contains("$parfilter$scatter$wherefield$0$field$1"), "the field-filter scatter kernel must be generated:\n{ir}");
}

#[test]
fn par_map_chained_into_reduction_fuses_intermediate() {
    if !backend_available() {
        return;
    }
    // A directly consumed integer `par_map(f).sum()` uses one partial per range instead of a full
    // transformed array followed by a serial reread. 2 + 4 + 6 = 12.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  print([1, 2, 3].par_map(dbl).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chain", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map_reduce[dbl]"), "the direct chain must use the fused reduction:\n{text}");
    assert!(!text.contains("par_map[dbl]"), "the fused chain must not materialize the map result:\n{text}");
}

#[test]
fn par_map_reduction_preserves_integer_wrap() {
    if !backend_available() {
        return;
    }
    let src = "fn keep_i8(x: i8) -> i8 = x\nfn keep_u8(x: u8) -> u8 = x\nfn main() -> Result<(), Error> {\n  print([127, 1].par_map(keep_i8).sum())\n  print([200, 100].par_map(keep_u8).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-reduce-wrap", src);
    assert_eq!(out.status.code(), Some(0));
    // 128 in i8 is -128; 300 in u8 is 44. Both folds are modulo the result width.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-128\n44\n");
}

#[test]
fn par_map_reduction_captures_copy_context() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  k := 10\n  print([1, 2, 3].par_map(fn x { x + k }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-reduce-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");
}

#[test]
fn par_map_reduction_empty_input_returns_zero() {
    if !backend_available() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  mut b: array_builder<i64> := array_builder()\n  xs := b.build()\n  print(xs.par_map(dbl).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-reduce-empty", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

#[test]
fn par_map_reduction_range_kernel_writes_partials() {
    if !backend_available() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\npub fn run(xs: slice<i64>) -> i64 = xs.par_map(dbl).sum()\nfn main() -> i32 = 0\n";
    let ir = emit_llvm(src);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parreducekernel")))
        .unwrap_or_else(|| panic!("no par_map reduction range kernel in IR:\n{ir}"));
    let kernel = kernel.split_once("\n}\n").map_or(kernel, |(body, _)| body);
    assert!(kernel.contains("phi i64"), "the reduction kernel needs a counted loop and an accumulator:\n{kernel}");
    assert!(kernel.contains("call i64 @dbl(i64"), "the reduction kernel must call the body directly:\n{kernel}");
    assert!(kernel.contains("add i64"), "the reduction kernel must use plain wrapping integer addition:\n{kernel}");
    assert!(kernel.contains("store i64"), "the reduction kernel must publish one partial:\n{kernel}");
}

#[test]
fn par_map_runtime_call_carries_static_body_work_weight() {
    if !backend_available() {
        return;
    }
    let src = r#"fn cheap(x: i64) -> i64 = x + 1
fn heavy(x: i64) -> i64 {
  mut a := x
  a = a * 2654435761
  a = a * a + 7
  a = a * 40503 - 99
  return a
}
pub fn cheap_sum(xs: slice<i64>) -> i64 = xs.par_map(cheap).sum()
pub fn heavy_sum(xs: slice<i64>) -> i64 = xs.par_map(heavy).sum()
fn main() -> i32 = 0
"#;
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-work-weight", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map_reduce[cheap](") && text.contains("work=1"), "cheap map should carry weight 1:\n{text}");
    assert!(text.contains("par_map_reduce[heavy](") && text.contains("work=2"), "heavier map should carry weight 2:\n{text}");

    let ir = emit_llvm(src);
    let calls: Vec<&str> = ir.lines().filter(|line| line.contains("@align_rt_par_map_reduce(")).collect();
    assert!(calls.iter().any(|line| line.contains("i64 1, ptr")), "cheap reduction call must carry weight 1:\n{ir}");
    assert!(calls.iter().any(|line| line.contains("i64 2, ptr")), "heavy reduction call must carry weight 2:\n{ir}");
}

#[test]
fn chunks_par_map_chunk_function() {
    if !backend_available() {
        return;
    }
    // `chunks(n).par_map(f)` remains correct through the sequential collection fallback while
    // the dedicated chunk-parallel algorithm is out of scope for the AoS range kernel.
    // [1..5].chunks(2) → [1,2],[3,4],[5]; chunk_sum → [3, 7, 5].
    let src = "fn chunk_sum(c: slice<i64>) -> i64 = c.sum()\nfn main() -> Result<(), Error> {\n  sums := [1, 2, 3, 4, 5].chunks(2).par_map(chunk_sum)\n  print(sums.len())\n  print(sums[0])\n  print(sums[2])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chunks", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n3\n5\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-chunks", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(!text.contains("par_map["), "chunks must remain on the sequential fallback until its dedicated algorithm lands:\n{text}");
}

#[test]
fn chunks_par_map_then_reduce() {
    if !backend_available() {
        return;
    }
    // Chunk-parallel sums, then a final reduction over the per-chunk results: 3+7+11 = 21.
    let src = "fn chunk_sum(c: slice<i64>) -> i64 = c.sum()\nfn main() -> Result<(), Error> {\n  total := [1, 2, 3, 4, 5, 6].chunks(2).par_map(chunk_sum).sum()\n  print(total)\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chunks-reduce", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "21\n");
}

#[test]
fn chunks_par_map_inside_arena_frees_chunk_buffer() {
    if !backend_available() {
        return;
    }
    // Inside an `arena {}`, the `chunks` header buffer is heap-allocated (not arena), so it must
    // still be freed (`drop_value`) — the arena's bulk-free doesn't cover it. (1+2)+(3+4) = 10.
    let src = "fn chunk_sum(c: slice<i64>) -> i64 = c.sum()\nfn main() -> Result<(), Error> {\n  arena {\n    total := [1, 2, 3, 4].chunks(2).par_map(chunk_sum).sum()\n    print(total)\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chunks-arena", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
    // The always-heap chunks buffer is freed even inside the arena.
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("drop_value"), "the chunks buffer must be freed inside the arena:\n{text}");
}

#[test]
fn chunks_par_map_impure_rejected() {
    // The Pure requirement still applies to a chunk-consuming function.
    let src = "fn noisy(c: slice<i64>) -> i64 {\n  print(c.len())\n  return c.sum()\n}\nfn main() -> Result<(), Error> {\n  s := [1, 2].chunks(1).par_map(noisy)\n  print(s.len())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-chunks-impure", src));
}

#[test]
fn chunks_par_map_view_write_rejected() {
    // `out` is not part of the function-value type, so the Pure check must inspect the body rather
    // than relying on the callable signature. Writing a chunk's slice would race across ranges and
    // also contradict the generated kernel's readonly context/input contract.
    let src = "fn touches(out c: slice<i64>) -> i64 {\n  c[0] = 99\n  return c.len()\n}\nfn main() -> Result<(), Error> {\n  s := [1, 2].chunks(1).par_map(touches)\n  print(s.len())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-chunks-view-write", src));
}

#[test]
fn chunks_par_map_bulk_view_writes_rejected() {
    let cases = [
        (
            "vec-store",
            "fn touches(out c: slice<i64>) -> i64 {\n  v: vec2<i64> := [1, 2]\n  c.store(0, v)\n  return c[0]\n}\nfn main() -> Result<(), Error> {\n  s := [1, 2].chunks(1).par_map(touches)\n  print(s.len())\n  return Ok(())\n}\n",
        ),
        (
            "map-into",
            "fn inc(x: i64) -> i64 = x + 1\nfn touches(out c: slice<i64>) -> i64 {\n  [1].map(inc).map_into(c)\n  return c[0]\n}\nfn main() -> Result<(), Error> {\n  s := [1, 2].chunks(1).par_map(touches)\n  print(s.len())\n  return Ok(())\n}\n",
        ),
    ];
    for (name, src) in cases {
        assert!(check_errs(&format!("pm-chunks-{name}"), src), "{name} must be impure");
    }
}

#[test]
fn par_map_takes_parallel_path_and_is_correct() {
    if !backend_available() {
        return;
    }
    // A direct (no prior stages) array source runs in parallel (runtime work-split). Correctness
    // across thread boundaries: dbl over [1..12] → sum 156, first 2, last 24.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  xs := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].par_map(dbl)\n  print(xs.sum())\n  print(xs[0])\n  print(xs[11])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-parallel", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "156\n2\n24\n");
    // The direct par_map lowers to the parallel runtime path (not the sequential collect loop).
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map["), "a direct par_map should take the parallel path:\n{text}");
}

#[test]
fn par_map_range_kernel_owns_the_direct_element_loop() {
    if !backend_available() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> i32 {\n  ys := [1, 2, 3].par_map(dbl)\n  return ys[0] as i32\n}\n";
    let ir = emit_llvm(src);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parkernel")))
        .unwrap_or_else(|| panic!("no par_map range kernel in IR:\n{ir}"));
    let kernel = kernel.split_once("\n}\n").map_or(kernel, |(body, _)| body);

    assert!(
        kernel.lines().next().is_some_and(|line| {
            line.contains("ptr readonly captures(none)")
                && line.contains("ptr noalias writeonly captures(none)")
                && line.contains("i64")
        }),
        "range kernel must pin its read/write pointer contracts:\n{kernel}"
    );
    assert!(kernel.contains("phi i64"), "range kernel needs one counted induction variable:\n{kernel}");
    assert!(kernel.contains("getelementptr inbounds i64"), "range kernel needs typed element GEPs:\n{kernel}");
    assert!(kernel.contains("call i64 @dbl(i64"), "the element loop must call its known body directly:\n{kernel}");
    assert!(
        !kernel.lines().any(|line| line.trim_start().starts_with("call ") && line.contains(" %")),
        "the element loop must not retain an indirect per-element callback:\n{kernel}"
    );
}

#[test]
fn cheap_par_map_range_kernel_vectorizes_after_specialization() {
    if !backend_available() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\npub fn run(xs: slice<i64>) -> array<i64> = xs.par_map(dbl)\nfn main() -> i32 = 0\n";
    let ir = emit_llvm_optimized(src, &["run"]);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parkernel")))
        .unwrap_or_else(|| panic!("no optimized par_map range kernel in IR:\n{ir}"));
    let kernel = kernel.split_once("\n}\n").map_or(kernel, |(body, _)| body);

    assert!(
        kernel.contains("vector.body") && kernel.contains("load <") && kernel.contains("store <"),
        "a cheap arithmetic range kernel should expose a vectorized loop to LLVM:\n{kernel}"
    );
    assert!(
        !kernel.contains("call i64 @dbl") && !kernel.lines().any(|line| line.trim_start().starts_with("call ") && line.contains(" %")),
        "the optimized hot loop must inline the body and contain no per-element call:\n{kernel}"
    );
}

#[test]
fn par_map_after_length_preserving_maps_uses_one_range_kernel() {
    if !backend_available() {
        return;
    }
    let src = "fn add_one(x: i64) -> i64 = x + 1\nfn triple(x: i64) -> i64 = x * 3\nfn finish(x: i64) -> i64 = x - 1\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3].map(add_one).map(triple).par_map(finish)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-staged-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "24\n");

    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "pm-staged-map", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map[add_one -> triple -> finish]"), "the map chain should be one parallel MIR node:\n{text}");

    let ir = emit_llvm(src);
    let kernel = ir
        .split("define ")
        .find(|part| part.lines().next().is_some_and(|line| line.contains("$parmapchain$")))
        .unwrap_or_else(|| panic!("no staged par_map range kernel in IR:\n{ir}"));
    let kernel = kernel.split_once("\n}\n").map_or(kernel, |(body, _)| body);
    for func in ["@add_one", "@triple", "@finish"] {
        assert!(kernel.contains(&format!("call i64 {func}")), "staged kernel must call {func} directly:\n{kernel}");
    }
}

#[test]
fn staged_par_map_captures_share_one_immutable_context() {
    if !backend_available() {
        return;
    }
    let src = "fn triple(x: i64) -> i64 = x * 3\nfn main() -> Result<(), Error> {\n  add := 1\n  bias := 2\n  out := [1, 2, 3].map(fn x { x + add }).map(triple).par_map(fn x { x + bias })\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-staged-captures", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "33\n");
}

#[test]
fn par_map_after_multiple_filters_uses_one_stable_parallel_node() {
    if !backend_available() {
        return;
    }
    // Both filters run in source order, and only the survivors reach the terminal map: [2, 4] → [6, 12].
    let src = "fn positive(x: i64) -> bool = x > 0\nfn even(x: i64) -> bool = x % 2 == 0\nfn dec(x: i64) -> i64 = x * 3\nfn main() -> Result<(), Error> {\n  out := [-2, -1, 0, 1, 2, 3, 4].where(positive).where(even).par_map(dec)\n  print(out.sum())\n  print(out.len())\n  print(out[0])\n  print(out[1])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-multi-where", src);
    assert_eq!(out.status.code(), Some(0), "stdout={} stderr={}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n2\n6\n12\n");
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("par_map[where positive -> where even -> dec]"), "filters should use one parallel MIR node:\n{text}");

    let ir = emit_llvm(src);
    assert!(ir.contains("$parfilter$count$"), "filter count kernel should be emitted:\n{ir}");
    assert!(ir.contains("$parfilter$scatter$"), "filter scatter kernel should be emitted:\n{ir}");
}

#[test]
fn impure_where_stage_rejected_before_parallel_widening() {
    let src = "fn noisy(x: i64) -> bool {\n  print(x)\n  return x > 0\n}\nfn finish(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  ys := [1, 2].where(noisy).par_map(finish)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-where-impure", src));
}

#[test]
fn par_map_rejects_mutable_slice_alias_in_where() {
    // The slice descriptor is Copy, so a lambda can capture an alias of mutable array storage. A
    // local `mut` rebind of that descriptor must still be treated as a caller-view write, not as
    // private scalar state; the widened where count/scatter kernels would otherwise race and rerun
    // the store.
    let src = "fn main() -> Result<(), Error> {\n  mut xs := [1, 2, 3, 4]\n  view: slice<i64> := xs\n  ys := xs.where(fn x {\n    mut v: slice<i64> := view\n    v[0] = x\n    return true\n  }).par_map(fn x { x })\n  print(ys.len())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-where-view-write", src));
}

#[test]
fn par_map_filter_empty_result_is_a_valid_owned_array() {
    if !backend_available() {
        return;
    }
    let src = "fn never(x: i64) -> bool = x < 0\nfn finish(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3].where(never).par_map(finish)\n  print(out.len())\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-where-empty", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n0\n");
}

#[test]
fn par_map_filter_captures_share_the_context_record() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  limit := 1\n  bias := 10\n  out := [0, 1, 2, 3].where(fn x { x > limit }).par_map(fn x { x + bias })\n  print(out.len())\n  print(out[0])\n  print(out[1])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-where-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n12\n13\n");
}

// --- purity (Pure requirement) ---

#[test]
fn impure_named_map_stage_rejected_before_parallel_widening() {
    let src = "fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn finish(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  ys := [1, 2].map(noisy).par_map(finish)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-staged-impure-named", src));
}

#[test]
fn impure_capturing_map_stage_rejected_before_parallel_widening() {
    let src = "fn main() -> Result<(), Error> {\n  k := 10\n  ys := [1, 2].map(fn x {\n    print(x)\n    return x + k\n  }).par_map(fn x { x * 2 })\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-staged-impure-capture", src));
}

#[test]
fn par_map_impure_function_rejected() {
    // A function that prints has a side effect — rejected by the Pure requirement.
    let src = "fn noisy(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(noisy)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-impure", src));
}

#[test]
fn par_map_transitively_impure_rejected() {
    // Purity is transitive: `mid` calls `leaf` which prints, so `mid` is impure too.
    let src = "fn leaf(x: i64) -> i64 {\n  print(x)\n  return x\n}\nfn mid(x: i64) -> i64 = leaf(x) + 1\nfn main() -> Result<(), Error> {\n  ys := [1, 2].par_map(mid)\n  print(ys.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("pm-trans", src));
}

#[test]
fn par_map_calling_pure_helper_ok() {
    if !backend_available() {
        return;
    }
    // A pure function that calls another pure function is still Pure — accepted.
    let src = "fn inc(x: i64) -> i64 = x + 1\nfn step(x: i64) -> i64 = inc(x) * 2\nfn main() -> Result<(), Error> {\n  ys := [1, 2, 3].par_map(step)\n  print(ys.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-purehelper", src);
    assert_eq!(out.status.code(), Some(0));
    // (1+1)*2 + (2+1)*2 + (3+1)*2 = 4 + 6 + 8 = 18
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
}
