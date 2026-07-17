//! doc-13 §8.4 (S3) — local constant-array pooling: correctness + IR-shape regression gates.
//!
//! A non-`mut`, non-`align(N)`, fixed `array<T>` local whose every element folds to a constant
//! scalar and whose length is at or above the measured cutoff (`CONST_POOL_MIN_ELEMS`, 32) is
//! initialized from a per-unit read-only global (the #514 rodata mechanism) with one memcpy — which
//! LLVM elides to a direct rodata read for a non-mutated binding — instead of `n` element stores.
//! The local KEEPS its fixed `array<T>` type (Copy), so no downstream use is re-typed or rejected;
//! only the initialization lowering changes.
//!
//! These gates pin: the pooled optimized shape (reads rodata, no alloca / no store chain); every
//! negative control (mut / align(N) / runtime element / owned `array<T>` binding / below-cutoff /
//! `str` element) keeping the per-element stores; the `ALIGN_CONST_POOL=off` toggle reverting the
//! shape; run-parity pooled vs unpooled through indexing and a materializing pipeline; the #506
//! donation non-interaction (a pooled source is a fixed `Ty::Array`, never a donatable owned
//! temporary — rodata is read, never freed); and the never-freed rodata (no drop of the table).

mod common;
use common::*;
use std::sync::Mutex;

/// Pooling is decided from the process-global `ALIGN_CONST_POOL` env var (read at sema), and cargo
/// runs a file's tests on parallel threads sharing that env. This lock serializes every
/// pool-sensitive test so an env-setting test never overlaps a default-reading one. Every
/// env-setting test unsets the var before releasing the guard, so a default reader always sees a
/// clean env.
static POOL_LOCK: Mutex<()> = Mutex::new(());
fn pool_guard() -> std::sync::MutexGuard<'static, ()> {
    POOL_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// An `n`-element i64 array literal `[0, 1, 2, ...]` (deterministic distinct values).
fn table(n: usize) -> String {
    let mut s = String::from("[");
    for i in 0..n {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&((i * 3 + 1) % 997).to_string());
    }
    s.push(']');
    s
}

/// Count of table-element `store i64 <imm>` instructions — the per-element init chain a pooled
/// binding must NOT emit. (Matches `store i64 <constant>, ...`; the pooled memcpy leaves none.)
fn imm_store_count(ir: &str) -> usize {
    ir.lines()
        .filter(|l| {
            l.trim_start()
                .strip_prefix("store i64 ")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit() || c == '-'))
        })
        .count()
}

// --- positive: the pooled shape ------------------------------------------------------------------

#[test]
fn pooled_large_table_reads_rodata_with_no_alloca_or_stores() {
    let _g = pool_guard();
    // A 64-element (>= cutoff) immutable local table read through a runtime index: after -O2 the
    // slot alloca and the memcpy are gone and the index reads the constant global directly.
    let src = format!(
        "fn lookup(i: i64) -> i64 {{ xs := {t}\n return xs[i] + xs[(i * 7 + 3) % 64] }}\n",
        t = table(64)
    );
    let ir = emit_llvm_optimized(&src, &["lookup"]);
    assert!(ir.contains("@const_arr"), "pooled read must reference the rodata global:\n{ir}");
    assert_eq!(imm_store_count(&ir), 0, "pooled binding must emit no per-element store chain:\n{ir}");
    assert!(!ir.contains("alloca [64 x i64]"), "the table alloca must be eliminated:\n{ir}");
}

#[test]
fn pooled_raw_shape_is_memcpy_not_store_chain() {
    let _g = pool_guard();
    // Pre-opt: the pooled binding lowers to a memcpy from the rodata global, not `n` stores.
    let src = format!("fn lookup(i: i64) -> i64 {{ xs := {t}\n return xs[i] }}\n", t = table(64));
    let ir = emit_llvm_with_exports(&src, &["lookup"]);
    assert!(ir.contains("@const_arr"), "raw pooled IR must build the rodata global:\n{ir}");
    assert!(ir.contains("memcpy"), "raw pooled IR must copy from the global:\n{ir}");
    // At most the two indexed reads' worth of stores — never a 64-store init chain.
    assert!(imm_store_count(&ir) < 8, "raw pooled IR must not emit the element store chain:\n{ir}");
}

// --- negative controls: each keeps the per-element store path (never pooled) ----------------------

fn assert_not_pooled(name: &str, src: &str) {
    let _g = pool_guard();
    let ir = emit_llvm_with_exports(src, &["f"]);
    assert!(!ir.contains("@const_arr"), "[{name}] must NOT pool (no rodata global):\n{ir}");
}

#[test]
fn mut_binding_not_pooled() {
    // A `mut` binding needs its own writable storage — it keeps the stores (and its own alloca).
    let src = format!("fn f(i: i64) -> i64 {{ mut xs := {t}\n xs[0] = 9\n return xs[i] }}\n", t = table(64));
    assert_not_pooled("mut", &src);
}

#[test]
fn align_binding_not_pooled() {
    // `align(N)` requests over-aligned stack storage for vector loads — not a rodata view.
    let src = format!("fn f(i: i64) -> i64 {{ align(64) xs := {t}\n return xs[i] }}\n", t = table(64));
    assert_not_pooled("align", &src);
}

#[test]
fn runtime_element_not_pooled() {
    // A runtime-valued element (`n + k`) does not fold to a constant, so the literal is not poolable.
    let mut elems = String::from("[");
    for i in 0..64 {
        if i > 0 {
            elems.push_str(", ");
        }
        elems.push_str(&format!("n + {i}"));
    }
    elems.push(']');
    let src = format!("fn f(i: i64, n: i64) -> i64 {{ xs := {elems}\n return xs[i] }}\n");
    assert_not_pooled("runtime-element", &src);
}

#[test]
fn literal_into_owned_consumer_not_pooled() {
    // `[...].to_array()` moves the literal into an OWNED `array<T>` (heap, Move, freed on drop). The
    // literal is a temporary source to `.to_array()`, not a `let`-bound fixed array, so it is never
    // pooled (a Static rodata view can never be donated to something that frees it). This is the
    // "moved into an owned consumer" negative control.
    let src = format!("fn f(i: i64) -> i64 {{ ys := {t}.to_array()\n return ys[i] }}\n", t = table(64));
    assert_not_pooled("owned-consumer", &src);
}

#[test]
fn below_cutoff_table_not_pooled() {
    // 16 elements (< 32 cutoff): inline stores are at parity/faster, so pooling is off.
    let src = format!("fn f(i: i64) -> i64 {{ xs := {t}\n return xs[i] }}\n", t = table(16));
    assert_not_pooled("below-cutoff", &src);
}

#[test]
fn str_element_table_not_pooled() {
    // A `str`-element fixed array is excluded from v1 pooling (its rodata is `[N x {ptr,len}]`).
    let mut elems = String::from("[");
    for i in 0..64 {
        if i > 0 {
            elems.push_str(", ");
        }
        elems.push_str(&format!("\"s{i}\""));
    }
    elems.push(']');
    let src = format!("fn f(i: i64) -> str {{ xs := {elems}\n return xs[i] }}\n");
    assert_not_pooled("str-element", &src);
}

// --- toggle -------------------------------------------------------------------------------------

#[test]
fn toggle_off_reverts_pooled_shape_to_stores() {
    let _g = pool_guard();
    let src = format!("fn f(i: i64) -> i64 {{ xs := {t}\n return xs[i] }}\n", t = table(64));
    // Default (pooled).
    assert!(emit_llvm_with_exports(&src, &["f"]).contains("@const_arr"));
    // Forced off: the toggle is read at sema (`check`), so set it around the emit. SAFETY: the test
    // binary runs its tests on separate processes per file section; this one sets+unsets serially.
    let off = {
        unsafe { std::env::set_var("ALIGN_CONST_POOL", "off") };
        let ir = emit_llvm_with_exports(&src, &["f"]);
        unsafe { std::env::remove_var("ALIGN_CONST_POOL") };
        ir
    };
    assert!(!off.contains("@const_arr"), "ALIGN_CONST_POOL=off must not pool:\n{off}");
    assert!(imm_store_count(&off) >= 32, "off build must keep the element store chain:\n{off}");
}

// --- run parity + donation + leak (need the backend/linker) --------------------------------------

/// Build+run `src` under a forced `ALIGN_CONST_POOL` state and return the process exit code. The
/// caller holds [`pool_guard`] so the env write is not observed by a concurrent test.
fn run_exit(name: &str, src: &str, pooled: bool) -> Option<i32> {
    if pooled {
        unsafe { std::env::remove_var("ALIGN_CONST_POOL") };
    } else {
        unsafe { std::env::set_var("ALIGN_CONST_POOL", "off") };
    }
    let out = build_and_run(name, src);
    unsafe { std::env::remove_var("ALIGN_CONST_POOL") };
    out.status.code()
}

#[test]
fn pooled_and_unpooled_indexing_agree() {
    let _g = pool_guard();
    if !backend_available() {
        return;
    }
    // Sum a runtime-indexed lookup over a 64-element pooled table; pooled and unpooled must agree.
    let src = format!(
        "fn lookup(i: i64) -> i64 {{ xs := {t}\n return xs[i] }}\n\
         fn main() -> i32 {{\n  mut total := 0\n  mut i := 0\n  loop {{\n    if i >= 64 {{ break }}\n    total = total + lookup(i)\n    i = i + 1\n  }}\n  return total as i32\n}}\n",
        t = table(64)
    );
    let pooled = run_exit("cpool-idx-on", &src, true);
    let unpooled = run_exit("cpool-idx-off", &src, false);
    assert_eq!(pooled, unpooled, "pooled vs unpooled indexing diverged");
    assert!(pooled.is_some());
}

#[test]
fn pooled_source_through_pipeline_agrees_and_frees_no_rodata() {
    let _g = pool_guard();
    if !backend_available() {
        return;
    }
    // A pooled table used as a materializing-pipeline source: a runtime multiplier defeats constant
    // folding, so the source is really read from rodata each call and the output is a fresh owned
    // array. Pooled and unpooled must agree (no crash => rodata was not donated/freed).
    let src = format!(
        "fn compute(m: i64) -> i64 {{ xs := {t}\n ys := xs.map(fn v {{ v * m }}).where(fn v {{ v > 500 }}).to_array()\n return ys.sum() }}\n\
         fn main() -> i32 {{\n  mut acc := 0\n  mut k := 1\n  loop {{\n    if k > 5 {{ break }}\n    acc = acc + compute(k)\n    k = k + 1\n  }}\n  return acc as i32\n}}\n",
        t = table(64)
    );
    let pooled = run_exit("cpool-pipe-on", &src, true);
    let unpooled = run_exit("cpool-pipe-off", &src, false);
    assert_eq!(pooled, unpooled, "pooled vs unpooled pipeline diverged");
    assert!(pooled.is_some(), "pipeline run must succeed (rodata not freed)");
}

#[test]
fn pooled_table_rodata_is_never_freed() {
    let _g = pool_guard();
    // The pooled slot stays a fixed `Ty::Array` (Copy) — it carries no drop and its rodata backing
    // is never handed to a free. The pooled function must issue no CALL to a free (a runtime
    // `declare` of the free symbol is irrelevant — only a call would free the rodata).
    let src = format!("fn lookup(i: i64) -> i64 {{ xs := {t}\n return xs[i] }}\n", t = table(64));
    let ir = emit_llvm_with_exports(&src, &["lookup"]);
    assert!(ir.contains("@const_arr"), "sanity: the table is pooled");
    assert!(
        !ir.lines().any(|l| l.contains("call") && l.contains("_free")),
        "a pooled rodata table must never be freed:\n{ir}"
    );
}

/// The review's reproduced stale-cache miscompile, pinned: a VALUE-ONLY edit of a pooled table
/// must change the printed MIR (impl_hash's input) and therefore miss the incremental object
/// cache. Two subprocess builds against one warm cache root: edit `999` -> `111`, and the rerun
/// must print the new value — never the stale cached object's.
#[test]
fn value_only_edit_of_pooled_table_misses_the_cache() {
    let _g = pool_guard();
    let dir = std::env::temp_dir().join(format!("align_pool_cache_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let cache = dir.join("cache");
    let src = |last: i64| {
        let mut elems: Vec<String> = (0..63).map(|i| i.to_string()).collect();
        elems.push(last.to_string());
        format!("fn main() -> i32 {{\n  xs := [{}]\n  print(xs[63])\n  return 0\n}}\n", elems.join(", "))
    };
    let path = dir.join("t.align");
    let run = |v: i64| {
        std::fs::write(&path, src(v)).expect("write source");
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .arg("run")
            .arg(&path)
            .env("ALIGNC_CACHE", &cache)
            .current_dir(&dir)
            .output()
            .expect("spawn alignc run");
        assert!(out.status.success(), "run failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(run(999), "999");
    assert_eq!(run(111), "111", "a value-only edit must miss the warm cache, never serve stale rodata");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The impl_hash input itself: value-differing tables must never print identical MIR. Covers both
/// the pooled local (Stmt::StoreConstArray) and the #514 aggregate constant (Rvalue::ConstArray).
#[test]
fn const_array_values_are_part_of_printed_mir() {
    let a = mir_text_of("pool_print_a", &program_with_last(999));
    let b = mir_text_of("pool_print_b", &program_with_last(111));
    assert_ne!(a, b, "value-only table edits must change the printed MIR (impl_hash input)");
    assert!(a.contains("999") && b.contains("111"), "elements are printed value-exactly");

    // A dynamic index keeps the Rvalue::ConstArray in MIR (a constant index folds it away).
    let ca = mir_text_of(
        "const_print_a",
        "T := [7, 8, 9]\nfn pick(i: i64) -> i64 = T[i]\nfn main() -> i64 = pick(2)\n",
    );
    let cb = mir_text_of(
        "const_print_b",
        "T := [7, 8, 42]\nfn pick(i: i64) -> i64 = T[i]\nfn main() -> i64 = pick(2)\n",
    );
    assert_ne!(ca, cb, "aggregate-constant value edits must change the printed MIR");
}

fn program_with_last(last: i64) -> String {
    let mut elems: Vec<String> = (0..63).map(|i| i.to_string()).collect();
    elems.push(last.to_string());
    format!("fn main() -> i32 {{\n  xs := [{}]\n  print(xs[63])\n  return 0\n}}\n", elems.join(", "))
}

fn mir_text_of(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(!checked.diags.has_errors(), "unexpected errors in {name}");
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}
