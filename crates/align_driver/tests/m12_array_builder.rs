//! M12 Slice A6 — the growable `array_builder<T>`: the third grow-then-freeze member
//! (`builder`->`string`, `buffer`->bytes, this->`array<T>`). `array_builder()` opens an empty
//! builder; `b.push(v)` / `b.append(xs: slice<T>)` grow it in place (a `mut`-bound local); `b.build()`
//! **consumes** it into an owned `array<T>` (a zero-copy ptr+len retype over `align_rt_realloc`
//! storage). Element set v1 = Copy scalars + `string` (push MOVES a string in; the builder's own Drop
//! deep-frees pushed-not-frozen strings). Move-handle exclusions (no aggregate riding, capture into
//! par_map/spawn rejected). (`docs/impl/07-roadmap.md` M12 Slice A6; `draft.md` §18.2.)

mod common;
use common::*;

fn code(out: &std::process::Output) -> Option<i32> {
    out.status.code()
}

// --- scalar round-trips + freeze-to-array<T> feeds the pipeline -----------------------------------

/// The headline: push i64 elements, freeze into an owned `array<i64>`, and consume it with the
/// existing pipeline (`.sum()`) — the whole point of grow-then-freeze. Also index the frozen array.
#[test]
fn i64_push_build_then_pipeline_sum() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<i64> := array_builder()\n  b.push(10)\n  b.push(20)\n  b.push(12)\n  xs := b.build()\n  return (xs.sum() + xs[0]) as i32\n}\n";
    let out = build_and_run("ab-i64", src);
    assert_eq!(code(&out), Some(52), "stderr: {}", String::from_utf8_lossy(&out.stderr)); // 42 + 10
}

/// f64 round-trip: push floats, freeze, and reduce with `.sum()`.
#[test]
fn f64_push_build_then_sum() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<f64> := array_builder()\n  b.push(1.5)\n  b.push(2.25)\n  b.push(0.25)\n  xs := b.build()\n  return (xs.sum() * 4.0) as i32\n}\n";
    let out = build_and_run("ab-f64", src);
    assert_eq!(code(&out), Some(16), "stderr: {}", String::from_utf8_lossy(&out.stderr)); // 4.0 * 4
}

/// bool round-trip: push then index each element back out of the frozen array.
#[test]
fn bool_push_build_then_index() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<bool> := array_builder()\n  b.push(true)\n  b.push(false)\n  b.push(true)\n  xs := b.build()\n  mut n := 0\n  if xs[0] { n = n + 1 }\n  if xs[1] { n = n + 10 }\n  if xs[2] { n = n + 100 }\n  return n\n}\n";
    let out = build_and_run("ab-bool", src);
    assert_eq!(code(&out), Some(101), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// char round-trip: push then index a char back and compare.
#[test]
fn char_push_build_then_index() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<char> := array_builder()\n  b.push('a')\n  b.push('z')\n  xs := b.build()\n  mut n := 0\n  if xs[0] == 'a' { n = n + 1 }\n  if xs[1] == 'z' { n = n + 2 }\n  return n\n}\n";
    let out = build_and_run("ab-char", src);
    assert_eq!(code(&out), Some(3), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

// --- empty / append / order ----------------------------------------------------------------------

/// An empty builder freezes into an empty `array<T>` (`.len() == 0`).
#[test]
fn empty_builder_builds_empty_array() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<i64> := array_builder()\n  xs := b.build()\n  return xs.len() as i32\n}\n";
    let out = build_and_run("ab-empty", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Interleaved `push`/`append` preserves insertion order; appending an empty slice adds nothing.
#[test]
fn interleaved_push_append_preserves_order() {
    if !backend_available() {
        return;
    }
    // 1, [2,3,4], (empty append), 5 -> sum 15, len 5, first element 1.
    let src = "fn id(x: i64) -> i64 = x\nfn main() -> i32 {\n  mid := [2, 3, 4].map(id).to_array()\n  mut b: array_builder<i64> := array_builder()\n  b.push(1)\n  b.append(mid[..])\n  b.append(mid[0..0])\n  b.push(5)\n  xs := b.build()\n  return (xs.sum() + xs.len() + xs[0]) as i32\n}\n";
    let out = build_and_run("ab-append", src);
    assert_eq!(code(&out), Some(21), "stderr: {}", String::from_utf8_lossy(&out.stderr)); // 15 + 5 + 1
}

/// Amortized growth over many pushes stays correct (forces several reallocations).
#[test]
fn many_pushes_grow_correctly() {
    if !backend_available() {
        return;
    }
    // Push 0..1000 (forces several reallocations); sum = 499500. Printed (not returned) — a Unix
    // exit code wraps at 256, so a large result is checked via stdout.
    let src = "fn main() -> Result<(), Error> {\n  mut b: array_builder<i64> := array_builder()\n  mut i := 0\n  loop {\n    b.push(i)\n    i = i + 1\n    if i >= 1000 { break }\n  }\n  xs := b.build()\n  print(xs.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("ab-grow", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "499500\n");
}

// --- the two mandatory guardrails ---------------------------------------------------------------

/// MANDATORY (#402): a builder declared OUTSIDE a `loop` body, pushed INSIDE, built AFTER — it must
/// survive the loop's per-iteration drops (its `LocalId` is not in the loop's `body_locals` range,
/// so `loop_iter_drops` never frees it each pass). Sum of 0+1+2+3+4 = 10.
#[test]
fn builder_outside_loop_survives_per_iteration_drops() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<i64> := array_builder()\n  mut i := 0\n  loop {\n    b.push(i)\n    i = i + 1\n    if i >= 5 { break }\n  }\n  xs := b.build()\n  return xs.sum() as i32\n}\n";
    let out = build_and_run("ab-loop-outside", src);
    assert_eq!(code(&out), Some(10), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// MANDATORY: capturing an `array_builder` into a `spawn` closure is rejected (`ty_capture_is_move`
/// — an owned Move handle cannot be captured by value).
#[test]
fn capture_into_spawn_rejected() {
    let src = "fn main() -> Result<(), Error> {\n  mut b: array_builder<i64> := array_builder()\n  task_group {\n    a := spawn(fn { b.push(1); 1 })\n    wait()\n    print(a.get())\n  }\n  return Ok(())\n}\n";
    let diags = check_diagnostics("ab-cap-spawn", src);
    assert!(diags.contains("capture"), "expected a capture rejection, got:\n{diags}");
}

/// Capturing an `array_builder` into a pipeline `map` lambda is likewise rejected.
#[test]
fn capture_into_par_map_rejected() {
    let src = "fn main() -> Result<(), Error> {\n  mut b: array_builder<i64> := array_builder()\n  print([1, 2, 3].par_map(fn x { b.push(x); x }).sum())\n  return Ok(())\n}\n";
    assert!(check_errs("ab-cap-parmap", src));
}

// --- consume / move semantics --------------------------------------------------------------------

/// `build` consumes the builder: using it again is a moved-value error.
#[test]
fn build_consumes_use_after_is_moved() {
    let src = "fn main() -> i32 {\n  mut b: array_builder<i64> := array_builder()\n  b.push(1)\n  xs := b.build()\n  ys := b.build()\n  return (xs.len() + ys.len()) as i32\n}\n";
    let diags = check_diagnostics("ab-use-after-build", src);
    assert!(diags.contains("moved"), "expected a moved-value error, got:\n{diags}");
}

// --- string elements: move-in, deep-drop, reassignment -------------------------------------------

/// A `string` element builder: `push` MOVES each owned string in; `build` freezes into an
/// `array<string>` whose `.len()` reports the element count (move-element indexing is deferred
/// project-wide, so contents are checked by count — the read_dir `array<string>` precedent). Run
/// many cycles so the frozen `array<string>` deep-drop (each element buffer, then the header) is
/// exercised repeatedly without leaking/crashing.
#[test]
fn string_push_build_len_and_deep_drop_cycles() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut n := 0\n  mut c := 0\n  loop {\n    mut b: array_builder<string> := array_builder()\n    b.push(\"alpha\".clone())\n    b.push(\"beta\".clone())\n    b.push(\"gamma\".clone())\n    xs := b.build()\n    n = xs.len() as i32\n    c = c + 1\n    if c >= 2000 { break }\n  }\n  return n\n}\n";
    let out = build_and_run("ab-str-cycles", src);
    assert_eq!(code(&out), Some(3), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// The builder's OWN Drop deep-frees pushed-but-not-frozen strings: build an unfrozen string builder
/// each loop iteration and let it drop (no `build`). Over many cycles this exercises
/// `array_builder_free_strings` (deep-free each pushed string, then the storage) without leaking.
#[test]
fn unfrozen_string_builder_drop_frees_pushed_strings() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut c := 0\n  loop {\n    mut b: array_builder<string> := array_builder()\n    b.push(\"one\".clone())\n    b.push(\"two\".clone())\n    c = c + 1\n    if c >= 2000 { break }\n  }\n  return 0\n}\n";
    let out = build_and_run("ab-str-unfrozen-drop", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Reassigning a `mut` string builder frees the OLD builder (incl. its pushed strings) before the
/// new one takes the slot (same-region heap->heap `drop_old`). The final builder holds one element.
#[test]
fn reassignment_frees_old_string_builder() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  mut b: array_builder<string> := array_builder()\n  b.push(\"x\".clone())\n  b.push(\"y\".clone())\n  b = array_builder()\n  b.push(\"z\".clone())\n  xs := b.build()\n  return xs.len() as i32\n}\n";
    let out = build_and_run("ab-reassign", src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

// --- fail-closed sema errors ---------------------------------------------------------------------

/// A type-mismatched `push` is a clean sema error (not a silent coercion).
#[test]
fn type_mismatched_push_rejected() {
    let src = "fn main() -> i32 {\n  mut b: array_builder<i64> := array_builder()\n  b.push(true)\n  return 0\n}\n";
    assert!(check_errs("ab-push-mismatch", src));
}

/// A non-`mut` builder cannot be grown (`push` mutates in place).
#[test]
fn push_on_immutable_builder_rejected() {
    let src = "fn main() -> i32 {\n  b: array_builder<i64> := array_builder()\n  b.push(1)\n  return 0\n}\n";
    assert!(check_errs("ab-push-immutable", src));
}

/// `append` is unavailable on a `string` builder (a borrowed `slice<string>` cannot be bulk-moved;
/// strings are added one at a time via `push`, which moves them in).
#[test]
fn append_on_string_builder_rejected() {
    let src = "fn main() -> i32 {\n  mut b: array_builder<string> := array_builder()\n  names: array<string> := [\"a\".clone()]\n  b.append(names[..])\n  return 0\n}\n";
    assert!(check_errs("ab-str-append", src));
}

/// An `array_builder<str>` (a view element) is rejected at the type argument — fail-closed to the
/// settled v1 element set (Copy scalars + owned `string`).
#[test]
fn str_view_element_rejected_at_type() {
    let src = "fn main() -> i32 {\n  mut b: array_builder<str> := array_builder()\n  return 0\n}\n";
    assert!(check_errs("ab-str-view-elem", src));
}

/// Constructing without an inferable element type is a clean error (annotate the binding).
#[test]
fn uninferable_element_type_rejected() {
    let src = "fn main() -> i32 {\n  b := array_builder()\n  return 0\n}\n";
    assert!(check_errs("ab-no-infer", src));
}
