//! M12 Slice A6 — the growable `array_builder<T>`: the third grow-then-freeze member
//! (`builder`->`string`, `buffer`->bytes, this->`array<T>`). `array_builder()` opens an empty
//! builder; `b.push(v)` / `b.append(xs: slice<T>)` grow it in place (a `mut`-bound local); `b.build()`
//! **consumes** it into an owned `array<T>` (a zero-copy ptr+len retype over `align_rt_realloc`
//! storage). Heap elements are primitive Copy scalars, `string`, or a closed declared record; push
//! moves strings and Move records, and unfinished builders deep-drop their initialized prefix.
//! Move-handle exclusions and capture restrictions remain unchanged.

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

// --- closed heap records ------------------------------------------------------------------------

/// Copy records use the same zero-copy AoS buffer for empty, singleton, and repeatedly grown
/// builders. Reading fields after several reallocations verifies both stride and natural alignment.
#[test]
fn copy_record_push_build_zero_one_many_and_realloc() {
    if !backend_available() {
        return;
    }
    let src = "Point { x: i32, y: i32 }\nfn main() -> i32 {\n  mut empty: array_builder<Point> := array_builder()\n  e := empty.build()\n  mut one: array_builder<Point> := array_builder()\n  one.push(Point { x: 7, y: 9 })\n  a := one.build()\n  mut many: array_builder<Point> := array_builder()\n  mut i: i32 := 0\n  loop {\n    many.push(Point { x: i, y: i + 1 })\n    i = i + 1\n    if i >= 100 { break }\n  }\n  xs := many.build()\n  return e.len() as i32 + a[0].x + xs[99].y\n}\n";
    let out = build_and_run("ab-copy-record", src);
    assert_eq!(code(&out), Some(107), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// A nested Move record transfers every reachable string into the builder, survives growth, and
/// becomes an ordinary `array<S>` whose Drop recursively frees each element.
#[test]
fn record_builder_realloc_preserves_nested_owners() {
    if !backend_available() {
        return;
    }
    let src = "Text { value: string }\nRow { id: i32, text: Text }\nfn main() -> i32 {\n  mut b: array_builder<Row> := array_builder()\n  mut i: i32 := 0\n  loop {\n    b.push(Row { id: i, text: Text { value: \"owned\".clone() } })\n    i = i + 1\n    if i >= 100 { break }\n  }\n  rows := b.build()\n  return rows.len() as i32\n}\n";
    let out = build_and_run("ab-move-record-build", src);
    assert_eq!(code(&out), Some(100), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Dropping an unfinished Move-record builder runs the same recursive element DropPlan as the
/// frozen array path. Repetition exercises both the initialized prefix and header cleanup.
#[test]
fn unfrozen_move_record_builder_deep_drops_initialized_prefix() {
    if !backend_available() {
        return;
    }
    let src = "Owned { text: string }\nfn main() -> i32 {\n  mut i := 0\n  loop {\n    mut b: array_builder<Owned> := array_builder()\n    b.push(Owned { text: \"left\".clone() })\n    b.push(Owned { text: \"right\".clone() })\n    i = i + 1\n    if i >= 2000 { break }\n  }\n  return 0\n}\n";
    let out = build_and_run("ab-move-record-unfrozen", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Reassignment deep-drops the old initialized prefix before the local receives a fresh builder.
#[test]
fn record_builder_reassignment_drops_old_storage() {
    if !backend_available() {
        return;
    }
    let src = "Owned { text: string }\nfn main() -> i32 {\n  mut b: array_builder<Owned> := array_builder()\n  b.push(Owned { text: \"old-a\".clone() })\n  b.push(Owned { text: \"old-b\".clone() })\n  b = array_builder()\n  b.push(Owned { text: \"new\".clone() })\n  return b.build().len() as i32\n}\n";
    let out = build_and_run("ab-record-reassign", src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Record builders preserve the existing owner model across `borrow mut`, by-value transfer, and
/// return. The call-crossing header is boxed while element ownership remains exactly once.
#[test]
fn record_builder_by_value_parameter_return_and_borrow_mut() {
    if !backend_available() {
        return;
    }
    let src = "Owned { text: string }\nfn add(borrow mut b: array_builder<Owned>, text: string) {\n  b.push(Owned { text: text })\n}\nfn pass(b: array_builder<Owned>) -> array_builder<Owned> = b\nfn main() -> i32 {\n  mut b: array_builder<Owned> := array_builder()\n  add(b, \"first\".clone())\n  mut c := pass(b)\n  add(c, \"second\".clone())\n  return c.build().len() as i32\n}\n";
    let out = build_and_run("ab-record-functions", src);
    assert_eq!(code(&out), Some(2), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let consumes_borrow = "Owned { text: string }\nfn finish(borrow mut b: array_builder<Owned>) -> array<Owned> = b.build()\nfn main() -> i32 = 0\n";
    assert!(check_errs("ab-record-consume-borrow", consumes_borrow));
}

/// A boxed header abandoned by a by-value callee drains its initialized Move-record prefix.
#[test]
fn boxed_record_builder_abandonment_deep_drops() {
    if !backend_available() {
        return;
    }
    let src = "Owned { text: string }\nfn abandon(b: array_builder<Owned>) {}\nfn main() -> i32 {\n  mut i := 0\n  loop {\n    mut b: array_builder<Owned> := array_builder()\n    b.push(Owned { text: \"boxed\".clone() })\n    abandon(b)\n    i = i + 1\n    if i >= 2000 { break }\n  }\n  return 0\n}\n";
    let out = build_and_run("ab-record-boxed-abandon", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Stack-local unfinished builders clean their initialized prefix on early return, propagated
/// error, branch/match joins, and loop back-edges/breaks.
#[test]
fn record_builder_abandonment_all_exit_kinds() {
    if !backend_available() {
        return;
    }
    let src = "Tag { A, B }\nOwned { text: string }\nfn early_return() -> i32 {\n  mut b: array_builder<Owned> := array_builder()\n  b.push(Owned { text: \"return\".clone() })\n  return 1\n}\nfn fail() -> Result<(), Error> = Err(Error.Code(7))\nfn early_error() -> Result<(), Error> {\n  mut b: array_builder<Owned> := array_builder()\n  b.push(Owned { text: \"error\".clone() })\n  fail()?\n  return Ok(())\n}\nfn branch(flag: bool) {\n  if flag {\n    mut b: array_builder<Owned> := array_builder()\n    b.push(Owned { text: \"branch-a\".clone() })\n  } else {\n    mut b: array_builder<Owned> := array_builder()\n    b.push(Owned { text: \"branch-b\".clone() })\n  }\n}\nfn matched(tag: Tag) {\n  match tag {\n    A => { mut b: array_builder<Owned> := array_builder(); b.push(Owned { text: \"match-a\".clone() }) }\n    B => { mut b: array_builder<Owned> := array_builder(); b.push(Owned { text: \"match-b\".clone() }) }\n  }\n}\nfn repeated() {\n  mut i := 0\n  loop {\n    mut b: array_builder<Owned> := array_builder()\n    b.push(Owned { text: \"loop\".clone() })\n    i = i + 1\n    if i >= 20 { break }\n  }\n}\nfn main() -> i32 {\n  n := early_return()\n  result := early_error()\n  branch(true)\n  branch(false)\n  matched(Tag.A)\n  matched(Tag.B)\n  repeated()\n  return match result { Ok(_) => 0, Err(_) => n }\n}\n";
    let out = build_and_run("ab-record-abandonment", src);
    assert_eq!(code(&out), Some(1), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Failure while constructing the next Move record remains source-aggregate cleanup and cannot
/// increment the builder's initialized length.
#[test]
fn record_builder_partial_element_failure_drops_fields() {
    if !backend_available() {
        return;
    }
    let src = "Pair { left: string, right: string }\nfn fail() -> Result<string, Error> = Err(Error.Code(9))\nfn attempt(borrow mut b: array_builder<Pair>) -> Result<(), Error> {\n  b.push(Pair { left: \"constructed\".clone(), right: fail()? })\n  return Ok(())\n}\nfn main() -> i32 {\n  mut b: array_builder<Pair> := array_builder()\n  result := attempt(b)\n  n := b.build().len() as i32\n  return match result { Ok(_) => 10, Err(_) => n }\n}\n";
    let out = build_and_run("ab-record-partial", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// Push consumes a Move record as a complete value, just like pushing a `string`.
#[test]
fn move_record_push_consumes_source() {
    let src = "Owned { text: string }\nfn main() -> i32 {\n  value := Owned { text: \"owned\".clone() }\n  mut b: array_builder<Owned> := array_builder()\n  b.push(value)\n  print(value.text)\n  return 0\n}\n";
    let diags = check_diagnostics("ab-move-record-source", src);
    assert!(diags.contains("moved"), "expected a moved-value error, got:\n{diags}");
}

/// Complete Move-record rvalues transfer through every established value-producing source shape.
/// Each selected source becomes exactly one initialized builder element.
#[test]
fn record_builder_move_source_matrix() {
    if !backend_available() {
        return;
    }
    let src = "MyErr { Bad }\nChoice { A, B }\nOwned { text: string }\nfn make(text: str) -> Owned = Owned { text: text.clone() }\nfn load() -> Result<Owned, Error> = Ok(make(\"try\"))\nfn load_custom() -> Result<Owned, MyErr> = Ok(make(\"map-err\"))\nfn to_error(error: MyErr) -> Error = Error.Code(1)\nfn main() -> Result<(), Error> {\n  mut b: array_builder<Owned> := array_builder()\n  local := make(\"local\")\n  b.push(local)\n  b.push(Owned { text: \"literal\".clone() })\n  b.push(make(\"result\"))\n  b.push({ Owned { text: \"block\".clone() } })\n  b.push(if true { make(\"if-a\") } else { make(\"if-b\") })\n  b.push(match Choice.A { A => make(\"match-a\"), B => make(\"match-b\") })\n  option: Option<Owned> := Some(make(\"option\"))\n  b.push(option else make(\"fallback\"))\n  b.push(load()?)\n  b.push(load_custom().map_err(to_error)?)\n  print(b.build().len())\n  return Ok(())\n}\n";
    let out = build_and_run("ab-record-source-matrix", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "9\n");

    let borrowed = "Owned { text: string }\nfn push_borrow(borrow value: Owned, borrow mut b: array_builder<Owned>) { b.push(value) }\nfn main() -> i32 = 0\n";
    assert!(check_errs("ab-record-borrowed-source", borrowed));

    let field = "Owned { text: string }\nOuter { value: Owned }\nfn main() -> i32 {\n  outer := Outer { value: Owned { text: \"field\".clone() } }\n  mut b: array_builder<Owned> := array_builder()\n  b.push(outer.value)\n  return 0\n}\n";
    assert!(check_errs("ab-record-field-source", field));
}

/// Heap records are intentionally closed: views, explicit layout attributes, and empty records do
/// not acquire an accidental byte-copy ABI through `array_builder`.
#[test]
fn record_builder_field_predicate_rejects_closed_shapes() {
    for (name, ty, declaration) in [
        ("view", "View", "View { text: str }"),
        ("empty", "Empty", "Empty { }"),
        ("aligned", "Aligned", "align(16) Aligned { value: i64 }"),
        ("c-layout", "CRow", "layout(C) CRow { value: i64 }"),
    ] {
        let src = format!("{declaration}\nfn main() -> i32 {{\n  mut b: array_builder<{ty}> := array_builder()\n  return 0\n}}\n");
        assert!(check_errs(&format!("ab-record-reject-{name}"), &src), "{name} record unexpectedly admitted");
    }
}

/// `append` keeps its primitive Copy-scalar contract; record elements use `push`, which preserves
/// the complete-value ownership rule.
#[test]
fn record_builder_append_is_rejected() {
    let src = "Point { x: i32 }\nfn main() -> i32 {\n  points := [Point { x: 1 }]\n  mut b: array_builder<Point> := array_builder()\n  b.append(points[..])\n  return 0\n}\n";
    assert!(check_errs("ab-record-append", src));
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
