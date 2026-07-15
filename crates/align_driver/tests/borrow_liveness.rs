//! Intra-frame borrow-liveness regression coverage. A view may stay live only while the owned
//! source generation it borrows is live; moving or replacing that source invalidates existing
//! views, while uses before replacement and re-borrows from the replacement remain legal.

mod common;
use common::*;

#[test]
fn buffer_view_used_after_source_reassign_is_rejected() {
    let src = "\
fn main() -> i32 {
  mut b := buffer(8)
  bytes := b.bytes()
  b = buffer(16)
  return bytes.len() as i32
}
";
    assert!(check_errs("borrow-buffer-reassign", src));
}

#[test]
fn string_view_used_after_source_move_is_rejected() {
    let src = "\
fn consume(s: string) -> i64 = s.len()
fn main() -> i32 {
  owned := \"hello\".clone()
  view: str := owned
  consume(owned)
  return view.len() as i32
}
";
    assert!(check_errs("borrow-string-move", src));
}

#[test]
fn response_body_used_after_response_reassign_is_rejected() {
    let src = "\
import std.http
fn bad(data: str) -> Result<i64, Error> {
  mut resp := http.parse(data)?
  body := resp.body()
  resp = http.parse(data)?
  return Ok(body.len())
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-response-reassign", src));
}

#[test]
fn conditional_source_reassign_invalidates_the_joined_view() {
    let src = "\
fn bad(flag: bool) -> i32 {
  mut b := buffer(8)
  bytes := b.bytes()
  if flag { b = buffer(16) }
  return bytes.len() as i32
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-conditional-reassign", src));
}

#[test]
fn loop_back_edge_sees_a_view_invalidated_by_the_previous_iteration() {
    let src = "\
fn main() -> i32 {
  mut b := buffer(8)
  bytes := b.bytes()
  mut first := true
  loop {
    print(bytes.len())
    if first {
      b = buffer(16)
      first = false
    } else {
      break
    }
  }
  return 0
}
";
    assert!(check_errs("borrow-loop-back-edge", src));
}

#[test]
fn view_use_before_source_reassign_is_allowed() {
    let src = "\
fn main() -> i32 {
  mut b := buffer(8)
  bytes := b.bytes()
  print(bytes.len())
  b = buffer(16)
  return b.len() as i32
}
";
    assert!(!check_errs("borrow-use-before-reassign", src));
}

#[test]
fn reborrow_after_source_reassign_is_allowed() {
    let src = "\
fn main() -> i32 {
  mut b := buffer(8)
  mut bytes := b.bytes()
  b = buffer(16)
  bytes = b.bytes()
  return bytes.len() as i32
}
";
    assert!(!check_errs("borrow-reborrow-after-reassign", src));
}

#[test]
fn move_on_a_diverging_branch_does_not_invalidate_fallthrough() {
    let src = "\
fn consume(s: string) -> i32 = s.len() as i32
fn ok(flag: bool) -> i32 {
  owned := \"hello\".clone()
  view: str := owned
  if flag { return consume(owned) }
  return view.len() as i32
}
fn main() -> i32 = 0
";
    assert!(!check_errs("borrow-diverging-move", src));
}

#[test]
fn tcp_reader_used_after_connection_reassign_is_rejected() {
    let src = "\
import std.net
import std.io
fn bad() -> Result<i64, Error> {
  mut conn := tcp.connect(\"127.0.0.1\", 80)?
  r := conn.reader()
  conn = tcp.connect(\"127.0.0.1\", 80)?
  b := buffer(8)
  return r.read(b)
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-tcp-reader-reassign", src));
}

#[test]
fn cli_string_used_after_parsed_reassign_is_rejected() {
    let src = "\
import std.cli
fn bad(args: array<str>) -> Result<i64, Error> {
  c := cli.command(\"demo\")
  c.flag_str(\"name\", \"world\")
  mut p := c.parse(args)?
  name := p.get_str(\"name\")
  p = c.parse(args)?
  return Ok(name.len())
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-cli-parsed-reassign", src));
}

#[test]
fn response_array_element_body_used_after_array_reassign_is_rejected() {
    let src = "\
import std.http
fn bad(urls: slice<str>) -> Result<i64, Error> {
  cl := http.client()
  mut rs := cl.get_many(urls, 2)?
  body := rs[0].body()
  rs = cl.get_many(urls, 2)?
  return Ok(body.len())
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-response-array-reassign", src));
}

#[test]
fn chained_string_view_used_after_buffer_reassign_is_rejected() {
    let src = "\
fn bad() -> Result<i64, Error> {
  mut b := buffer(8)
  text := b.bytes().as_str()?
  b = buffer(16)
  return Ok(text.len())
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-chained-buffer-view", src));
}

#[test]
fn view_returned_by_a_call_keeps_argument_provenance() {
    let src = "\
fn identity(s: str) -> str = s
fn consume(s: string) -> i64 = s.len()
fn main() -> i32 {
  owned := \"hello\".clone()
  view := identity(owned)
  consume(owned)
  return view.len() as i32
}
";
    assert!(check_errs("borrow-call-result", src));
}

#[test]
fn string_field_view_used_after_field_replacement_is_rejected() {
    let src = "\
TextBox { text: string }
fn main() -> i32 {
  mut b := TextBox { text: \"old\".clone() }
  view: str := b.text
  b.text = \"new\".clone()
  return view.len() as i32
}
";
    assert!(check_errs("borrow-string-field-reassign", src));
}

#[test]
fn branch_local_reborrow_clears_that_path_invalidation() {
    let src = "\
fn ok(flag: bool) -> i32 {
  mut b := buffer(8)
  mut bytes := b.bytes()
  if flag {
    b = buffer(16)
    bytes = b.bytes()
  }
  return bytes.len() as i32
}
fn main() -> i32 = 0
";
    assert!(!check_errs("borrow-branch-reborrow", src));
}

#[test]
fn reassign_on_a_diverging_branch_does_not_invalidate_fallthrough() {
    let src = "\
fn ok(flag: bool) -> i32 {
  mut b := buffer(8)
  bytes := b.bytes()
  if flag {
    b = buffer(16)
    return b.len() as i32
  }
  return bytes.len() as i32
}
fn main() -> i32 = 0
";
    assert!(!check_errs("borrow-diverging-reassign", src));
}

#[test]
fn invalidated_borrow_diagnostic_names_view_and_source() {
    let src = "\
fn main() -> i32 {
  mut storage := buffer(8)
  stale := storage.bytes()
  storage = buffer(16)
  return stale.len() as i32
}
";
    let diagnostics = check_diagnostics("borrow-diagnostic", src);
    assert!(
        diagnostics.contains("use of invalidated borrow 'stale'")
            && diagnostics.contains("source 'storage' was moved or reassigned"),
        "expected the diagnostic to identify both bindings:\n{diagnostics}"
    );
}

#[test]
fn buffer_growth_invalidates_an_existing_view() {
    let src = "\
fn main() -> i32 {
  mut b := buffer(1)
  stale := b.bytes()
  b.append(\"this forces growth\")
  return stale.len() as i32
}
";
    assert!(check_errs("borrow-buffer-growth", src));
}

#[test]
fn read_line_growth_invalidates_an_existing_buffer_view() {
    let src = "\
import std.io
fn bad() -> Result<i64, Error> {
  mut r := io.stdin.buffered()
  b := buffer(1)
  stale := b.bytes()
  r.read_line(b)?
  return Ok(stale.len())
}
fn main() -> i32 = 0
";
    assert!(check_errs("borrow-read-line-growth", src));
}

#[test]
fn fixed_array_owned_field_replacement_invalidates_its_view() {
    let src = "\
User { name: string }
fn main() -> i32 {
  mut users := [User { name: \"old\".clone() }]
  view: str := users[0].name
  users[0].name = \"new\".clone()
  return view.len() as i32
}
";
    assert!(check_errs("borrow-array-field-reassign", src));
}

#[test]
fn materialized_primitive_soa_survives_source_array_reassign() {
    let src = "\
import core.json
Point { x: i64 }
fn ok(data: str) -> Result<i64, Error> {
  arena {
    mut points: array<Point> := json.decode(data)?
    columns := points.to_soa()
    points = json.decode(data)?
    return Ok(columns.x[0])
  }
}
fn main() -> i32 = 0
";
    assert!(!check_errs("borrow-materialized-soa", src));
}

#[test]
fn primitive_soa_decode_survives_owned_input_move() {
    let src = "\
import core.json
Point { x: i64 }
fn consume(s: string) -> i64 = s.len()
fn main() -> Result<(), Error> {
  arena {
    input := \"[{\\\"x\\\":7}]\".clone()
    points: soa<Point> := json.decode(input)?
    consume(input)
    print(points.x[0])
  }
  return Ok(())
}
";
    assert!(!check_errs("borrow-primitive-soa-input", src));
}

#[test]
fn slice_of_borrowing_owned_array_tracks_array_and_element_roots() {
    let src = "\
fn identity(s: str) -> str = s
fn consume(xs: array<str>) -> i64 = xs.len()
fn main() -> i32 {
  arena {
    owned := \"hello\".clone()
    source: array<str> := [owned]
    values := source.map(identity).to_array()
    view := values[..]
    consume(values)
    return view.len() as i32
  }
}
";
    assert!(check_errs("borrow-owned-array-slice", src));
}

#[test]
fn pipeline_result_tracks_a_view_returned_from_a_capture() {
    let src = "\
fn consume(s: string) -> i64 = s.len()
fn main() -> i32 {
  arena {
    owned := \"hello\".clone()
    captured: str := owned
    values := [1, 2].map(fn n: i64 { captured }).to_array()
    consume(owned)
    return values[0].len() as i32
  }
}
";
    assert!(check_errs("borrow-pipeline-capture", src));
}
