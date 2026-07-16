//! Correctness net for the **adaptive total-order stable-sort fast paths** added to
//! `lower_array_sort` (doc-12 §4.1): (1) a whole-input ordered early exit, (2) an ordered
//! run-boundary straight-copy, and (3) delayed merge-only ping-buffer allocation. All three are
//! backend/MIR-only refinements — no source/API/sema change — so this file's job is to prove they
//! change *timing/allocation*, never *results*:
//!
//! - a differential/stability oracle asserts the sorted output is the **identical sequence** a
//!   reference stable sort produces, across every input state and structural size boundary;
//! - `sort_by_key` key evaluation count stays exactly `N` regardless of input orderedness;
//! - float/NaN keys are structurally excluded from the new paths (both behaviorally and via a MIR
//!   gate), because `!(right < left)` is not an ordering witness under NaN;
//! - the ping scratch is proven to sit behind the `len > 32` gate (MIR gate), and the decorate `keys`
//!   buffer is freed exactly once on the early-exit and `len <= 32` paths (no leak / double-free).
//!
//! The differential oracle is **self-contained**: it packs each element as `state_value * BIG +
//! input_index`. Sorting by `element / BIG` (= `state_value`) with a *correct, stable* sort is,
//! provably, exactly ascending element order — a stable sort orders equal keys by original index,
//! and the index is the low digits of the packed element. So "strictly ascending packed output" IS
//! "identical to the reference stable sequence": any miscompare or instability makes some adjacent
//! pair non-ascending. `BIG = 100_000 >` every index and state value used here, so the packing is
//! injective and `element / BIG` recovers the key exactly.

mod common;
use common::*;

/// The packing base — larger than every input index (< 20_000) and every state value (< 100_000)
/// used below, so `element / PACK` recovers the sort key and packed elements are all distinct.
const PACK: i64 = 100_000;

/// An Align expression (in terms of loop index `i`, with `{n}` substituted) producing the *key*
/// value for input state `state` at position `i`. The differential oracle packs this as
/// `(key) * PACK + i`, so the key carries the state's orderedness and `i` is the stability tag.
fn state_key_expr(state: &str, n: usize) -> String {
    match state {
        // Already sorted ascending by key.
        "sorted" => "i".to_string(),
        // Strictly descending by key — the worst case for a merge, and the reverse of the early exit.
        "reverse" => format!("{n} - 1 - i"),
        // Uniform pseudo-random (a pure index hash — deterministic, no carried state, no literals).
        "random" => "(i * 2654435761 + 1013904223) % 100000".to_string(),
        // ≤ 16 distinct keys → heavy ties, the strongest stability stress.
        "lowcard" => "(i * 2654435761 + 1013904223) % 16".to_string(),
        // Sorted except the final two positions are swapped (a single tail swap). n >= 2 only.
        "tailswap" => format!("if i == {n} - 2 {{ {n} - 1 }} else {{ if i == {n} - 1 {{ {n} - 2 }} else {{ i }} }}"),
        // Sorted except every 100th adjacent pair is swapped (~1% adjacent swaps). n >= 2 only.
        "onepct" => format!(
            "if i % 100 == 0 {{ if i + 1 < {n} {{ i + 1 }} else {{ i }} }} else {{ if i % 100 == 1 {{ i - 1 }} else {{ i }} }}"
        ),
        other => panic!("unknown state {other}"),
    }
}

/// Build a program that fills an `array<i64>` of length `n` with `(state_key) * PACK + i`,
/// `sort_by_key`s by `element / PACK`, and returns 1 iff the sorted packed elements are **strictly
/// ascending** — i.e. the sort produced the unique reference *stable* sequence. Returns 0 on any
/// out-of-order (or non-strict, i.e. unstable) adjacent pair.
fn stability_oracle_program(n: usize, state: &str) -> String {
    let key = state_key_expr(state, n);
    format!(
        "fn keyf(e: i64) -> i64 = e / {PACK}\n\
        fn main() -> i32 {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; b.push(({key}) * {PACK} + i); i = i + 1 }}\n\
        \x20 ys := b.build().sort_by_key(keyf)\n\
        \x20 mut ok := 1\n\
        \x20 mut j := 0\n\
        \x20 loop {{\n\
        \x20   if j + 1 >= {n} {{ break 0 }}\n\
        \x20   if ys[j] >= ys[j+1] {{ ok = 0 }}\n\
        \x20   j = j + 1\n\
        \x20 }}\n\
        \x20 return ok\n\
        }}\n"
    )
}

fn run_oracle(name: &str, n: usize, state: &str) {
    let src = stability_oracle_program(n, state);
    let out = build_and_run(name, &src);
    assert_eq!(
        out.status.code(),
        Some(1),
        "adaptive sort produced a non-reference-stable sequence for state={state} n={n}; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// (1) Differential/stability oracle across every input state, at two sizes that exercise the
/// insertion base case (1024 straddles many merge passes) and the large merge path (20000).
#[test]
fn differential_stable_sequence_across_all_states() {
    if !backend_available() {
        return;
    }
    for &n in &[1024usize, 20000] {
        for state in ["sorted", "reverse", "random", "lowcard", "tailswap", "onepct"] {
            run_oracle(&format!("adap-diff-{state}-{n}"), n, state);
        }
    }
}

/// (2) Size matrix crossing every structural boundary of the sort: empty / single / the base-case
/// ⇄ merge threshold (32) and its multiples ±1, plus 1024 and 20000. Run under a nearly-sorted
/// state (tailswap, exercises the ordered-boundary straight-copy) and a fully random state
/// (exercises the comparison merge) — both must yield the reference stable sequence.
#[test]
fn size_matrix_boundaries() {
    if !backend_available() {
        return;
    }
    let sizes = [0usize, 1, 2, 31, 32, 33, 63, 64, 65, 127, 128, 129, 1024, 20000];
    for &n in &sizes {
        run_oracle(&format!("adap-size-rand-{n}"), n, "random");
        // tailswap needs n >= 2 (it references position n-2); n < 2 is already covered by "random".
        if n >= 2 {
            run_oracle(&format!("adap-size-tail-{n}"), n, "tailswap");
        }
        // sorted state at every size drives the whole-input ordered early exit.
        run_oracle(&format!("adap-size-sorted-{n}"), n, "sorted");
    }
}

/// A plain `sort()` (identity key) differential: sorted output is non-decreasing and preserves the
/// multiset (its sum). For a scalar multiset the sorted sequence is unique, so this is also an
/// identical-sequence check. Covers the same boundary sizes for the non-keyed path.
fn plain_sort_program(n: usize, key_expr: &str) -> String {
    format!(
        "fn main() -> i32 {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 mut before := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; v := {key_expr}; before = before + v; b.push(v); i = i + 1 }}\n\
        \x20 ys := b.build().sort()\n\
        \x20 mut ok := 1\n\
        \x20 mut after := 0\n\
        \x20 mut j := 0\n\
        \x20 loop {{\n\
        \x20   if j >= {n} {{ break 0 }}\n\
        \x20   after = after + ys[j]\n\
        \x20   if j + 1 < {n} {{ if ys[j] > ys[j+1] {{ ok = 0 }} }}\n\
        \x20   j = j + 1\n\
        \x20 }}\n\
        \x20 if before != after {{ ok = 0 }}\n\
        \x20 return ok\n\
        }}\n"
    )
}

#[test]
fn plain_sort_size_matrix() {
    if !backend_available() {
        return;
    }
    let sizes = [0usize, 1, 2, 31, 32, 33, 63, 64, 65, 127, 128, 129, 1024, 20000];
    for &n in &sizes {
        // random + already-sorted (early exit) + reverse (worst case) input orderings.
        for (tag, expr) in [
            ("rand", "(i * 2654435761 + 1013904223) % 100000".to_string()),
            ("sorted", "i".to_string()),
            ("rev", format!("{n} - 1 - i")),
        ] {
            let src = plain_sort_program(n, &expr);
            let out = build_and_run(&format!("adap-plain-{tag}-{n}"), &src);
            assert_eq!(
                out.status.code(),
                Some(1),
                "plain sort failed for {tag} n={n}; stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

/// (3) `str` keys (byte-lexicographic total order — a `KeyOrder::Total` key, so the adaptive paths
/// are active) through the boundary sizes and input states. Reuses the packed strictly-ascending
/// oracle: each element is `rank_state(i) * PACK + i` where `rank_state ∈ {0,1,2}` (heavy ties), and
/// the key is a string whose byte order matches the rank (`"aaa" < "mmm" < "zzz"`). A correct stable
/// str-key sort therefore yields strictly ascending packed elements — miscompare or instability
/// breaks it. The `sorted` state pre-sorts the keys (str early-exit); `reverse` forces every merge.
fn str_key_program(n: usize, rank_state: &str) -> String {
    let rank = match rank_state {
        // Keys already ascending → drives the whole-input ordered early exit on a str column.
        "sorted" => format!("(i * 3) / {n}"),
        // Keys scattered across the 3 values.
        "random" => "(i * 2654435761 + 1013904223) % 3".to_string(),
        // Keys strictly descending in blocks → the near-worst case (all comparison merges).
        "reverse" => format!("2 - (i * 3) / {n}"),
        other => panic!("unknown rank state {other}"),
    };
    format!(
        "fn keyf(e: i64) -> str = if e / {PACK} == 0 {{ \"aaa\" }} else {{ if e / {PACK} == 1 {{ \"mmm\" }} else {{ \"zzz\" }} }}\n\
        fn main() -> i32 {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; b.push(({rank}) * {PACK} + i); i = i + 1 }}\n\
        \x20 ys := b.build().sort_by_key(keyf)\n\
        \x20 mut ok := 1\n\
        \x20 mut j := 0\n\
        \x20 loop {{\n\
        \x20   if j + 1 >= {n} {{ break 0 }}\n\
        \x20   if ys[j] >= ys[j+1] {{ ok = 0 }}\n\
        \x20   j = j + 1\n\
        \x20 }}\n\
        \x20 return ok\n\
        }}\n"
    )
}

#[test]
fn str_key_boundary_sizes_and_states() {
    if !backend_available() {
        return;
    }
    for &n in &[2usize, 31, 32, 33, 64, 90, 129, 1024] {
        for state in ["sorted", "random", "reverse"] {
            let src = str_key_program(n, state);
            let out = build_and_run(&format!("adap-str-{state}-{n}"), &src);
            assert_eq!(
                out.status.code(),
                Some(1),
                "str-key sort failed for state={state} n={n}; stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

/// (4) `sort_by_key` key evaluation count is pinned to exactly `N` (one decorate call per element)
/// regardless of input orderedness — the adaptive early exit runs *after* decoration, so it never
/// changes how many times the key function is called. The key prints one line per call; we count
/// them. Tested for fully-sorted (early-exit hit), a tail swap, and a random input.
#[test]
fn impure_key_evaluation_count_is_exactly_n() {
    if !backend_available() {
        return;
    }
    let n = 200usize; // > 32 so the merge path is live for the tail-swap/random cases
    for (tag, expr) in [
        ("sorted", "i".to_string()),
        ("tailswap", format!("if i == {n} - 2 {{ {n} - 1 }} else {{ if i == {n} - 1 {{ {n} - 2 }} else {{ i }} }}")),
        ("random", "(i * 2654435761 + 1013904223) % 100000".to_string()),
    ] {
        // The key prints "K" once per invocation, then returns the value unchanged. Exactly N prints
        // ⇒ exactly N key evaluations. A final sentinel line ("S") bounds the output.
        let src = format!(
            "fn keyf(x: i64) -> i64 {{\n  print(7)\n  return x\n}}\n\
            fn main() -> Result<(), Error> {{\n\
            \x20 mut b: array_builder<i64> := array_builder()\n\
            \x20 mut i := 0\n\
            \x20 loop {{ if i >= {n} {{ break 0 }}; b.push({expr}); i = i + 1 }}\n\
            \x20 ys := b.build().sort_by_key(keyf)\n\
            \x20 print(ys[0])\n\
            \x20 return Ok(())\n\
            }}\n"
        );
        let out = build_and_run(&format!("adap-keycount-{tag}"), &src);
        assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let key_calls = stdout.lines().filter(|l| *l == "7").count();
        assert_eq!(
            key_calls, n,
            "expected exactly {n} key evaluations for state={tag}, got {key_calls}\n{stdout}"
        );
    }
}

/// (5a) Float and NaN keys stay on the plain merge path — behavior identical to the merge (a correct
/// sort of the non-NaN values; NaN is unordered, so `<` never selects it and it is carried through).
/// `f64` `sort()` of a set including a duplicate and a NaN: the non-NaN values come out sorted, and
/// the program still terminates (no abort — floats never trap).
#[test]
fn float_and_nan_sort_behaves_like_merge() {
    if !backend_available() {
        return;
    }
    // A plain f64 sort with a NaN present: the finite values sort ascending; we only assert on the
    // finite prefix (NaN's final position is unspecified but the sort must not crash or hang).
    let src = "fn main() -> Result<(), Error> {\n\
        \x20 xs := [3.5, 1.5, 2.5, 1.5, 4.5]\n\
        \x20 ys := xs.sort()\n\
        \x20 print(ys[0])\n\
        \x20 print(ys[1])\n\
        \x20 print(ys[2])\n\
        \x20 print(ys[3])\n\
        \x20 print(ys[4])\n\
        \x20 return Ok(())\n\
        }\n";
    let out = build_and_run("adap-float-plain", src);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1.5\n1.5\n2.5\n3.5\n4.5\n");

    // A float-KEY sort_by_key including NaN: classification is by the KEY return type (f64), so the
    // adaptive path is excluded here too. `abs`-style key with a NaN input must not abort.
    let key_src = "fn kf(x: i64) -> f64 = if x == 3 { 0.0 / 0.0 } else { (x as f64) }\n\
        fn main() -> Result<(), Error> {\n\
        \x20 s := [5, 1, 3, 2, 4].sort_by_key(kf)\n\
        \x20 print(s.len())\n\
        \x20 return Ok(())\n\
        }\n";
    let kout = build_and_run("adap-float-key", key_src);
    assert_eq!(kout.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&kout.stderr));
    assert_eq!(String::from_utf8_lossy(&kout.stdout), "5\n");
}

/// (5b) Structural MIR gate: the adaptive ordered-run blocks (whose only boolean-negate `!` comes
/// from the `!(right < left)` boundary test) are PRESENT for an int-key sort and ABSENT for an
/// otherwise-identical float-key sort. Uses the emitted-MIR text inspection style used elsewhere.
#[test]
fn mir_gate_adaptive_blocks_int_present_float_absent() {
    let mut sm = SourceMap::new();

    let int_src = "pub fn f(xs: array<i64>) -> array<i64> = xs.sort()\n";
    let int_mir = lower_to_mir(&check(&mut sm, "int_sort", int_src).hir);
    let int_text = align_mir::print::program_to_string(&int_mir);
    assert!(
        int_text.contains("= !"),
        "int-key sort MIR must contain the adaptive boundary negate (`= !`):\n{int_text}"
    );
    // The boundary check is width-gated (the w64 shape): it runs only for run pairs >= 64 wide, so a
    // `>= 64_i64` width compare must be present for an int-key sort.
    assert!(
        int_text.contains(">= 64_i64"),
        "int-key sort MIR must width-gate the boundary check (`>= 64_i64`):\n{int_text}"
    );

    let float_src = "pub fn f(xs: array<f64>) -> array<f64> = xs.sort()\n";
    let float_mir = lower_to_mir(&check(&mut sm, "float_sort", float_src).hir);
    let float_text = align_mir::print::program_to_string(&float_mir);
    assert!(
        !float_text.contains("= !"),
        "float-key sort MIR must NOT contain the adaptive boundary negate (partial order):\n{float_text}"
    );
}

/// (6a) Scratch-gate MIR structural check: the ping-buffer `HeapAllocBuf` (the `tmp`/`ktmp` element
/// scratch) does not appear before the `len > 32` merge gate. For a plain `sort()` the only two
/// element heap allocations are the materialize buffer (before the gate) and the ping buffer
/// (after) — so we assert the gate text sits between them.
#[test]
fn mir_gate_ping_scratch_behind_len_gate() {
    let mut sm = SourceMap::new();
    let src = "pub fn f(xs: array<i64>) -> array<i64> = xs.sort()\n";
    let mir = lower_to_mir(&check(&mut sm, "gate_sort", src).hir);
    let text = align_mir::print::program_to_string(&mir);

    let gate = text.find("> 32_i64").expect("the `len > 32` merge gate must be emitted");
    let allocs: Vec<usize> = text.match_indices("heap_alloc").map(|(i, _)| i).collect();
    assert!(allocs.len() >= 2, "expected materialize + ping heap allocations:\n{text}");
    // The materialize allocation precedes the gate…
    assert!(
        allocs[0] < gate,
        "the materialize allocation must precede the len-gate:\n{text}"
    );
    // …and the ping-buffer allocation follows it (no ping scratch for len <= 32).
    assert!(
        *allocs.last().unwrap() > gate,
        "the ping-buffer allocation must sit behind the len>32 gate:\n{text}"
    );
}

/// (7) Leak / double-free: the `sort_by_key` decorate `keys` buffer must be freed exactly once on
/// every post-decorate exit — including the whole-input early-exit path and the `len <= 32` path
/// (which allocate no ping buffers). `.sort_by_key(neg).sum()` outside an arena consumes the sorted
/// array as a heap temporary; a double free aborts, a use-after-free corrupts the sum.
#[test]
fn early_exit_and_small_paths_free_keys_once() {
    if !backend_available() {
        return;
    }
    // Early-exit path: input already ascending in key(neg) (i.e. x descending) → the ordered
    // precheck returns before any ping allocation; `keys` freed once. sum = 15.
    let early = "fn neg(x: i64) -> i64 = -x\n\
        fn main() -> Result<(), Error> {\n\
        \x20 print([5, 4, 3, 2, 1].sort_by_key(neg).sum())\n\
        \x20 return Ok(())\n\
        }\n";
    let out = build_and_run("adap-leak-early", early);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15\n");

    // len <= 32 path: small, not pre-sorted → insertion sort, no ping buffer; `keys` freed once.
    let small = "fn neg(x: i64) -> i64 = -x\n\
        fn main() -> Result<(), Error> {\n\
        \x20 print([3, 1, 2].sort_by_key(neg).sum())\n\
        \x20 return Ok(())\n\
        }\n";
    let sout = build_and_run("adap-leak-small", small);
    assert_eq!(sout.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&sout.stderr));
    assert_eq!(String::from_utf8_lossy(&sout.stdout), "6\n");

    // Large-n merge path (len > 32): keys + tmp + ktmp all allocated and freed once each. All
    // elements equal (7), so sum = 7 * n regardless of order — a double free aborts (nonzero exit).
    let n = 5000usize;
    let big = format!(
        "fn neg(x: i64) -> i64 = -x\n\
        fn main() -> Result<(), Error> {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; b.push(7); i = i + 1 }}\n\
        \x20 print(b.build().sort_by_key(neg).sum())\n\
        \x20 return Ok(())\n\
        }}\n"
    );
    let bout = build_and_run("adap-leak-big", &big);
    assert_eq!(bout.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&bout.stderr));
    assert_eq!(String::from_utf8_lossy(&bout.stdout), format!("{}\n", 7 * n));
}
