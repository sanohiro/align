//! M10 Slice 2 — std.rand. A Copy `rng` (Xoshiro256++): `rand.seed()` (OS-seeded) /
//! `rand.seed_with(s)` (deterministic); `r.next()` / `r.range(lo, hi)` / `r.shuffle(out xs)` /
//! `r.sample(xs, k)`. The completion condition: `seed_with` is deterministic (same seed → same
//! sequence, portable across runs); `range` is bounds-correct `[lo, hi)` and aborts on `lo >= hi`;
//! `shuffle` yields a permutation of the input (Fisher-Yates); `sample` returns `k` distinct items
//! and aborts on `k < 0` / `k > len`; `rng` is Copy (value pass / reassign); `import` is required.
//! (`docs/impl/07-roadmap.md` M10 Slice 2; `draft.md` §18.2.)

mod common;
use common::*;

/// `seed_with(s)` is deterministic and portable: two rngs seeded with the same `s` produce the same
/// sequence, and the exact first two `next()` outputs are pinned (locks the Xoshiro256++ constants
/// + SplitMix64 seeding, so a change that alters the stream is caught).
#[test]
fn seed_with_is_deterministic_and_pinned() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut a := rand.seed_with(42)
  mut b := rand.seed_with(42)
  print(a.next())
  print(b.next())
  print(a.next())
  print(b.next())
  return 0
}
";
    let out = build_and_run("m10-rand-seed-with", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // Same seed → same sequence (line 1 == line 2, line 3 == line 4); the exact values pin the
    // algorithm (portable across runs / machines).
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "-3425465463722317665\n-3425465463722317665\n5881210131331364753\n5881210131331364753\n"
    );
}

/// `seed()` draws OS entropy (`getrandom`): two independently seeded generators produce different
/// first outputs (a `1/2^64` collision would be a runtime fluke, not a correctness failure).
#[test]
fn seed_os_differs_between_generators() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut a := rand.seed()
  mut b := rand.seed()
  print(a.next())
  print(b.next())
  return 0
}
";
    let out = build_and_run("m10-rand-seed-os", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "expected two outputs, got {text:?}");
    assert_ne!(lines[0], lines[1], "two OS seeds produced the same first value (astronomically unlikely — likely a seeding bug)");
}

/// `range(lo, hi)` is bounds-correct on `[lo, hi)`: a single-value range `[5, 6)` is always exactly
/// `5` (lo inclusive, hi exclusive), and a negative single-value range `[-3, -2)` is always `-3`.
/// The tight ranges make the boundary contract a deterministic assertion without a statistical loop.
#[test]
fn range_is_half_open_and_handles_negatives() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(1)
  print(r.range(5, 6))
  print(r.range(5, 6))
  print(r.range(-3, -2))
  print(r.range(-3, -2))
  return 0
}
";
    let out = build_and_run("m10-rand-range-boundary", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n5\n-3\n-3\n");
}

/// `range(lo, hi)` with `lo >= hi` is an empty range (nothing to draw) and aborts at runtime, like
/// an out-of-bounds index.
#[test]
fn range_empty_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(1)
  print(r.range(20, 10))
  return 0
}
";
    let out = build_and_run("m10-rand-range-empty", prog);
    assert!(!out.status.success(), "range(20, 10) (lo >= hi) must abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("empty range"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `shuffle(out xs)` is an in-place Fisher-Yates permutation: the multiset is preserved (the sum is
/// unchanged) and, under a fixed seed, the resulting order is deterministic (portable). The pinned
/// order proves the elements are rearranged, not merely summed.
#[test]
fn shuffle_is_a_deterministic_permutation() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(123)
  mut xs := [10, 20, 30, 40, 50][0..5]
  r.shuffle(xs)
  print(xs.sum())
  print(xs[0])
  print(xs[1])
  print(xs[2])
  print(xs[3])
  print(xs[4])
  return 0
}
";
    let out = build_and_run("m10-rand-shuffle", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // sum preserved (150 = 10+20+30+40+50) → same multiset; the pinned order is a permutation.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "150\n10\n30\n20\n50\n40\n");
}

/// `sample(xs, k)` returns `k` distinct items (without replacement) as a fresh owned `array<T>`;
/// under a fixed seed the drawn items are deterministic. The three drawn values are pinned and are
/// all distinct members of the source set.
#[test]
fn sample_draws_k_distinct_items() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(123)
  s := r.sample([100, 200, 300, 400, 500, 600][0..6], 3)
  print(s.len())
  print(s[0])
  print(s[1])
  print(s[2])
  return 0
}
";
    let out = build_and_run("m10-rand-sample", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // 3 distinct members of {100,…,600}, pinned for portability.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n400\n600\n500\n");
}

/// `sample(xs, 0)` is the empty draw — a zero-length owned array (owns no buffer, so its `Drop` is a
/// harmless `free(null)`).
#[test]
fn sample_zero_is_empty() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(1)
  s := r.sample([1, 2, 3][0..3], 0)
  print(s.len())
  return 0
}
";
    let out = build_and_run("m10-rand-sample-zero", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

/// `sample(xs, k)` with `k > xs.len()` (more distinct items than exist) aborts at runtime.
#[test]
fn sample_k_too_large_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(1)
  s := r.sample([1, 2, 3][0..3], 5)
  print(s.len())
  return 0
}
";
    let out = build_and_run("m10-rand-sample-toobig", prog);
    assert!(!out.status.success(), "sample(k=5) from a length-3 slice must abort");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("distinct items"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `rng` is a **Copy** value: passing it by value to a function gives the callee its own copy, so
/// the callee's `next()` does not advance the caller's generator — two calls with the same `rng`
/// argument return the same first value. Reassigning an `rng` local also works (Copy, no move).
#[test]
fn rng_is_copy_by_value_and_reassignable() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.rand
fn first(r0: rng) -> i64 {
  mut r := r0
  return r.next()
}
pub fn main() -> i32 {
  mut r := rand.seed_with(99)
  a := first(r)
  b := first(r)
  print(a)
  print(b)
  mut r2 := rand.seed_with(1)
  r2 = rand.seed_with(2)
  print(r2.next())
  return 0
}
";
    let out = build_and_run("m10-rand-copy", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "expected three outputs, got {text:?}");
    // Copy semantics: the callee mutated its own copy, so the caller's rng is unchanged → a == b.
    assert_eq!(lines[0], lines[1], "rng passed by value must be a copy (callee's next() must not advance the caller's rng)");
}

// --- negative (type-check) cases ---------------------------------------------------------------

/// The `std.rand` builtins require `import std.rand`.
#[test]
fn rand_requires_import() {
    let src = "\
pub fn main() -> i32 {
  mut r := rand.seed_with(1)
  print(r.next())
  return 0
}
";
    assert!(check_errs("m10-rand-noimport", src), "rand.* without `import std.rand` must error");
}

/// The `rng` receiver of `next`/`range`/`shuffle`/`sample` must be a `mut` local — the method
/// advances the generator state in place.
#[test]
fn rng_method_requires_mut_receiver() {
    let src = "\
import std.rand
pub fn main() -> i32 {
  r := rand.seed_with(1)
  print(r.next())
  return 0
}
";
    assert!(check_errs("m10-rand-immut", src), "advancing an immutable rng must be a type error");
}

/// The numeric arguments (`seed_with`'s seed, `range`'s bounds, `sample`'s count) must be exactly
/// `i64` — the `align_rt_rng_*` runtime ABI — not a narrower width (Align has no implicit int
/// coercion; the `time.sleep` #343 discipline).
#[test]
fn rng_numeric_args_must_be_i64() {
    let src = "\
import std.rand
pub fn main() -> i32 {
  mut r := rand.seed_with(1)
  lo: i32 := 3
  print(r.range(lo, 10))
  return 0
}
";
    assert!(check_errs("m10-rand-i32-arg", src), "a non-i64 range bound must be a type error");
}

/// An `rng` advance is Impure (it mutates generator state / reads OS entropy), so an rng-using
/// function is never `Pure` and is rejected by `par_map` (which requires a Pure closure).
#[test]
fn rng_closure_rejected_by_par_map() {
    let src = "\
import std.rand
fn f(x: i64) -> i64 {
  mut r := rand.seed_with(x)
  return r.next()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m10-rand-parmap", src), "an rng-using (impure) closure must be rejected by par_map");
}
