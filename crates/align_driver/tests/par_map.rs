//! `par_map(f)` — apply a Pure function to each element, materializing an owned `array<R>`
//! (`draft.md` §11). The Pure requirement is enforced by effect/purity inference. A direct source
//! lowers to a generated whole-range kernel scheduled across the process-resident worker pool;
//! capturing forms use the same range kernel through an immutable call-scoped context; staged forms
//! retain the sequential fallback.


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

    // Cross the runtime's range threshold too: the context must remain live while pool workers
    // execute the generated kernel, not only on the caller-only small-input path.
    let large_src = "fn main() -> Result<(), Error> {\n  mut b: array_builder<i64> := array_builder()\n  mut i := 0\n  loop {\n    b.push(i)\n    i = i + 1\n    if i >= 65537 { break }\n  }\n  xs := b.build()\n  k := 10\n  ys := xs.par_map(fn x { x + k })\n  print(ys[0])\n  print(ys[65536])\n  return Ok(())\n}\n";
    let large = build_and_run("pm-capture-large", large_src);
    assert_eq!(large.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&large.stdout), "10\n65546\n");
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
fn par_map_after_where() {
    if !backend_available() {
        return;
    }
    // Stages before par_map compose: keep >2, then *10 → [30, 40, 50]; sum = 120.
    let src = "fn big(x: i64) -> bool = x > 2\nfn dec(x: i64) -> i64 = x * 10\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3, 4, 5].where(big).par_map(dec)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-where", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");
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
}

#[test]
fn par_map_chained_into_reduction_frees_intermediate() {
    if !backend_available() {
        return;
    }
    // `arr.par_map(f).sum()` — the par_map result is a fresh owned array consumed by `sum`; it
    // must be freed (`drop_value`), not leaked. 2 + 4 + 6 = 12.
    let src = "fn dbl(x: i64) -> i64 = x * 2\nfn main() -> Result<(), Error> {\n  print([1, 2, 3].par_map(dbl).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chain", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
    // The consumed intermediate buffer is freed (no leak).
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("drop_value"), "the par_map intermediate must be freed:\n{text}");
}

#[test]
fn chunks_par_map_chunk_function() {
    if !backend_available() {
        return;
    }
    // The §11 headline: `chunks(n).par_map(f)` where `f: (slice<T>) -> R` reduces each chunk.
    // [1..5].chunks(2) → [1,2],[3,4],[5]; chunk_sum → [3, 7, 5].
    let src = "fn chunk_sum(c: slice<i64>) -> i64 = c.sum()\nfn main() -> Result<(), Error> {\n  sums := [1, 2, 3, 4, 5].chunks(2).par_map(chunk_sum)\n  print(sums.len())\n  print(sums[0])\n  print(sums[2])\n  return Ok(())\n}\n";
    let out = build_and_run("pm-chunks", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n3\n5\n");
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
fn par_map_after_where_stays_sequential() {
    if !backend_available() {
        return;
    }
    // With a prior stage (`where`), par_map falls back to the sequential collect loop (a parallel
    // split can't see through the filter). Still correct: keep >2, *10 → [30,40,50], sum 120.
    let src = "fn big(x: i64) -> bool = x > 2\nfn dec(x: i64) -> i64 = x * 10\nfn main() -> Result<(), Error> {\n  out := [1, 2, 3, 4, 5].where(big).par_map(dec)\n  print(out.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("pm-seq", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(!text.contains("par_map["), "a staged par_map should stay sequential:\n{text}");
}

// --- purity (Pure requirement) ---

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
