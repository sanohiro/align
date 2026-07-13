//! `sort()` / `sort_by_key(f)` lower to a **stable, bottom-up merge sort** (O(n log n)) with an
//! insertion-sort base case for short runs (`SORT_INSERTION_THRESHOLD` in `align_mir`). This file
//! is the correctness net for that rewrite (Codex binary-optimization audit item 4): random /
//! sorted / reverse / duplicate / all-equal / empty / single-element inputs, the base-case ⇄ merge
//! threshold boundaries (N = threshold ± 1), and — the property the old insertion sort also had but
//! that a naive merge would break — **stability** (equal keys keep their input relative order),
//! tested both in the insertion base case and through several merge passes.
//!
//! Larger inputs are generated at runtime with `loop` + `array_builder` (no giant literals); a
//! self-contained LCG stands in for randomness so the tests need no `import`. `main` returns a small
//! `ok` flag (1 = pass) that becomes the process exit code, or prints an exact expected sequence.

mod common;
use common::*;

fn code(out: &std::process::Output) -> Option<i32> {
    out.status.code()
}

/// Build a program that fills an `array<i64>` of length `n` with `expr` (in terms of loop index
/// `i`), sorts it ascending, and returns 1 iff the result is non-decreasing (adjacent order holds)
/// AND the element sum is preserved (no element dropped or duplicated).
fn sorted_and_multiset_preserved(n: usize, expr: &str) -> String {
    format!(
        "fn main() -> i32 {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; b.push({expr}); i = i + 1 }}\n\
        \x20 mut xs := b.build()\n\
        \x20 mut before := 0\n\
        \x20 mut a := 0\n\
        \x20 loop {{ if a >= {n} {{ break 0 }}; before = before + xs[a]; a = a + 1 }}\n\
        \x20 ys := xs.sort()\n\
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

// --- shapes: random / sorted / reverse / duplicate / all-equal --------------------------------

/// A large pseudo-random input (an LCG whose wrapping i64 arithmetic sprays the whole range): sorts
/// non-decreasing and preserves the multiset. This is the headline O(n log n) correctness case.
#[test]
fn random_large_is_sorted_and_preserves_multiset() {
    if !backend_available() {
        return;
    }
    // LCG step folded into the push expression via a running `s` is awkward; use a fresh mul/add on
    // the index instead — still spreads across the range and is deterministic.
    let src = sorted_and_multiset_preserved(20000, "(i * 2654435761 + 1013904223) % 100000 - 50000");
    let out = build_and_run("sortm-rand", &src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Already-sorted input stays sorted (a common fast-path shape; must not be corrupted).
#[test]
fn already_sorted_stays_sorted() {
    if !backend_available() {
        return;
    }
    let src = sorted_and_multiset_preserved(5000, "i");
    let out = build_and_run("sortm-asc", &src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Reverse-sorted input (the worst case for the old insertion sort) sorts correctly.
#[test]
fn reverse_sorted_input() {
    if !backend_available() {
        return;
    }
    let src = sorted_and_multiset_preserved(5000, "5000 - i");
    let out = build_and_run("sortm-rev", &src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Heavy duplicates (only 5 distinct keys over 4000 elements) and the all-equal degenerate case.
#[test]
fn heavy_duplicates_and_all_equal() {
    if !backend_available() {
        return;
    }
    let dup = sorted_and_multiset_preserved(4000, "i % 5");
    assert_eq!(code(&build_and_run("sortm-dup", &dup)), Some(1));
    let eq = sorted_and_multiset_preserved(4000, "7");
    assert_eq!(code(&build_and_run("sortm-eq", &eq)), Some(1));
}

// --- base-case ⇄ merge threshold boundaries (N = 31 / 32 / 33 and the next octave) --------------

/// N straddling the insertion/merge threshold (32): 31 and 32 are one insertion run; 33 forces the
/// first merge pass; 64/65 exercise a full and a ragged second pass. All must sort a reversed input.
#[test]
fn threshold_boundaries() {
    if !backend_available() {
        return;
    }
    for n in [31usize, 32, 33, 63, 64, 65, 128, 129] {
        let src = sorted_and_multiset_preserved(n, &format!("{n} - i"));
        let out = build_and_run(&format!("sortm-thresh-{n}"), &src);
        assert_eq!(code(&out), Some(1), "N={n} failed to sort; stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
}

// --- degenerate sizes -------------------------------------------------------------------------

/// Empty and single-element pipelines: the `len < 2` fast path returns the buffer untouched (and
/// must still be a valid, freeable array). Sum over an empty/one-element sorted array is 0 / the
/// element.
#[test]
fn empty_and_single_element() {
    if !backend_available() {
        return;
    }
    // Empty: filter everything out, sort, sum → 0.
    let empty = "fn gone(x: i64) -> bool = x > 100\nfn main() -> i32 {\n  return [1, 2, 3].where(gone).sort().sum() as i32\n}\n";
    assert_eq!(code(&build_and_run("sortm-empty", empty)), Some(0), "stderr: {}", String::from_utf8_lossy(&build_and_run("sortm-empty", empty).stderr));
    // Single: keep one element, sort, sum → that element.
    let single = "fn keep(x: i64) -> bool = x == 42\nfn main() -> i32 {\n  return [7, 42, 9].where(keep).sort().sum() as i32\n}\n";
    assert_eq!(code(&build_and_run("sortm-single", single)), Some(42));
}

// --- stability --------------------------------------------------------------------------------

/// Stability in the insertion base case (N < threshold): elements sharing a key keep their input
/// order. Input `[6,7,8,3,4,5,0,1,2]` with key `x % 3` groups (in input order) 6,3,0 | 7,4,1 |
/// 8,5,2 — a *descending* run within each key group, which a merge that mishandles equal keys (or a
/// sort-by-(key,value)) would reorder to ascending. The exact expected order pins stability.
#[test]
fn stability_insertion_base_case() {
    if !backend_available() {
        return;
    }
    let src = "fn k(x: i64) -> i64 = x % 3\nfn main() -> Result<(), Error> {\n  arena {\n    s := [6, 7, 8, 3, 4, 5, 0, 1, 2].sort_by_key(k)\n    mut i := 0\n    loop { if i >= 9 { break 0 }; print(s[i]); i = i + 1 }\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("sortm-stab-ins", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n3\n0\n7\n4\n1\n8\n5\n2\n");
}

/// Stability *through several merge passes* (N well over the threshold): a descending input keyed by
/// `x % 7` must, after `sort_by_key`, be non-decreasing in key and — within each equal-key run —
/// still descending in value (the preserved input order). Returns 1 iff both hold for every pair.
#[test]
fn stability_through_merge_passes() {
    if !backend_available() {
        return;
    }
    let n = 400usize;
    let src = format!(
        "fn k(x: i64) -> i64 = x % 7\n\
        fn main() -> i32 {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; b.push({n} - 1 - i); i = i + 1 }}\n\
        \x20 ys := b.build().sort_by_key(k)\n\
        \x20 mut ok := 1\n\
        \x20 mut j := 0\n\
        \x20 loop {{\n\
        \x20   if j + 1 >= {n} {{ break 0 }}\n\
        \x20   ka := ys[j] % 7\n\
        \x20   kb := ys[j+1] % 7\n\
        \x20   if ka > kb {{ ok = 0 }}\n\
        \x20   if ka == kb {{ if ys[j] < ys[j+1] {{ ok = 0 }} }}\n\
        \x20   j = j + 1\n\
        \x20 }}\n\
        \x20 return ok\n\
        }}\n"
    );
    let out = build_and_run("sortm-stab-merge", &src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

// --- non-i64 element / key types --------------------------------------------------------------

/// `f64` `sort()` including a duplicate value; exact ascending order printed back.
#[test]
fn float_sort_with_duplicate() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  arena {\n    s := [3.5, 1.25, 2.0, 0.5, 1.25].sort()\n    mut i := 0\n    loop { if i >= 5 { break 0 }; print(s[i]); i = i + 1 }\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("sortm-f64", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0.5\n1.25\n1.25\n2.0\n3.5\n");
}

/// `char` elements sorted by identity key (char is orderable but not numeric, so via `sort_by_key`).
#[test]
fn char_sort_by_identity() {
    if !backend_available() {
        return;
    }
    let src = "fn id(c: char) -> char = c\nfn main() -> Result<(), Error> {\n  arena {\n    s := ['d', 'a', 'c', 'b', 'a'].sort_by_key(id)\n    mut i := 0\n    loop { if i >= 5 { break 0 }; print(s[i]); i = i + 1 }\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("sortm-char", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\na\nb\nc\nd\n");
}

/// A `str` key sorted through the merge (N over the threshold): three name buckets keyed by `x % 3`,
/// a reversed input, must come out grouped as aaa… < mmm… < zzz… (byte-lexicographic key order).
#[test]
fn str_key_through_merge() {
    if !backend_available() {
        return;
    }
    let n = 90usize;
    let src = format!(
        "fn key(x: i64) -> str = if x % 3 == 0 {{ \"aaa\" }} else {{ if x % 3 == 1 {{ \"mmm\" }} else {{ \"zzz\" }} }}\n\
        fn main() -> i32 {{\n\
        \x20 mut b: array_builder<i64> := array_builder()\n\
        \x20 mut i := 0\n\
        \x20 loop {{ if i >= {n} {{ break 0 }}; b.push({n} - 1 - i); i = i + 1 }}\n\
        \x20 ys := b.build().sort_by_key(key)\n\
        \x20 mut ok := 1\n\
        \x20 mut j := 0\n\
        \x20 loop {{\n\
        \x20   if j + 1 >= {n} {{ break 0 }}\n\
        \x20   if ys[j] % 3 > ys[j+1] % 3 {{ ok = 0 }}\n\
        \x20   j = j + 1\n\
        \x20 }}\n\
        \x20 return ok\n\
        }}\n"
    );
    let out = build_and_run("sortm-strkey", &src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}
