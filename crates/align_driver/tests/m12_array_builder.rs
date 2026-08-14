//! M12 Slice A6 — the growable `array_builder<T>`: the third grow-then-freeze member
//! (`builder`->`string`, `buffer`->bytes, this->`array<T>`). `array_builder()` opens an empty
//! builder; `b.push(v)` / `b.append(xs: slice<T>)` grow it in place (a `mut`-bound local); `b.build()`
//! **consumes** it into an owned `array<T>` (a zero-copy ptr+len retype over `align_rt_realloc`
//! storage). The heap form accepts Copy scalars, `string`, and the closed declared-record subset;
//! push moves a string or Move record in, and the builder's own Drop recursively frees every
//! pushed-not-frozen owner. Move-handle exclusions (no aggregate riding, capture into par_map/spawn
//! rejected). (`docs/impl/07-roadmap.md` M12 Slice A6; `draft.md` §18.2.)

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

// --- declared-record elements -------------------------------------------------------------------

#[test]
fn copy_record_push_build_zero_one_many_and_realloc() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Point { x: i64, y: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut empty: array_builder<Point> := array_builder()\n",
        "  zs := empty.build()\n",
        "  mut b: array_builder<Point> := array_builder()\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    b.push(Point{x: i, y: i + 1})\n",
        "    i = i + 1\n",
        "    if i >= 80 { break }\n",
        "  }\n",
        "  xs := b.build()\n",
        "  return (zs.len() + xs.len() + xs[0].x + xs[79].y) as i32\n",
        "}\n",
    );
    let out = build_and_run("ab-record-copy", src);
    assert_eq!(code(&out), Some(160), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn move_record_push_nulls_source_and_build_deep_drops() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Name { text: string, score: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Name> := array_builder()\n",
        "  first := Name{text: \"alpha\".clone(), score: 7}\n",
        "  b.push(first)\n",
        "  b.push(Name{text: \"be\\0ta\".clone(), score: 9})\n",
        "  xs := b.build()\n",
        "  return (xs.len() + xs[0].score + xs[1].score) as i32\n",
        "}\n",
    );
    let out = build_and_run("ab-record-move", src);
    assert_eq!(code(&out), Some(18), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn unfinished_move_record_builder_drops_initialized_prefix() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Name { text: string }\n",
        "fn main() -> i32 {\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    mut b: array_builder<Name> := array_builder()\n",
        "    b.push(Name{text: \"owned\".clone()})\n",
        "    i = i + 1\n",
        "    if i >= 2000 { break }\n",
        "  }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("ab-record-unfinished", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn record_builder_abandonment_all_exit_kinds() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "import core.json\n",
        "Item { name: string, value: i64 }\n",
        "Parsed { value: i64 }\n",
        "Choice { Left, Right }\n",
        "LoadErr { Bad }\n",
        "fn item(value: i64) -> Item = Item{name: \"owned\".clone(), value: value}\n",
        "fn fail() -> Result<i32, Error> = Err(Error.Code(1))\n",
        "fn custom_fail() -> Result<i32, LoadErr> = Err(LoadErr.Bad)\n",
        "fn to_error(e: LoadErr) -> Error = match e { Bad => Error.Code(2) }\n",
        "fn relay(items: array_builder<Item>) -> array_builder<Item> = items\n",
        "fn normal() -> i32 { mut b: array_builder<Item> := array_builder()\n b.push(item(1))\n return 0 }\n",
        "fn early() -> i32 { mut b: array_builder<Item> := array_builder()\n b.push(item(2))\n return 0 }\n",
        "fn tried() -> Result<i32, Error> { mut b: array_builder<Item> := array_builder()\n b.push(item(3))\n n := fail()?\n return Ok(n) }\n",
        "fn mapped() -> Result<i32, Error> { mut b: array_builder<Item> := array_builder()\n b.push(item(4))\n n := custom_fail().map_err(to_error)?\n return Ok(n) }\n",
        "fn unwrapped() -> i32 { mut b: array_builder<Item> := array_builder()\n b.push(item(5))\n n := fail() else { return 0 }\n return n }\n",
        "fn branched(flag: bool) -> i32 { if flag { mut b: array_builder<Item> := array_builder()\n b.push(item(6)) } else { mut b: array_builder<Item> := array_builder()\n b.push(item(7)) }\n return 0 }\n",
        "fn matched() -> i32 { choice := Choice.Left\n match choice { Left => { mut b: array_builder<Item> := array_builder()\n b.push(item(8)) }, Right => { mut b: array_builder<Item> := array_builder()\n b.push(item(9)) } }\n return 0 }\n",
        "fn looped() -> i32 { mut n := 0\n loop { mut b: array_builder<Item> := array_builder()\n b.push(item(n))\n n = n + 1\n if n >= 8 { break } }\n return 0 }\n",
        "fn boxed() -> i32 { mut b: array_builder<Item> := array_builder()\n b.push(item(10))\n abandoned := relay(b)\n return 0 }\n",
        "fn malformed() -> Result<i32, Error> { mut b: array_builder<Item> := array_builder()\n b.push(item(11))\n parsed: Parsed := json.decode(\"{\")?\n return Ok(parsed.value as i32) }\n",
        "fn main() -> i32 {\n",
        "  mut total := normal() + early() + unwrapped() + branched(true) + branched(false) + matched() + looped() + boxed()\n",
        "  total = total + match tried() { Ok(value) => value, Err(_) => 0 }\n",
        "  total = total + match mapped() { Ok(value) => value, Err(_) => 0 }\n",
        "  total = total + match malformed() { Ok(value) => value, Err(_) => 0 }\n",
        "  return total\n",
        "}\n",
    );
    let out = build_and_run("ab-record-exits", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn record_builder_partial_element_and_enclosing_record_cleanup() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Item { first: string, second: string }\n",
        "Envelope { items: array<Item>, tail: string }\n",
        "fn fail_string() -> Result<string, Error> = Err(Error.Code(1))\n",
        "fn partial() -> Result<i32, Error> {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  b.push(Item{first: \"kept\".clone(), second: \"prefix\".clone()})\n",
        "  b.push(Item{first: \"partial\".clone(), second: fail_string()?})\n",
        "  return Ok(b.build().len() as i32)\n",
        "}\n",
        "fn enclosing() -> Result<Envelope, Error> {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  b.push(Item{first: \"built\".clone(), second: \"owner\".clone()})\n",
        "  return Ok(Envelope{items: b.build(), tail: fail_string()?})\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  mut n := 0\n",
        "  loop {\n",
        "    a := match partial() { Ok(value) => value, Err(_) => 0 }\n",
        "    b := match enclosing() { Ok(_) => 1, Err(_) => 0 }\n",
        "    n = n + 1 + a + b\n",
        "    if n >= 1000 { break }\n",
        "  }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("ab-record-partial-cleanup", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn record_builder_move_source_matrix() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Item { name: string, value: i64 }\n",
        "Choice { Left, Right }\n",
        "LoadErr { Bad }\n",
        "fn make(value: i64) -> Item = Item{name: \"call\".clone(), value: value}\n",
        "fn maybe(value: i64) -> Result<Item, Error> = Ok(make(value))\n",
        "fn load(value: i64) -> Result<Item, LoadErr> = Ok(make(value))\n",
        "fn to_error(e: LoadErr) -> Error = match e { Bad => Error.Code(1) }\n",
        "fn collect(flag: bool) -> Result<array<Item>, Error> {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  local := Item{name: \"local\".clone(), value: 1}\n",
        "  b.push(local)\n",
        "  b.push(Item{name: \"fresh\".clone(), value: 2})\n",
        "  b.push(make(3))\n",
        "  b.push(if flag { make(4) } else { make(40) })\n",
        "  choice := Choice.Left\n",
        "  b.push(match choice { Left => make(5), Right => make(50) })\n",
        "  b.push(maybe(6)?)\n",
        "  b.push(load(7).map_err(to_error)?)\n",
        "  b.push(maybe(8) else { return Err(Error.Code(2)) })\n",
        "  b.push({ make(9) })\n",
        "  b.push(if flag { make(10) } else { return Err(Error.Code(3)) })\n",
        "  return Ok(b.build())\n",
        "}\n",
        "fn main() -> Result<(), Error> {\n",
        "  xs := collect(true)?\n",
        "  print(xs[0].value + xs[1].value + xs[2].value + xs[3].value + xs[4].value + xs[5].value + xs[6].value + xs[7].value + xs[8].value + xs[9].value)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("ab-record-source-matrix", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "55\n");
}

#[test]
fn record_builder_source_use_after_push_and_borrowed_source_rejected() {
    let moved = concat!(
        "Item { name: string }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  item := Item{name: \"owned\".clone()}\n",
        "  b.push(item)\n",
        "  b.push(item)\n",
        "  return 0\n",
        "}\n",
    );
    assert!(check_diagnostics("ab-record-source-moved", moved).contains("moved"));

    let borrowed = concat!(
        "Item { name: string }\n",
        "fn retain(borrow item: Item) {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  b.push(item)\n",
        "}\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(check_diagnostics("ab-record-source-borrowed", borrowed).contains("borrow"));

    let projection = concat!(
        "Item { name: string }\n",
        "Outer { item: Item }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  outer := Outer{item: Item{name: \"owned\".clone()}}\n",
        "  b.push(outer.item)\n",
        "  return 0\n",
        "}\n",
    );
    assert!(check_errs("ab-record-source-projection", projection));

    let divergent = concat!(
        "Item { name: string }\n",
        "Other { name: string }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  b.push(if true { Item{name: \"item\".clone()} } else { Other{name: \"other\".clone()} })\n",
        "  return 0\n",
        "}\n",
    );
    assert!(check_errs("ab-record-source-divergent", divergent));

    let consumed_arm = concat!(
        "Item { name: string }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  item := Item{name: \"owned\".clone()}\n",
        "  b.push(item)\n",
        "  b.push(if true { item } else { Item{name: \"fresh\".clone()} })\n",
        "  return 0\n",
        "}\n",
    );
    assert!(check_errs("ab-record-source-consumed-arm", consumed_arm));
}

#[test]
fn record_builder_by_value_parameter_return_and_borrow_mut() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Item { name: string, value: i64 }\n",
        "fn relay(items: array_builder<Item>) -> array_builder<Item> = items\n",
        "fn add(borrow mut items: array_builder<Item>, item: Item) { items.push(item) }\n",
        "fn main() -> i32 {\n",
        "  mut first: array_builder<Item> := array_builder()\n",
        "  first.push(Item{name: \"one\".clone(), value: 1})\n",
        "  mut second := relay(first)\n",
        "  add(second, Item{name: \"two\".clone(), value: 2})\n",
        "  values := second.build()\n",
        "  return (values.len() + values[0].value + values[1].value) as i32\n",
        "}\n",
    );
    let out = build_and_run("ab-record-boundary", src);
    assert_eq!(code(&out), Some(5), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn nested_move_record_reallocation_and_reassignment_drop_once() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Inner { name: string }\n",
        "Outer { inner: Inner, value: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Outer> := array_builder()\n",
        "  b.push(Outer{inner: Inner{name: \"old\".clone()}, value: 99})\n",
        "  b = array_builder()\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    b.push(Outer{inner: Inner{name: \"live\".clone()}, value: i})\n",
        "    i = i + 1\n",
        "    if i >= 80 { break }\n",
        "  }\n",
        "  values := b.build()\n",
        "  return (values.len() + values[79].value) as i32\n",
        "}\n",
    );
    let out = build_and_run("ab-record-nested-reassign", src);
    assert_eq!(code(&out), Some(159), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn record_builder_closed_shape_and_append_rejections() {
    let cases = [
        ("view", "Bad { value: str }"),
        ("array", "Bad { value: array<i64> }"),
        ("option", "Bad { value: Option<i64> }"),
        ("empty", "Bad {}"),
        ("c-layout", "layout(C) Bad { value: i64 }"),
        ("aligned", "align(16) Bad { value: i64 }"),
    ];
    for (name, definition) in cases {
        let src = format!(
            "{definition}\nfn main() -> i32 {{\n  mut b: array_builder<Bad> := array_builder()\n  return 0\n}}\n"
        );
        assert!(check_errs(&format!("ab-record-reject-{name}"), &src), "{name} was accepted");
    }

    let append = concat!(
        "Point { x: i64 }\n",
        "fn main() -> i32 {\n",
        "  values := [Point{x: 1}]\n",
        "  mut b: array_builder<Point> := array_builder()\n",
        "  b.append(values[..])\n",
        "  return 0\n",
        "}\n",
    );
    assert!(check_errs("ab-record-append", append));
}

#[test]
fn record_builder_nominal_twins_remain_distinct() {
    if !backend_available() {
        return;
    }
    let positive = concat!(
        "Left { value: i64 }\n",
        "Right { value: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut left: array_builder<Left> := array_builder()\n",
        "  mut right: array_builder<Right> := array_builder()\n",
        "  left.push(Left{value: 1})\n",
        "  right.push(Right{value: 2})\n",
        "  xs := left.build()\n",
        "  ys := right.build()\n",
        "  return (xs[0].value + ys[0].value) as i32\n",
        "}\n",
    );
    let out = build_and_run("ab-record-nominal-twins", positive);
    assert_eq!(code(&out), Some(3), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let wrong = concat!(
        "Left { value: i64 }\n",
        "Right { value: i64 }\n",
        "fn main() -> i32 {\n",
        "  mut left: array_builder<Left> := array_builder()\n",
        "  left.push(Right{value: 1})\n",
        "  return 0\n",
        "}\n",
    );
    assert!(check_errs("ab-record-nominal-twin-mismatch", wrong));
}

#[test]
fn record_builder_invalid_storage_capture_and_borrowed_consumption_rejected() {
    let aggregate = concat!(
        "Item { name: string }\n",
        "Holder { items: array_builder<Item> }\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(check_errs("ab-record-aggregate-storage", aggregate));

    let borrowed_build = concat!(
        "Item { name: string }\n",
        "fn consume(borrow mut items: array_builder<Item>) { values := items.build() }\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(check_errs("ab-record-borrowed-build", borrowed_build));

    let capture = concat!(
        "Item { name: string }\n",
        "fn main() -> Result<(), Error> {\n",
        "  mut items: array_builder<Item> := array_builder()\n",
        "  task_group { task := spawn(fn { items.push(Item{name: \"x\".clone()}); 1 })\n",
        "    wait()\n",
        "    print(task.get())\n",
        "  }\n",
        "  return Ok(())\n",
        "}\n",
    );
    assert!(check_errs("ab-record-capture", capture));
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
/// closed heap element set (Copy scalars, owned `string`, and view-free declared records).
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
