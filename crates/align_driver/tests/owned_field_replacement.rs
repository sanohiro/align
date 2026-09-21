//! Tests for owned field replacement on records (Plan 65 Row O / Issue #1048 Gap B).
//!
//! Verifies `owner.field = value` where `field` is an owned Move type with a Drop plan
//! (e.g. `array<T>`, `buffer`, `string`, nested records, resources).
//! Old values must be dropped at the assignment point, moved sources nulled, and memory reclaimed.

mod common;
use common::*;

#[test]
fn owned_field_replacement_array_primitive() {
    if !backend_available() {
        return;
    }
    let src = "\
extern \"C\" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
}

Session {
  id: i64,
  prefix_ids: array<i64>,
}

fn exercise() -> i32 {
  mut s := Session {
    id: 42,
    prefix_ids: [10, 20, 30].to_array(),
  }
  if s.prefix_ids.len() != 3 { return 1 }
  if s.prefix_ids[0] != 10 || s.prefix_ids[2] != 30 { return 2 }

  base_live := unsafe { align_rt_requested_live_bytes() }
  if base_live <= 0 { return 3 }

  // Overwrite prefix_ids repeatedly in a loop.
  // If the old array is properly freed on each assignment, live bytes must not grow.
  mut i := 0
  loop {
    if i >= 50 { break }
    s.prefix_ids = [100 + i, 200 + i].to_array()
    i = i + 1
  }

  if s.prefix_ids.len() != 2 { return 4 }
  if s.prefix_ids[0] != 149 || s.prefix_ids[1] != 249 { return 5 }

  // Live bytes after 50 replacements should equal live bytes for a 2-element array
  current_live := unsafe { align_rt_requested_live_bytes() }
  if current_live <= 0 { return 6 }
  if current_live >= base_live * 5 { return 7 } // definitely no unbounded leak

  return 0
}

fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  res := exercise()
  if res != 0 { return res }
  // After session goes out of scope, all memory must be freed
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 8 }
  return 0
}
";
    assert_eq!(build_and_run("owned-field-repl-arr-i64", src).status.code(), Some(0));
}

#[test]
fn owned_field_replacement_string() {
    if !backend_available() {
        return;
    }
    let src = "\
extern \"C\" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
}

Doc {
  title: string,
  body: string,
}

fn exercise() -> i32 {
  mut doc := Doc {
    title: \"align\".clone(),
    body: \"aot language\".clone(),
  }
  if doc.title.len() != 5 || doc.body.len() != 12 { return 1 }

  base_live := unsafe { align_rt_requested_live_bytes() }
  if base_live <= 0 { return 2 }

  // Replace title repeatedly in a loop
  mut i := 0
  loop {
    if i >= 40 { break }
    doc.title = \"align v2\".clone()
    i = i + 1
  }

  if doc.title.len() != 8 { return 3 }
  // Live bytes should be bounded and not leak 40 strings
  if unsafe { align_rt_requested_live_bytes() } > base_live * 2 { return 4 }

  return 0
}

fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  res := exercise()
  if res != 0 { return res }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 5 }
  return 0
}
";
    assert_eq!(build_and_run("owned-field-repl-str", src).status.code(), Some(0));
}

#[test]
fn owned_field_replacement_buffer() {
    if !backend_available() {
        return;
    }
    let src = "\
extern \"C\" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
}

State {
  version: i32,
  buf: buffer,
}

fn exercise() -> i32 {
  mut b0 := buffer(256)
  b0.put_u8(42)
  mut st := State {
    version: 1,
    buf: b0,
  }

  live_single := unsafe { align_rt_requested_live_bytes() }
  if live_single <= 0 { return 1 }

  // Replace buffer repeatedly
  mut i := 0
  loop {
    if i >= 30 { break }
    mut next_buf := buffer(256)
    next_buf.put_u8(7)
    st.buf = next_buf
    if unsafe { align_rt_requested_live_bytes() } != live_single { return 2 }
    i = i + 1
  }

  if st.buf.bytes().u8(0) != 7 { return 3 }
  return 0
}

fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  res := exercise()
  if res != 0 { return res }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 4 }
  return 0
}
";
    assert_eq!(build_and_run("owned-field-repl-buf", src).status.code(), Some(0));
}

#[test]
fn owned_field_replacement_nested_record() {
    if !backend_available() {
        return;
    }
    let src = "\
Inner {
  items: array<i64>,
}

Outer {
  inner: Inner,
  name: str,
}

fn main() -> i32 {
  mut outer := Outer {
    inner: Inner { items: [1, 2, 3].to_array() },
    name: \"test\",
  }
  if outer.inner.items.len() != 3 { return 1 }

  outer.inner.items = [10, 20].to_array()
  if outer.inner.items.len() != 2 { return 2 }
  if outer.inner.items[0] != 10 || outer.inner.items[1] != 20 { return 3 }

  return 0
}
";
    assert_eq!(build_and_run("owned-field-repl-nested", src).status.code(), Some(0));
}

#[test]
fn owned_field_replacement_arena_uniform() {
    if !backend_available() {
        return;
    }
    let src = "\
Pair {
  left: array<i64>,
  right: array<i64>,
}

fn main() -> i32 {
  arena {
    mut p := Pair {
      left: [1, 2].to_array(),
      right: [3, 4].to_array(),
    }
    p.right = [5, 6, 7].to_array()
    if p.left.len() != 2 { return 1 }
    if p.right.len() != 3 { return 2 }
    if p.right[0] != 5 || p.right[2] != 7 { return 3 }
  }
  return 0
}
";
    assert_eq!(build_and_run("owned-field-repl-arena", src).status.code(), Some(0));
}

#[test]
fn owned_field_replacement_option_string() {
    if !backend_available() {
        return;
    }
    let src = "\
extern \"C\" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
}

Holder {
  val: Option<string>,
}

fn exercise() -> i32 {
  mut h := Holder {
    val: Some(\"hello\".clone()),
  }
  base_live := unsafe { align_rt_requested_live_bytes() }
  if base_live <= 0 { return 1 }

  // Replace with another Some
  h.val = Some(\"world\".clone())
  if unsafe { align_rt_requested_live_bytes() } != base_live { return 2 }

  // Replace with None
  h.val = None
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 3 }

  return 0
}

fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  res := exercise()
  if res != 0 { return res }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 4 }
  return 0
}
";
    assert_eq!(build_and_run("owned-field-repl-opt-str", src).status.code(), Some(0));
}

#[test]
fn owned_field_replacement_after_fallible_control_flow() {
    if !backend_available() {
        return;
    }
    let src = r#"
extern "C" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_bytes() -> i64
}

Fault { code: string, detail: string }
Session { decode_key: string, count: i64 }

fn act(kind: i64) -> Result<(), Fault> {
  if kind < 0 {
    return Err(Fault { code: "CONFIG".clone(), detail: "kind".clone() })
  }
  return Ok(())
}

fn prepare(kind: i64) -> Result<(), Fault> {
  act(kind) else {
    return Err(Fault { code: "PREPARE".clone(), detail: "act".clone() })
  }
  return Ok(())
}

fn drive_borrowed(borrow mut session: Session, n: i64) -> Result<i64, Fault> {
  key := "graph-key".clone()
  prepare(n)?
  session.decode_key = key.clone()
  return Ok(session.decode_key.len())
}

fn drive_local(n: i64) -> Result<i64, Fault> {
  mut session := Session { decode_key: "old".clone(), count: 0 }
  prepare(n)?
  session.decode_key = "local-key".clone()
  return Ok(session.decode_key.len())
}

fn drive_match(borrow mut session: Session, n: i64) -> Result<i64, Fault> {
  match prepare(n) {
    Ok(unit) => {},
    Err(error) => { return Err(error) },
  }
  session.decode_key = "match-key".clone()
  return Ok(session.decode_key.len())
}

fn exercise() -> i32 {
  mut session := Session { decode_key: "".clone(), count: 0 }
  borrowed := drive_borrowed(session, 3) else { return 1 }
  if borrowed != 9 || session.decode_key.len() != 9 { return 2 }
  local := drive_local(3) else { return 3 }
  if local != 9 { return 4 }
  matched := drive_match(session, 3) else { return 5 }
  if matched != 9 || session.decode_key.len() != 9 { return 6 }
  match drive_borrowed(session, -1) {
    Ok(value) => { return 7 },
    Err(error) => {
      if error.code.len() != 7 || error.detail.len() != 3 { return 8 }
    },
  }
  return 0
}

fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  status := exercise()
  if status != 0 { return status }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 9 }
  return 0
}
"#;

    let output = build_and_run("owned-field-repl-fallible", src);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
