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
        "Item { first: string, tags: array<string>, optional: Option<string>, second: string }\n",
        "Envelope { items: array<Item>, tail: string }\n",
        "fn tags() -> array<string> { mut b: array_builder<string> := array_builder()\n",
        "  b.push(\"tag\".clone())\n",
        "  return b.build() }\n",
        "fn fail_string() -> Result<string, Error> = Err(Error.Code(1))\n",
        "fn partial() -> Result<i32, Error> {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  b.push(Item{first: \"kept\".clone(), tags: tags(), optional: Some(\"optional\".clone()), second: \"prefix\".clone()})\n",
        "  b.push(Item{first: \"partial\".clone(), tags: tags(), optional: None, second: fail_string()?})\n",
        "  return Ok(b.build().len() as i32)\n",
        "}\n",
        "fn enclosing() -> Result<Envelope, Error> {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  b.push(Item{first: \"built\".clone(), tags: tags(), optional: Some(\"nested\".clone()), second: \"owner\".clone()})\n",
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
        "Item { name: string, tags: array<string>, maybe: Option<string>, value: i64 }\n",
        "Choice { Left, Right }\n",
        "LoadErr { Bad }\n",
        "fn tags() -> array<string> { mut b: array_builder<string> := array_builder()\n",
        "  b.push(\"tag\".clone())\n",
        "  return b.build() }\n",
        "fn make(value: i64) -> Item = Item{name: \"call\".clone(), tags: tags(), maybe: Some(\"optional\".clone()), value: value}\n",
        "fn maybe(value: i64) -> Result<Item, Error> = Ok(make(value))\n",
        "fn load(value: i64) -> Result<Item, LoadErr> = Ok(make(value))\n",
        "fn to_error(e: LoadErr) -> Error = match e { Bad => Error.Code(1) }\n",
        "fn collect(flag: bool) -> Result<array<Item>, Error> {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  local := Item{name: \"local\".clone(), tags: tags(), maybe: None, value: 1}\n",
        "  b.push(local)\n",
        "  b.push(Item{name: \"fresh\".clone(), tags: tags(), maybe: Some(\"fresh-optional\".clone()), value: 2})\n",
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

    let recursive_moved = concat!(
        "Item { names: array<string>, maybe: Option<string> }\n",
        "fn names() -> array<string> { mut b: array_builder<string> := array_builder()\n",
        "  b.push(\"name\".clone())\n",
        "  return b.build() }\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Item> := array_builder()\n",
        "  item := Item{names: names(), maybe: Some(\"owned\".clone())}\n",
        "  b.push(item)\n",
        "  b.push(item)\n",
        "  return 0\n",
        "}\n",
    );
    assert!(
        check_diagnostics("ab-recursive-source-moved", recursive_moved).contains("moved")
    );

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
fn recursive_record_arena_owned_paths_reject_before_heap_push() {
    let direct = concat!(
        "Item { values: array<i64> }\n",
        "fn main() -> i32 {\n",
        "  mut heap: array_builder<Item> := array_builder()\n",
        "  arena out {\n",
        "    mut values: array_builder<i64> := array_builder(out)\n",
        "    values.push(1)\n",
        "    heap.push(Item{values: values.build()})\n",
        "  }\n",
        "  return 0\n",
        "}\n",
    );
    assert!(
        check_diagnostics("ab-recursive-arena-direct", direct)
            .contains("cannot push a record with arena-owned")
    );

    let optional = concat!(
        "Item { values: Option<array<i64>> }\n",
        "fn main() -> i32 {\n",
        "  mut heap: array_builder<Item> := array_builder()\n",
        "  arena out {\n",
        "    mut values: array_builder<i64> := array_builder(out)\n",
        "    values.push(1)\n",
        "    heap.push(Item{values: Some(values.build())})\n",
        "  }\n",
        "  return 0\n",
        "}\n",
    );
    assert!(
        check_diagnostics("ab-recursive-arena-option", optional)
            .contains("cannot push a record with arena-owned")
    );

    let array_element = concat!(
        "Leaf { values: array<i64> }\n",
        "Item { leaves: array<Leaf> }\n",
        "fn main() -> i32 {\n",
        "  mut heap: array_builder<Item> := array_builder()\n",
        "  arena out {\n",
        "    mut values: array_builder<i64> := array_builder(out)\n",
        "    values.push(1)\n",
        "    mut leaves: array_builder<Leaf> := array_builder(out)\n",
        "    leaves.push(Leaf{values: values.build()})\n",
        "    heap.push(Item{leaves: leaves.build()})\n",
        "  }\n",
        "  return 0\n",
        "}\n",
    );
    assert!(
        check_diagnostics("ab-recursive-arena-array-element", array_element)
            .contains("cannot push a record with arena-owned")
    );
}

#[test]
fn record_builder_by_value_parameter_return_and_borrow_mut() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Item { name: string, tags: array<string>, maybe: Option<string>, value: i64 }\n",
        "fn tags() -> array<string> { mut b: array_builder<string> := array_builder()\n",
        "  b.push(\"tag\".clone())\n",
        "  return b.build() }\n",
        "fn item(name: str, value: i64) -> Item = Item{name: name.clone(), tags: tags(), maybe: Some(\"optional\".clone()), value: value}\n",
        "fn relay(items: array_builder<Item>) -> array_builder<Item> = items\n",
        "fn add(borrow mut items: array_builder<Item>, item: Item) { items.push(item) }\n",
        "fn main() -> i32 {\n",
        "  mut first: array_builder<Item> := array_builder()\n",
        "  first.push(item(\"one\", 1))\n",
        "  mut second := relay(first)\n",
        "  add(second, item(\"two\", 2))\n",
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
fn recursive_heap_tree_record_reallocation_build_and_drop() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Leaf { name: string, tags: array<string> }\n",
        "Branch { maybe: Option<Leaf>, names: array<string>, nums: array<i64>, leaves: array<Leaf> }\n",
        "fn strings(prefix: str) -> array<string> {\n",
        "  mut b: array_builder<string> := array_builder()\n",
        "  b.push(prefix.clone())\n",
        "  b.push(\"tail\".clone())\n",
        "  return b.build()\n",
        "}\n",
        "fn leaf(name: str) -> Leaf = Leaf{name: name.clone(), tags: strings(\"tag\")}\n",
        "fn leaves() -> array<Leaf> {\n",
        "  mut b: array_builder<Leaf> := array_builder()\n",
        "  b.push(leaf(\"first\"))\n",
        "  b.push(leaf(\"second\"))\n",
        "  return b.build()\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  mut b: array_builder<Branch> := array_builder()\n",
        "  mut i := 0\n",
        "  loop {\n",
        "    b.push(Branch{maybe: Some(leaf(\"optional\")), names: strings(\"name\"), nums: [1, 2, 3].to_array(), leaves: leaves()})\n",
        "    i = i + 1\n",
        "    if i >= 80 { break }\n",
        "  }\n",
        "  values := b.build()\n",
        "  if values.len() != 80 { return 1 }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("ab-recursive-heap-tree", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

const REQUEST10_C6_SOURCE: &str = r#"
ArtifactExpectation { path: string, kind: string, expected_sha256: string }
ArtifactDigest { path: string, mode: string, byte_count: i64, sha256: string }
RegressionLimits { maximum_unrelated_diff_count: i64, maximum_patch_size_bytes: i64, maximum_public_api_change_count: i64, maximum_repair_loops: i64, maximum_benchmark_regression_ppm: Option<i64> }
EnvironmentProbe { schema_version: i64, artifact_kind: string, producer: string, os: string, os_release: string, architecture: string, cpu: string, logical_cpu_count: Option<i64>, gpu: string, runtime_identity: string, content_sha256: string }
EvaluationInputIdentity { schema_version: i64, artifact_kind: string, task_id: string, task_input_snapshot_sha256: string, parent_variant_sha256: string, candidate_variant_sha256: string, task_prompt_sha256: string, context_sources_sha256: string, generation_policy_sha256: string, generation_request_sha256: string, adapter_request_sha256: string, environment_policy_sha256: string, environment_sha256: string, sample_index: i64, paired_seed: i64, content_sha256: string }
GenerationRequestIdentity { schema_version: i64, artifact_kind: string, rendered_prompt_sha256: string, system_text_sha256: string, user_text_sha256: string, generation_policy_sha256: string, provider_control_sha256: string, environment_policy_sha256: string, max_tokens: i64, temperature_micros: i64, paired_seed: i64, provider_request_sha256: string, seed_attestation_sha256: string, content_sha256: string }
SeedCapabilityAttestation { schema_version: i64, artifact_kind: string, provider_kind: string, provider_model: string, requested_seed: i64, result: string, applied_seed: Option<i64>, provider_request_sha256: string, content_sha256: string }
TaskMeasurement { schema_version: i64, artifact_kind: string, status: string, failure_kind: string, build_status: string, test_status: string, repair_loop_count: i64, unrelated_diff_count: i64, patch_size_bytes: i64, public_api_change_count: i64, policy_violation_count: i64, cleanup_passed: bool, containment_passed: bool, benchmark_regression_ppm: Option<i64>, generation_to_passing_patch_ns: Option<i64>, rendered_prompt_sha256: string, generation_request: GenerationRequestIdentity, environment_probe: EnvironmentProbe, seed_attestation: SeedCapabilityAttestation, diagnostic_summary: string, diagnostic_stdout: string, diagnostic_stderr: string, content_sha256: string }
SnapshotRequest { schema_version: i64, artifact_kind: string, task_id: string, project_root: string, repo_path: string, repo_revision: string, require_clean_repo: bool, static_expectations: array<ArtifactExpectation>, additional_files: array<string>, workspace_path: string, allowed_workspace_entries: array<string>, content_sha256: string }
PromptEvaluationTask { schema_version: i64, artifact_kind: string, task_id: string, repo_id: string, repo_revision: string, repo_path: string, require_clean_repo: bool, cmd: string, argv: array<string>, snapshot_cmd: string, snapshot_argv: array<string>, measurement_adapter_runtime: string, snapshot_helper_runtime: string, cwd: string, timeout_ns: i64, task_prompt_path: string, context_sources_path: string, generation_policy_path: string, provider_control_path: string, environment_policy_path: string, artifacts: array<ArtifactExpectation>, regression_limits: RegressionLimits, content_sha256: string }
PromptTaskRow { schema_version: i64, artifact_kind: string, evaluation_id: string, task_id: string, sample_index: i64, variant: string, variant_id: string, variant_sha256: string, prompt_preparation_ns: i64, time_to_passing_patch_ns: Option<i64>, evaluation_input: EvaluationInputIdentity, measurement: TaskMeasurement, content_sha256: string }
TaskAggregate { task_id: string, parent_pass_count: i64, candidate_pass_count: i64, parent_repair_loop_count: i64, candidate_repair_loop_count: i64, paired_pass_count: i64, parent_paired_median_time_ns: Option<i64>, candidate_paired_median_time_ns: Option<i64>, time_improvement_ppm: Option<i64>, time_regression_ppm: Option<i64> }
CorpusAggregate { task_count: i64, sample_count: i64, parent_pass_count: i64, candidate_pass_count: i64, parent_repair_loop_count: i64, candidate_repair_loop_count: i64, paired_pass_count: i64, parent_paired_median_time_ns: Option<i64>, candidate_paired_median_time_ns: Option<i64>, completion_gain_count: i64, time_improvement_ppm: Option<i64>, time_regression_ppm: Option<i64>, repair_loop_regression_count: i64 }
RegressionReason { task_id: string, sample_index: i64, code: string, parent_value: string, candidate_value: string, limit: string }
RunSnapshotAttestation { schema_version: i64, artifact_kind: string, task_id: string, sample_index: i64, variant: string, status: string, error_code: string, error: string, snapshot_request_sha256: string, before_snapshot_result_sha256: string, after_snapshot_result_sha256: Option<string>, before_input_snapshot_sha256: Option<string>, after_input_snapshot_sha256: Option<string>, content_sha256: string }
SnapshotResult { schema_version: i64, artifact_kind: string, task_id: string, status: string, error_code: string, error: string, environment_probe: Option<EnvironmentProbe>, artifact_digests: array<ArtifactDigest>, content_sha256: string }
TaskInputSnapshot { schema_version: i64, artifact_kind: string, task_id: string, task_manifest_sha256: string, artifact_digests: array<ArtifactDigest>, environment_sha256: string, content_sha256: string }
Request10ConsumerRoot { tasks: array<PromptEvaluationTask>, snapshot_requests: array<SnapshotRequest>, snapshot_results: array<SnapshotResult>, input_snapshots: array<TaskInputSnapshot>, snapshot_attestations: array<RunSnapshotAttestation>, rows: array<PromptTaskRow>, task_aggregates: array<TaskAggregate>, corpus_aggregate: Option<CorpusAggregate>, serious_regression_reasons: array<RegressionReason> }

fn text(filled: bool) -> string {
  if filled { return "x".clone() }
  return "".clone()
}
fn strings(filled: bool) -> array<string> {
  mut b: array_builder<string> := array_builder()
  if filled { b.push(text(true)) }
  return b.build()
}
fn expectation(filled: bool) -> ArtifactExpectation = ArtifactExpectation{path: text(filled), kind: text(filled), expected_sha256: text(filled)}
fn expectations(include: bool, filled: bool) -> array<ArtifactExpectation> {
  mut b: array_builder<ArtifactExpectation> := array_builder()
  if include { b.push(expectation(filled)) }
  return b.build()
}
fn digest(filled: bool) -> ArtifactDigest = ArtifactDigest{path: text(filled), mode: text(filled), byte_count: 1, sha256: text(filled)}
fn digests(include: bool, filled: bool) -> array<ArtifactDigest> {
  mut b: array_builder<ArtifactDigest> := array_builder()
  if include { b.push(digest(filled)) }
  return b.build()
}
fn limits(filled: bool) -> RegressionLimits = RegressionLimits{maximum_unrelated_diff_count: 1, maximum_patch_size_bytes: 1, maximum_public_api_change_count: 1, maximum_repair_loops: 1, maximum_benchmark_regression_ppm: if filled { Some(1) } else { None }}
fn environment(filled: bool) -> EnvironmentProbe = EnvironmentProbe{schema_version: 1, artifact_kind: text(filled), producer: text(filled), os: text(filled), os_release: text(filled), architecture: text(filled), cpu: text(filled), logical_cpu_count: if filled { Some(1) } else { None }, gpu: text(filled), runtime_identity: text(filled), content_sha256: text(filled)}
fn evaluation_input(filled: bool) -> EvaluationInputIdentity = EvaluationInputIdentity{schema_version: 1, artifact_kind: text(filled), task_id: text(filled), task_input_snapshot_sha256: text(filled), parent_variant_sha256: text(filled), candidate_variant_sha256: text(filled), task_prompt_sha256: text(filled), context_sources_sha256: text(filled), generation_policy_sha256: text(filled), generation_request_sha256: text(filled), adapter_request_sha256: text(filled), environment_policy_sha256: text(filled), environment_sha256: text(filled), sample_index: 1, paired_seed: 1, content_sha256: text(filled)}
fn generation_request(filled: bool) -> GenerationRequestIdentity = GenerationRequestIdentity{schema_version: 1, artifact_kind: text(filled), rendered_prompt_sha256: text(filled), system_text_sha256: text(filled), user_text_sha256: text(filled), generation_policy_sha256: text(filled), provider_control_sha256: text(filled), environment_policy_sha256: text(filled), max_tokens: 1, temperature_micros: 1, paired_seed: 1, provider_request_sha256: text(filled), seed_attestation_sha256: text(filled), content_sha256: text(filled)}
fn seed(filled: bool) -> SeedCapabilityAttestation = SeedCapabilityAttestation{schema_version: 1, artifact_kind: text(filled), provider_kind: text(filled), provider_model: text(filled), requested_seed: 1, result: text(filled), applied_seed: if filled { Some(1) } else { None }, provider_request_sha256: text(filled), content_sha256: text(filled)}
fn measurement(filled: bool) -> TaskMeasurement = TaskMeasurement{schema_version: 1, artifact_kind: text(filled), status: text(filled), failure_kind: text(filled), build_status: text(filled), test_status: text(filled), repair_loop_count: 1, unrelated_diff_count: 1, patch_size_bytes: 1, public_api_change_count: 1, policy_violation_count: 1, cleanup_passed: true, containment_passed: true, benchmark_regression_ppm: if filled { Some(1) } else { None }, generation_to_passing_patch_ns: if filled { Some(1) } else { None }, rendered_prompt_sha256: text(filled), generation_request: generation_request(filled), environment_probe: environment(filled), seed_attestation: seed(filled), diagnostic_summary: text(filled), diagnostic_stdout: text(filled), diagnostic_stderr: text(filled), content_sha256: text(filled)}
fn snapshot_request(filled: bool) -> SnapshotRequest = SnapshotRequest{schema_version: 1, artifact_kind: text(filled), task_id: text(filled), project_root: text(filled), repo_path: text(filled), repo_revision: text(filled), require_clean_repo: true, static_expectations: expectations(filled, filled), additional_files: strings(filled), workspace_path: text(filled), allowed_workspace_entries: strings(filled), content_sha256: text(filled)}
fn prompt_task(filled: bool) -> PromptEvaluationTask = PromptEvaluationTask{schema_version: 1, artifact_kind: text(filled), task_id: text(filled), repo_id: text(filled), repo_revision: text(filled), repo_path: text(filled), require_clean_repo: true, cmd: text(filled), argv: strings(filled), snapshot_cmd: text(filled), snapshot_argv: strings(filled), measurement_adapter_runtime: text(filled), snapshot_helper_runtime: text(filled), cwd: text(filled), timeout_ns: 1, task_prompt_path: text(filled), context_sources_path: text(filled), generation_policy_path: text(filled), provider_control_path: text(filled), environment_policy_path: text(filled), artifacts: expectations(filled, filled), regression_limits: limits(filled), content_sha256: text(filled)}
fn row(filled: bool) -> PromptTaskRow = PromptTaskRow{schema_version: 1, artifact_kind: text(filled), evaluation_id: text(filled), task_id: text(filled), sample_index: 1, variant: text(filled), variant_id: text(filled), variant_sha256: text(filled), prompt_preparation_ns: 1, time_to_passing_patch_ns: if filled { Some(1) } else { None }, evaluation_input: evaluation_input(filled), measurement: measurement(filled), content_sha256: text(filled)}
fn task_aggregate(filled: bool) -> TaskAggregate = TaskAggregate{task_id: text(filled), parent_pass_count: 1, candidate_pass_count: 1, parent_repair_loop_count: 1, candidate_repair_loop_count: 1, paired_pass_count: 1, parent_paired_median_time_ns: if filled { Some(1) } else { None }, candidate_paired_median_time_ns: if filled { Some(1) } else { None }, time_improvement_ppm: if filled { Some(1) } else { None }, time_regression_ppm: if filled { Some(1) } else { None }}
fn corpus(filled: bool) -> CorpusAggregate = CorpusAggregate{task_count: 1, sample_count: 1, parent_pass_count: 1, candidate_pass_count: 1, parent_repair_loop_count: 1, candidate_repair_loop_count: 1, paired_pass_count: 1, parent_paired_median_time_ns: if filled { Some(1) } else { None }, candidate_paired_median_time_ns: if filled { Some(1) } else { None }, completion_gain_count: 1, time_improvement_ppm: if filled { Some(1) } else { None }, time_regression_ppm: if filled { Some(1) } else { None }, repair_loop_regression_count: 1}
fn reason(filled: bool) -> RegressionReason = RegressionReason{task_id: text(filled), sample_index: 1, code: text(filled), parent_value: text(filled), candidate_value: text(filled), limit: text(filled)}
fn attestation(filled: bool) -> RunSnapshotAttestation = RunSnapshotAttestation{schema_version: 1, artifact_kind: text(filled), task_id: text(filled), sample_index: 1, variant: text(filled), status: text(filled), error_code: text(filled), error: text(filled), snapshot_request_sha256: text(filled), before_snapshot_result_sha256: text(filled), after_snapshot_result_sha256: if filled { Some(text(true)) } else { None }, before_input_snapshot_sha256: if filled { Some(text(true)) } else { None }, after_input_snapshot_sha256: if filled { Some(text(true)) } else { None }, content_sha256: text(filled)}
fn snapshot_result(filled: bool) -> SnapshotResult = SnapshotResult{schema_version: 1, artifact_kind: text(filled), task_id: text(filled), status: text(filled), error_code: text(filled), error: text(filled), environment_probe: if filled { Some(environment(true)) } else { None }, artifact_digests: digests(filled, filled), content_sha256: text(filled)}
fn input_snapshot(filled: bool) -> TaskInputSnapshot = TaskInputSnapshot{schema_version: 1, artifact_kind: text(filled), task_id: text(filled), task_manifest_sha256: text(filled), artifact_digests: digests(filled, filled), environment_sha256: text(filled), content_sha256: text(filled)}

fn tasks(include: bool, filled: bool) -> array<PromptEvaluationTask> { mut b: array_builder<PromptEvaluationTask> := array_builder()
  if include { b.push(prompt_task(filled)) }
  return b.build() }
fn snapshot_requests(include: bool, filled: bool) -> array<SnapshotRequest> { mut b: array_builder<SnapshotRequest> := array_builder()
  if include { b.push(snapshot_request(filled)) }
  return b.build() }
fn snapshot_results(include: bool, filled: bool) -> array<SnapshotResult> { mut b: array_builder<SnapshotResult> := array_builder()
  if include { b.push(snapshot_result(filled)) }
  return b.build() }
fn input_snapshots(include: bool, filled: bool) -> array<TaskInputSnapshot> { mut b: array_builder<TaskInputSnapshot> := array_builder()
  if include { b.push(input_snapshot(filled)) }
  return b.build() }
fn attestations(include: bool, filled: bool) -> array<RunSnapshotAttestation> { mut b: array_builder<RunSnapshotAttestation> := array_builder()
  if include { b.push(attestation(filled)) }
  return b.build() }
fn rows(include: bool, filled: bool) -> array<PromptTaskRow> { mut b: array_builder<PromptTaskRow> := array_builder()
  if include { b.push(row(filled)) }
  return b.build() }
fn task_aggregates(include: bool, filled: bool) -> array<TaskAggregate> { mut b: array_builder<TaskAggregate> := array_builder()
  if include { b.push(task_aggregate(filled)) }
  return b.build() }
fn reasons(include: bool, filled: bool) -> array<RegressionReason> { mut b: array_builder<RegressionReason> := array_builder()
  if include { b.push(reason(filled)) }
  return b.build() }
fn root(include: bool, filled: bool) -> Request10ConsumerRoot = Request10ConsumerRoot{tasks: tasks(include, filled), snapshot_requests: snapshot_requests(include, filled), snapshot_results: snapshot_results(include, filled), input_snapshots: input_snapshots(include, filled), snapshot_attestations: attestations(include, filled), rows: rows(include, filled), task_aggregates: task_aggregates(include, filled), corpus_aggregate: if include { Some(corpus(filled)) } else { None }, serious_regression_reasons: reasons(include, filled)}
fn main() -> i32 {
  mut b: array_builder<Request10ConsumerRoot> := array_builder()
  b.push(root(true, true))
  b.push(root(true, false))
  b.push(root(false, false))
  values := b.build()
  if values.len() == 3 { return 0 }
  return 1
}
"#;

#[test]
fn request10_exact_c6_consumer_graph() {
    let expected: &[(&str, &[(&str, &str)])] = &[
        ("ArtifactExpectation", &[("path", "string"), ("kind", "string"), ("expected_sha256", "string")]),
        ("ArtifactDigest", &[("path", "string"), ("mode", "string"), ("byte_count", "i64"), ("sha256", "string")]),
        ("RegressionLimits", &[("maximum_unrelated_diff_count", "i64"), ("maximum_patch_size_bytes", "i64"), ("maximum_public_api_change_count", "i64"), ("maximum_repair_loops", "i64"), ("maximum_benchmark_regression_ppm", "Option<i64>")]),
        ("EnvironmentProbe", &[("schema_version", "i64"), ("artifact_kind", "string"), ("producer", "string"), ("os", "string"), ("os_release", "string"), ("architecture", "string"), ("cpu", "string"), ("logical_cpu_count", "Option<i64>"), ("gpu", "string"), ("runtime_identity", "string"), ("content_sha256", "string")]),
        ("EvaluationInputIdentity", &[("schema_version", "i64"), ("artifact_kind", "string"), ("task_id", "string"), ("task_input_snapshot_sha256", "string"), ("parent_variant_sha256", "string"), ("candidate_variant_sha256", "string"), ("task_prompt_sha256", "string"), ("context_sources_sha256", "string"), ("generation_policy_sha256", "string"), ("generation_request_sha256", "string"), ("adapter_request_sha256", "string"), ("environment_policy_sha256", "string"), ("environment_sha256", "string"), ("sample_index", "i64"), ("paired_seed", "i64"), ("content_sha256", "string")]),
        ("GenerationRequestIdentity", &[("schema_version", "i64"), ("artifact_kind", "string"), ("rendered_prompt_sha256", "string"), ("system_text_sha256", "string"), ("user_text_sha256", "string"), ("generation_policy_sha256", "string"), ("provider_control_sha256", "string"), ("environment_policy_sha256", "string"), ("max_tokens", "i64"), ("temperature_micros", "i64"), ("paired_seed", "i64"), ("provider_request_sha256", "string"), ("seed_attestation_sha256", "string"), ("content_sha256", "string")]),
        ("SeedCapabilityAttestation", &[("schema_version", "i64"), ("artifact_kind", "string"), ("provider_kind", "string"), ("provider_model", "string"), ("requested_seed", "i64"), ("result", "string"), ("applied_seed", "Option<i64>"), ("provider_request_sha256", "string"), ("content_sha256", "string")]),
        ("TaskMeasurement", &[("schema_version", "i64"), ("artifact_kind", "string"), ("status", "string"), ("failure_kind", "string"), ("build_status", "string"), ("test_status", "string"), ("repair_loop_count", "i64"), ("unrelated_diff_count", "i64"), ("patch_size_bytes", "i64"), ("public_api_change_count", "i64"), ("policy_violation_count", "i64"), ("cleanup_passed", "bool"), ("containment_passed", "bool"), ("benchmark_regression_ppm", "Option<i64>"), ("generation_to_passing_patch_ns", "Option<i64>"), ("rendered_prompt_sha256", "string"), ("generation_request", "GenerationRequestIdentity"), ("environment_probe", "EnvironmentProbe"), ("seed_attestation", "SeedCapabilityAttestation"), ("diagnostic_summary", "string"), ("diagnostic_stdout", "string"), ("diagnostic_stderr", "string"), ("content_sha256", "string")]),
        ("SnapshotRequest", &[("schema_version", "i64"), ("artifact_kind", "string"), ("task_id", "string"), ("project_root", "string"), ("repo_path", "string"), ("repo_revision", "string"), ("require_clean_repo", "bool"), ("static_expectations", "array<ArtifactExpectation>"), ("additional_files", "array<string>"), ("workspace_path", "string"), ("allowed_workspace_entries", "array<string>"), ("content_sha256", "string")]),
        ("PromptEvaluationTask", &[("schema_version", "i64"), ("artifact_kind", "string"), ("task_id", "string"), ("repo_id", "string"), ("repo_revision", "string"), ("repo_path", "string"), ("require_clean_repo", "bool"), ("cmd", "string"), ("argv", "array<string>"), ("snapshot_cmd", "string"), ("snapshot_argv", "array<string>"), ("measurement_adapter_runtime", "string"), ("snapshot_helper_runtime", "string"), ("cwd", "string"), ("timeout_ns", "i64"), ("task_prompt_path", "string"), ("context_sources_path", "string"), ("generation_policy_path", "string"), ("provider_control_path", "string"), ("environment_policy_path", "string"), ("artifacts", "array<ArtifactExpectation>"), ("regression_limits", "RegressionLimits"), ("content_sha256", "string")]),
        ("PromptTaskRow", &[("schema_version", "i64"), ("artifact_kind", "string"), ("evaluation_id", "string"), ("task_id", "string"), ("sample_index", "i64"), ("variant", "string"), ("variant_id", "string"), ("variant_sha256", "string"), ("prompt_preparation_ns", "i64"), ("time_to_passing_patch_ns", "Option<i64>"), ("evaluation_input", "EvaluationInputIdentity"), ("measurement", "TaskMeasurement"), ("content_sha256", "string")]),
        ("TaskAggregate", &[("task_id", "string"), ("parent_pass_count", "i64"), ("candidate_pass_count", "i64"), ("parent_repair_loop_count", "i64"), ("candidate_repair_loop_count", "i64"), ("paired_pass_count", "i64"), ("parent_paired_median_time_ns", "Option<i64>"), ("candidate_paired_median_time_ns", "Option<i64>"), ("time_improvement_ppm", "Option<i64>"), ("time_regression_ppm", "Option<i64>")]),
        ("CorpusAggregate", &[("task_count", "i64"), ("sample_count", "i64"), ("parent_pass_count", "i64"), ("candidate_pass_count", "i64"), ("parent_repair_loop_count", "i64"), ("candidate_repair_loop_count", "i64"), ("paired_pass_count", "i64"), ("parent_paired_median_time_ns", "Option<i64>"), ("candidate_paired_median_time_ns", "Option<i64>"), ("completion_gain_count", "i64"), ("time_improvement_ppm", "Option<i64>"), ("time_regression_ppm", "Option<i64>"), ("repair_loop_regression_count", "i64")]),
        ("RegressionReason", &[("task_id", "string"), ("sample_index", "i64"), ("code", "string"), ("parent_value", "string"), ("candidate_value", "string"), ("limit", "string")]),
        ("RunSnapshotAttestation", &[("schema_version", "i64"), ("artifact_kind", "string"), ("task_id", "string"), ("sample_index", "i64"), ("variant", "string"), ("status", "string"), ("error_code", "string"), ("error", "string"), ("snapshot_request_sha256", "string"), ("before_snapshot_result_sha256", "string"), ("after_snapshot_result_sha256", "Option<string>"), ("before_input_snapshot_sha256", "Option<string>"), ("after_input_snapshot_sha256", "Option<string>"), ("content_sha256", "string")]),
        ("SnapshotResult", &[("schema_version", "i64"), ("artifact_kind", "string"), ("task_id", "string"), ("status", "string"), ("error_code", "string"), ("error", "string"), ("environment_probe", "Option<EnvironmentProbe>"), ("artifact_digests", "array<ArtifactDigest>"), ("content_sha256", "string")]),
        ("TaskInputSnapshot", &[("schema_version", "i64"), ("artifact_kind", "string"), ("task_id", "string"), ("task_manifest_sha256", "string"), ("artifact_digests", "array<ArtifactDigest>"), ("environment_sha256", "string"), ("content_sha256", "string")]),
        ("Request10ConsumerRoot", &[("tasks", "array<PromptEvaluationTask>"), ("snapshot_requests", "array<SnapshotRequest>"), ("snapshot_results", "array<SnapshotResult>"), ("input_snapshots", "array<TaskInputSnapshot>"), ("snapshot_attestations", "array<RunSnapshotAttestation>"), ("rows", "array<PromptTaskRow>"), ("task_aggregates", "array<TaskAggregate>"), ("corpus_aggregate", "Option<CorpusAggregate>"), ("serious_regression_reasons", "array<RegressionReason>")]),
    ];

    fn scalar_name(program: &align_sema::Program, scalar: align_sema::Scalar) -> String {
        use align_sema::Scalar;
        match scalar {
            Scalar::Int(ty) if ty.bits == 64 && ty.signed => "i64".to_string(),
            Scalar::Bool => "bool".to_string(),
            Scalar::String => "string".to_string(),
            Scalar::Struct(id) => program.structs[id as usize].source_name.clone(),
            other => panic!("unexpected Request 10 scalar {other:?}"),
        }
    }
    fn type_name(program: &align_sema::Program, ty: align_sema::Ty) -> String {
        use align_sema::{Layout, Ty};
        match ty {
            Ty::Int(ty) if ty.bits == 64 && ty.signed => "i64".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::String => "string".to_string(),
            Ty::Struct(id) => program.structs[id as usize].source_name.clone(),
            Ty::Option(payload) => format!("Option<{}>", scalar_name(program, payload)),
            Ty::DynArray(payload) => format!("array<{}>", scalar_name(program, payload)),
            Ty::DynStructArray(id, Layout::Aos) => {
                format!("array<{}>", program.structs[id as usize].source_name)
            }
            other => panic!("unexpected Request 10 field type {other:?}"),
        }
    }

    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "request10-c6-fields", REQUEST10_C6_SOURCE);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    for (record_name, expected_fields) in expected {
        let record = checked
            .hir
            .structs
            .iter()
            .find(|record| record.source_name == *record_name)
            .unwrap_or_else(|| panic!("missing exact C6 record {record_name}"));
        let actual = record
            .fields
            .iter()
            .map(|field| (field.name.as_str(), type_name(&checked.hir, field.ty)))
            .collect::<Vec<_>>();
        let expected = expected_fields
            .iter()
            .map(|(name, ty)| (*name, (*ty).to_string()))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "field vector drifted for {record_name}");
    }
    if backend_available() {
        let out = build_and_run("request10-c6-runtime", REQUEST10_C6_SOURCE);
        assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
}

#[test]
fn recursive_record_tagged_wrapper_transfer_and_drop_matrix() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "Payload { names: array<string>, label: string }\n",
        "Envelope { payload: Payload, tail: string }\n",
        "Choice { Empty, Data(Payload), Alternate(Payload) }\n",
        "fn names(value: str) -> array<string> { mut b: array_builder<string> := array_builder()\n",
        "  b.push(value.clone())\n",
        "  b.push(\"tail\".clone())\n",
        "  return b.build() }\n",
        "fn payload(value: str) -> Payload = Payload{names: names(value), label: value.clone()}\n",
        "fn maybe(present: bool) -> Option<Payload> { if present { return Some(payload(\"some\")) }\n",
        "  return None }\n",
        "fn result(ok: bool) -> Result<Payload, Payload> { if ok { return Ok(payload(\"ok\")) }\n",
        "  return Err(payload(\"err\")) }\n",
        "fn relay() -> Result<Payload, Payload> { value := result(true)?\n",
        "  return Ok(value) }\n",
        "fn map_payload(value: Payload) -> Payload = value\n",
        "fn mapped() -> Result<Payload, Payload> { value := result(true).map_err(map_payload)?\n",
        "  return Ok(value) }\n",
        "fn fail_string() -> Result<string, Payload> = Err(payload(\"partial-error\"))\n",
        "fn partial() -> Result<Envelope, Payload> = Ok(Envelope{payload: payload(\"partial-owner\"), tail: fail_string()?})\n",
        "fn exercise() -> i32 {\n",
        "  mut selected: Option<Payload> := Some(payload(\"old\"))\n",
        "  selected = None\n",
        "  selected = Some(payload(\"new\"))\n",
        "  value := selected else { return 1 }\n",
        "  none_score := match maybe(false) { Some(_) => 10, None => 0 }\n",
        "  some_score := match maybe(true) { Some(_) => 0, None => 20 }\n",
        "  ok_score := match relay() { Ok(_) => 0, Err(_) => 30 }\n",
        "  err_score := match result(false) { Ok(_) => 40, Err(_) => 0 }\n",
        "  mapped_score := match mapped() { Ok(_) => 0, Err(_) => 50 }\n",
        "  data_score := match Choice.Data(value) { Empty => 60, Data(_) => 0, Alternate(_) => 70 }\n",
        "  alternate_score := match Choice.Alternate(payload(\"alternate\")) { Empty => 80, Data(_) => 90, Alternate(_) => 0 }\n",
        "  partial_score := match partial() { Ok(_) => 100, Err(_) => 0 }\n",
        "  return none_score + some_score + ok_score + err_score + mapped_score + data_score + alternate_score + partial_score\n",
        "}\n",
        "fn main() -> i32 { mut i := 0\n",
        "  loop { if exercise() != 0 { return 1 }\n",
        "    i = i + 1\n",
        "    if i >= 1000 { break } }\n",
        "  return 0 }\n",
    );
    let out = build_and_run("ab-recursive-tagged", src);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn recursive_record_generic_substitution_is_concrete_before_admission() {
    if !backend_available() {
        return;
    }
    let positive = concat!(
        "Leaf { value: string }\n",
        "Tree<T> { maybe: Option<T>, leaves: array<T> }\n",
        "fn leaves() -> array<Leaf> { mut b: array_builder<Leaf> := array_builder()\n",
        "  b.push(Leaf{value: \"leaf\".clone()})\n",
        "  return b.build() }\n",
        "fn main() -> i32 { mut b: array_builder<Tree<Leaf>> := array_builder()\n",
        "  b.push(Tree{maybe: Some(Leaf{value: \"optional\".clone()}), leaves: leaves()})\n",
        "  values := b.build()\n",
        "  if values.len() == 1 { return 0 }\n",
        "  return 1 }\n",
    );
    let out = build_and_run("ab-recursive-generic", positive);
    assert_eq!(code(&out), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let excluded = concat!(
        "Bad<T> { values: array<Option<T>> }\n",
        "fn main() -> i32 { mut b: array_builder<Bad<i64>> := array_builder()\n",
        "  return 0 }\n",
    );
    assert!(check_errs("ab-recursive-generic-composite-array", excluded));
}

#[test]
fn record_builder_closed_shape_and_append_rejections() {
    let cases = [
        ("view", "Bad { value: str }"),
        ("slice", "Bad { value: slice<i64> }"),
        ("result", "Bad { value: Result<i64, i64> }"),
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
        "Item { names: array<string>, maybe: Option<string> }\n",
        "Holder { items: array_builder<Item> }\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(check_errs("ab-record-aggregate-storage", aggregate));

    let borrowed_build = concat!(
        "Item { names: array<string>, maybe: Option<string> }\n",
        "fn consume(borrow mut items: array_builder<Item>) { values := items.build() }\n",
        "fn main() -> i32 = 0\n",
    );
    assert!(check_errs("ab-record-borrowed-build", borrowed_build));

    let capture = concat!(
        "Item { names: array<string>, maybe: Option<string> }\n",
        "fn names() -> array<string> { mut b: array_builder<string> := array_builder()\n",
        "  b.push(\"name\".clone())\n",
        "  return b.build() }\n",
        "fn main() -> Result<(), Error> {\n",
        "  mut items: array_builder<Item> := array_builder()\n",
        "  task_group { task := spawn(fn { items.push(Item{names: names(), maybe: Some(\"x\".clone())}); 1 })\n",
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
