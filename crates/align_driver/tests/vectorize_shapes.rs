//! The vectorization IR-shape suite — the LLVM-upgrade gate (`docs/impl/09-explain-opt.md`,
//! Slice 3a). Each test compiles a data-path kernel, runs the `-O2` middle-end (`--stage optimized`
//! / `emit_llvm_ir(.., optimized = true)`), and asserts **textually** on the optimized LLVM IR that
//! the loop vectorizer did (or, for the negative controls, did not) fire. Assertions have teeth:
//! they pin presence AND absence, so a future LLVM upgrade that silently stops vectorizing a hot
//! pipeline — or spuriously "vectorizes" a loop-carried one — fails loudly here.
//!
//! Target is pinned to `x86-64-v3` (AVX2 → 256-bit → `<4 x i64>`) for stable widths; two kernels
//! also run at `x86-64-v2` (SSE → 128-bit → `<2 x i64>`) to pin that widths track the target. All
//! tests are gated to x86-64 hosts (the pinned CPU names are x86 tiers) and to backend availability.
//!
//! The `<N x ...>` widths, `vector.body`, and `llvm.vector.reduce.*` names are LLVM-19 IR spellings;
//! re-verify them at the LLVM upgrade — that re-verification is exactly what this gate exists for.
//!
//! Empirical verdicts (probed 2026-07-11, LLVM 19, this codegen) are recorded per-test below.
//! Divergences from the design table in `09-explain-opt.md` (kernels 4, 7) are noted in place — the
//! suite pins **reality**, not the table's pre-implementation guesses (the design's own rule).

mod common;
use common::*;

/// The kernels feed on a runtime-length prefix of a fixed array (`a[0..args.len()]`) so the trip
/// count is unknown and the data is opaque to the optimizer — otherwise a fixed literal is
/// constant-folded away and no loop survives to vectorize. `main(args)` must return
/// `Result<(), Error>`, and the pipeline result is kept live with `print`.
fn compile_ir(name: &str, src: &str, cpu: &str, optimized: bool) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "kernel `{name}` failed to compile:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    emit_llvm_ir(&mir, BuildTarget::Cpu(cpu.to_string()), optimized).expect("emit llvm ir")
}

/// Optimized IR (the `-O2` lens — "what LLVM did").
fn opt_ir(name: &str, src: &str, cpu: &str) -> String {
    compile_ir(name, src, cpu, true)
}

/// Raw IR (what codegen emitted, pre-optimization) — used only by the mutation checks.
fn raw_ir(name: &str, src: &str, cpu: &str) -> String {
    compile_ir(name, src, cpu, false)
}

/// The suite pins x86 CPU tiers, so it only runs on an x86-64 host with the LLVM backend present.
/// Elsewhere the requested CPU name would not match the host triple and the widths would differ.
fn x86_backend() -> bool {
    cfg!(target_arch = "x86_64") && backend_available()
}

const V3: &str = "x86-64-v3";
const V2: &str = "x86-64-v2";

// A reduction over a runtime-length prefix, kept live by `print`.
fn reduction_kernel(stage_and_terminal: &str, helpers: &str) -> String {
    format!(
        "{helpers}\nfn run(xs: slice<i64>) -> i64 = xs.{stage_and_terminal}\n\
         fn main(args: array<str>) -> Result<(), Error> {{\n  \
           a := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  \
           s : slice<i64> := a[0..args.len()]\n  \
           print(run(s))\n  \
           return Ok(())\n\
         }}\n"
    )
}

// --- Kernel 1: map(dbl).sum() — unknown trip count, int reduction. VECTORIZES. ★ locked ---

#[test]
fn k1_map_sum_vectorizes_v3() {
    if !x86_backend() {
        return;
    }
    let src = reduction_kernel("map(dbl).sum()", "fn dbl(x: i64) -> i64 = x * 2");
    let ir = opt_ir("k1", &src, V3);
    // AVX2: 256-bit int vectors = 4 lanes; a vectorized reduction loop with a horizontal add.
    assert!(ir.contains("<4 x i64>"), "want <4 x i64>:\n{ir}");
    assert!(ir.contains("vector.body"), "want a vectorized loop:\n{ir}");
    assert!(ir.contains("llvm.vector.reduce.add"), "want a horizontal add reduction:\n{ir}");
}

#[test]
fn k1_map_sum_width_tracks_target_v2() {
    if !x86_backend() {
        return;
    }
    // Same kernel at the SSE tier: 128-bit int vectors = 2 lanes. Pins that the vector width is the
    // target's, not a constant baked into the suite.
    let src = reduction_kernel("map(dbl).sum()", "fn dbl(x: i64) -> i64 = x * 2");
    let ir = opt_ir("k1v2", &src, V2);
    assert!(ir.contains("<2 x i64>"), "want <2 x i64> at v2:\n{ir}");
    assert!(!ir.contains("<4 x i64>"), "v2 must not use 4-lane vectors:\n{ir}");
    assert!(ir.contains("vector.body"), "want a vectorized loop:\n{ir}");
}

// --- Kernel 2: where(big).sum() — if-conversion, masked. VECTORIZES. ★ locked ---

#[test]
fn k2_where_sum_vectorizes_masked_v3() {
    if !x86_backend() {
        return;
    }
    let src = reduction_kernel("where(big).sum()", "fn big(x: i64) -> bool = x > 6");
    let ir = opt_ir("k2", &src, V3);
    // The `where` predicate is if-converted to a vector mask (`<4 x i1>`); the kept elements are
    // masked into the vectorized reduction.
    assert!(ir.contains("<4 x i1>"), "want a <4 x i1> predicate mask:\n{ir}");
    assert!(ir.contains("<4 x i64>"), "want <4 x i64>:\n{ir}");
    assert!(ir.contains("vector.body"), "want a vectorized loop:\n{ir}");
    assert!(ir.contains("llvm.vector.reduce.add"), "want a horizontal add reduction:\n{ir}");
}

#[test]
fn k2_where_sum_mask_width_tracks_target_v2() {
    if !x86_backend() {
        return;
    }
    let src = reduction_kernel("where(big).sum()", "fn big(x: i64) -> bool = x > 6");
    let ir = opt_ir("k2v2", &src, V2);
    assert!(ir.contains("<2 x i1>"), "want a <2 x i1> mask at v2:\n{ir}");
    assert!(ir.contains("<2 x i64>"), "want <2 x i64> at v2:\n{ir}");
    assert!(ir.contains("vector.body"), "want a vectorized loop:\n{ir}");
}

// --- Kernel 3: scan(0, add) — loop-carried dependency. NEGATIVE CONTROL. ★ locked ---

#[test]
fn k3_scan_does_not_vectorize() {
    if !x86_backend() {
        return;
    }
    // A prefix scan is loop-carried (element i depends on element i-1), so its loop cannot be
    // widened. The scan output is materialized in an arena and consumed by a runtime-index read (not
    // a reduction), so the scan loop is the *only* loop present — nothing else can supply a
    // `vector.body`.
    let src = "fn add(a: i64, x: i64) -> i64 = a + x\n\
         fn main(args: array<str>) -> Result<(), Error> {\n  \
           a := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  \
           n := args.len()\n  \
           s : slice<i64> := a[0..n]\n  \
           arena {\n    \
             ps := s.scan(0, add)\n    \
             print(ps[n - 1])\n  \
           }\n  \
           return Ok(())\n\
         }\n";
    let ir = opt_ir("k3", src, V3);
    // Teeth: no vectorized loop, and no horizontal reduction, formed anywhere. (A stray `<N x i64>`
    // from vector *stores* of the constant array initializer is unrelated to the loop and is NOT
    // asserted against — the load-bearing fact is that the loop-carried scan produced no
    // `vector.body`.)
    assert!(!ir.contains("vector.body"), "loop-carried scan must NOT vectorize:\n{ir}");
    assert!(
        !ir.contains("llvm.vector.reduce"),
        "no horizontal reduction should form over a scalar scan:\n{ir}"
    );
}

// --- Kernel 4: where(k).min() — masked min-reduction. VECTORIZES. ---
//
// Divergence from the design table: it named `<N x i32>`, but an `i32` element slice is not
// constructible today without a heap `array<i32>` (a fixed literal defaults to `i64`, and an
// `array<i32>` annotation on a literal is rejected — "materialize it with `.to_array()`"), so the
// masked-min-reduction claim is pinned over `i64`. The vectorizer and the `smin` reduction idiom
// are element-width-agnostic; the intent — a masked min-reduction widens — is what is verified.

#[test]
fn k4_where_min_vectorizes() {
    if !x86_backend() {
        return;
    }
    let src = reduction_kernel("where(k).min()", "fn k(x: i64) -> bool = x > 2");
    let ir = opt_ir("k4", &src, V3);
    assert!(ir.contains("<4 x i1>"), "want a <4 x i1> predicate mask:\n{ir}");
    assert!(ir.contains("<4 x i64>"), "want <4 x i64>:\n{ir}");
    assert!(ir.contains("vector.body"), "want a vectorized loop:\n{ir}");
    assert!(
        ir.contains("llvm.vector.reduce.smin"),
        "want a horizontal signed-min reduction:\n{ir}"
    );
}

// --- Kernel 5: map(f).reduce(1, mul) — user-combiner product reduction. VECTORIZES. ---

#[test]
fn k5_map_reduce_mul_vectorizes() {
    if !x86_backend() {
        return;
    }
    let src = reduction_kernel(
        "map(f).reduce(1, mul)",
        "fn f(x: i64) -> i64 = x + 1\nfn mul(a: i64, x: i64) -> i64 = a * x",
    );
    let ir = opt_ir("k5", &src, V3);
    assert!(ir.contains("<4 x i64>"), "want <4 x i64>:\n{ir}");
    assert!(ir.contains("vector.body"), "want a vectorized loop:\n{ir}");
    assert!(ir.contains("llvm.vector.reduce.mul"), "want a horizontal product reduction:\n{ir}");
}

// --- Kernel 6: float map(f).sum() — FP reassociation control. NEGATIVE CONTROL. ---

#[test]
fn k6_float_sum_does_not_vectorize_without_fast_math() {
    if !x86_backend() {
        return;
    }
    // Floating-point `+` is not associative, so an ordered FP sum reduction cannot be reassociated
    // into a vectorized horizontal add without `fast-math`/`reassoc` — which Align does not emit
    // (IEEE 754 semantics are preserved; `docs/open-questions.md` "floats never abort"). The FP
    // reduction therefore stays a scalar loop. (Confirms the design table's kernel-6 prediction.)
    let src = "fn f(x: f64) -> f64 = x * 2.0\n\
         fn run(xs: slice<f64>) -> f64 = xs.map(f).sum()\n\
         fn main(args: array<str>) -> Result<(), Error> {\n  \
           a := [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0]\n  \
           s : slice<f64> := a[0..args.len()]\n  \
           print(run(s))\n  \
           return Ok(())\n\
         }\n";
    let ir = opt_ir("k6", src, V3);
    assert!(
        !ir.contains("vector.body"),
        "an ordered FP sum must NOT vectorize without fast-math:\n{ir}"
    );
    assert!(
        !ir.contains("llvm.vector.reduce.fadd"),
        "no vectorized FP reduction should form:\n{ir}"
    );
}

// --- Kernel 7: src.map(dbl).map_into(dst) — two-slice materialize. VECTORIZES CLEANLY. ---
//
// Divergence from the design table: it predicted a scalar loop or a `vector.memcheck` runtime
// overlap guard "today (no `noalias`)", flipping to clean vectorization only when Slice 5 adds
// function-parameter `noalias`. Reality: the outcome is clean vectorization with NO
// `vector.memcheck`, confirmed below. `map_into` does emit scoped `!alias.scope`/`!noalias`
// metadata on the fused loop's source load and `dst` store (see `map_into.rs`
// `map_into_emits_scoped_noalias_metadata`), and that metadata is present in the raw IR and
// plausibly contributes — but the mechanism isn't isolated: at O2 `scale` fully inlines into
// `main`, where `s`/`d` trace back to distinct local allocas (`a`/`b`), and BasicAA can prove
// non-alias from that provenance alone, independent of the scoped metadata. The non-inlined case
// (metadata's contribution without the inlined-alloca shortcut) is untested. Either way, Slice 5's
// function-level `noalias` is not the unlock for this pattern — this loop already vectorizes
// cleanly without it.

#[test]
fn k7_map_into_vectorizes_without_memcheck() {
    if !x86_backend() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\n\
         fn scale(src: slice<i64>, out dst: slice<i64>) {\n  \
           src.map(dbl).map_into(dst)\n\
         }\n\
         fn main(args: array<str>) -> Result<(), Error> {\n  \
           a := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  \
           mut b := [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]\n  \
           n := args.len()\n  \
           s : slice<i64> := a[0..n]\n  \
           mut d : slice<i64> := b[0..n]\n  \
           scale(s, d)\n  \
           print(b[0])\n  \
           return Ok(())\n\
         }\n";
    let ir = opt_ir("k7", src, V3);
    assert!(ir.contains("vector.body"), "map_into should vectorize:\n{ir}");
    assert!(ir.contains("store <4 x i64>"), "want a vectorized store of the mapped elements:\n{ir}");
    assert!(
        !ir.contains("vector.memcheck"),
        "scoped noalias metadata should let the vectorizer drop the overlap guard:\n{ir}"
    );
}

// --- Kernel 8: map(dbl).to_array() — pointer-induction materialize. VECTORIZES. ---

#[test]
fn k8_to_array_materialize_vectorizes() {
    if !x86_backend() {
        return;
    }
    // `.to_array()` is an explicit heap materialization (the array is a real Move value that cannot
    // be elided even though only one element is later read), so the mapped store loop survives and
    // the vectorizer widens it to a `<4 x i64>` store.
    let src = "fn dbl(x: i64) -> i64 = x * 2\n\
         fn main(args: array<str>) -> Result<(), Error> {\n  \
           a := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  \
           n := args.len()\n  \
           s : slice<i64> := a[0..n]\n  \
           ys := s.map(dbl).to_array()\n  \
           print(ys[n - 1])\n  \
           return Ok(())\n\
         }\n";
    let ir = opt_ir("k8", src, V3);
    assert!(ir.contains("vector.body"), "the materialize loop should vectorize:\n{ir}");
    assert!(ir.contains("store <4 x i64>"), "want a vectorized store of the mapped elements:\n{ir}");
}

// --- Mutation checks: prove the suite reads OPTIMIZED IR, not the raw codegen output. ---
//
// Flip the stage to `raw`: the vector shapes the positive controls assert must all be ABSENT,
// because codegen emits scalar per-element loops and the loop vectorizer runs only in `-O2`. If
// these ever pass on raw IR, the suite is not testing what it claims to.

#[test]
fn mutation_k1_raw_is_not_vectorized() {
    if !x86_backend() {
        return;
    }
    let src = reduction_kernel("map(dbl).sum()", "fn dbl(x: i64) -> i64 = x * 2");
    let ir = raw_ir("k1raw", &src, V3);
    assert!(!ir.contains("vector.body"), "raw IR must have no vectorized loop:\n{ir}");
    assert!(!ir.contains("<4 x i64>"), "raw IR must have no 4-lane vectors:\n{ir}");
    assert!(!ir.contains("llvm.vector.reduce"), "raw IR must have no horizontal reduction:\n{ir}");
}

#[test]
fn mutation_k7_raw_is_not_vectorized() {
    if !x86_backend() {
        return;
    }
    let src = "fn dbl(x: i64) -> i64 = x * 2\n\
         fn scale(src: slice<i64>, out dst: slice<i64>) {\n  \
           src.map(dbl).map_into(dst)\n\
         }\n\
         fn main(args: array<str>) -> Result<(), Error> {\n  \
           a := [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n  \
           mut b := [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]\n  \
           n := args.len()\n  \
           s : slice<i64> := a[0..n]\n  \
           mut d : slice<i64> := b[0..n]\n  \
           scale(s, d)\n  \
           print(b[0])\n  \
           return Ok(())\n\
         }\n";
    let ir = raw_ir("k7raw", src, V3);
    assert!(!ir.contains("vector.body"), "raw IR must have no vectorized loop:\n{ir}");
    assert!(!ir.contains("store <4 x i64>"), "raw IR must have no vectorized store:\n{ir}");
}
