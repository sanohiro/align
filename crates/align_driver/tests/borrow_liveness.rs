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
fn pipeline_capture_owner_invalidation_is_rejected() {
    let src = "\
fn main() -> i32 {
  mut storage := \"hello\".clone()
  view: str := storage
  value := [1].map(fn x { x + view.len() }).reduce({
    storage = \"replacement\".clone()
    0
  }, fn acc, x { acc + x })
  return value as i32
}
";
    let diagnostics = check_diagnostics("borrow-pipeline-capture-invalidation", src);
    assert!(
        diagnostics.contains("use of invalidated borrow 'view'")
            && diagnostics.contains("source 'storage' was moved or reassigned"),
        "a stage snapshot may not outlive storage invalidated by a later terminal operand:\n{diagnostics}"
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
    let diagnostics = check_diagnostics("borrow-materialized-soa", src);
    assert!(
        diagnostics.is_empty(),
        "materializing primitive SoA columns must detach them from the source array:\n{diagnostics}"
    );
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

// --- Scope-end drop: a borrow generation also ends where the source's storage is FREED, not only
// where it is moved or reassigned. The one early drop MIR emits inside a function is a `loop`'s
// per-iteration drop set (`loop_iter_drops` — the owned locals declared in the body), emitted at the
// back-edge and at every `break`. `MoveCheck::loop_moves` mirrors exactly that set.

#[test]
fn a_view_of_a_loop_body_local_is_dead_after_the_back_edge() {
    // The general shape of the hole this closed: no `unsafe`, no std handle — the owned `string` is
    // freed at the back-edge, so the next iteration's read of `keep` printed freed heap bytes.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut keep: str := \"start\"
  mut n := 0
  loop {
    n = n + keep.len() as i32
    owned := mk(\"hello\")
    keep = owned
    if n > 100 { break }
  }
  return n
}
";
    let diags = check_diagnostics("borrow-loop-back-edge-drop", src);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("was dropped at the end of the loop iteration"),
        "a view of a loop-body local read on the next pass must be rejected with the \
         drop-specific wording: {diags}"
    );
}

#[test]
fn a_view_of_a_loop_body_local_is_dead_after_the_break() {
    // The same drop set is emitted on the `break` edge, so the read *after* the loop is rejected
    // too — even though the loop body itself never re-reads the view.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut keep: str := \"start\"
  mut n := 0
  loop {
    owned := mk(\"hello\")
    keep = owned
    n = n + 1
    if n > 2 { break }
  }
  return keep.len() as i32
}
";
    assert!(check_errs("borrow-loop-break-drop", src));
}

#[test]
fn a_view_used_inside_the_iteration_that_created_it_stays_legal() {
    // The control that keeps the rule from being vacuous: the view is re-established by its own
    // `let` on every pass, so the back-edge invalidation of the previous generation is irrelevant.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut n := 0
  loop {
    owned := mk(\"hello\")
    view: str := owned
    n = n + view.len() as i32
    if n > 100 { break }
  }
  return n
}
";
    assert!(!check_errs("borrow-loop-same-iteration", src));
}

#[test]
fn a_view_of_a_source_declared_outside_the_loop_survives_iterations() {
    // `owned` outlives every iteration — it is not in the loop's drop set — so carrying its view
    // across the back-edge and out through the `break` is legal.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  owned := mk(\"hello\")
  mut keep: str := \"\"
  mut n := 0
  loop {
    n = n + keep.len() as i32
    keep = owned
    if n > 100 { break }
  }
  return keep.len() as i32
}
";
    assert!(!check_errs("borrow-loop-outer-source", src));
}

#[test]
fn an_inner_loops_break_drops_only_the_inner_bodys_locals() {
    // A `break` leaves the innermost loop only, so it must not invalidate views of the *enclosing*
    // loop body's locals — `outer` is still live after the inner loop ends.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut n := 0
  loop {
    outer := mk(\"hello\")
    mut keep: str := \"\"
    loop {
      keep = outer
      n = n + 1
      if n > 3 { break }
    }
    n = n + keep.len() as i32
    if n > 100 { break }
  }
  return n
}
";
    assert!(!check_errs("borrow-nested-loop-outer-source", src));

    // ...and the inner body's own locals *are* dropped on that edge.
    let bad = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut n := 0
  loop {
    mut keep: str := \"\"
    loop {
      inner := mk(\"hello\")
      keep = inner
      break
    }
    n = n + keep.len() as i32
    if n > 100 { break }
  }
  return n
}
";
    assert!(check_errs("borrow-nested-loop-inner-source", bad));
}

#[test]
fn an_owned_loop_body_local_broken_out_of_the_loop_is_not_a_borrow() {
    // The drop set is emitted at `break`, but a value *moved out* by that same `break` has had its
    // drop flag cleared — it is owned by the loop's result, not freed. Nothing here borrows.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut n := 0
  s := loop {
    owned := mk(\"hello\")
    n = n + 1
    if n > 2 { break owned }
  }
  return s.len() as i32
}
";
    assert!(!check_errs("borrow-loop-break-moves-owned", src));
}

/// Arena-owned loop locals stay allocated until the enclosing `arena` exits. Their views may cross
/// an iteration edge, unlike the same shape over a heap-owned local, which is dropped at the edge.
#[test]
fn over_rejects_a_view_of_an_arena_allocated_loop_local() {
    let src = "\
fn main() -> i32 {
  mut n := 0
  arena {
    mut keep: slice<i64> := [7, 7, 7][..]
    loop {
      xs := [1, 2, 3].map(fn v: i64 { v + n }).to_array()
      keep = xs[..]
      n = n + 1
      if n > 3 { break }
    }
    print(keep[0])
  }
  return 0
}
";
    let diagnostics = check_diagnostics("borrow-arena-loop-owner", src);
    assert!(
        diagnostics.is_empty(),
        "an arena-owned loop local lives until the enclosing arena exits:\n{diagnostics}"
    );

    let heap_owned = "\
fn main() -> i32 {
  mut n := 0
  arena {
    mut keep: str := \"start\"
    loop {
      owned := \"heap-owned-loop-local\".clone()
      keep = owned
      n = n + 1
      if n > 3 { break }
    }
    print(keep)
  }
  return 0
}
";
    let diagnostics = check_diagnostics("borrow-arena-loop-heap-owner", heap_owned);
    assert!(
        diagnostics.contains("use of invalidated borrow 'keep'")
            && diagnostics.contains("source 'owned' was dropped at the end of the loop iteration"),
        "a heap-owned loop local is still dropped at the iteration edge:\n{diagnostics}"
    );

    let arena_exit = "\
fn main() -> i32 {
  seed := [0]
  mut keep: slice<i64> := seed
  arena {
    source := [1, 2, 3].to_array()
    keep = source
    _ := keep.len()
  }
  return keep.len() as i32
}
";
    let diagnostics = check_diagnostics("borrow-arena-owner-exit", arena_exit);
    assert!(
        diagnostics.contains("use of invalidated borrow 'keep'"),
        "the enclosing arena exit must end its generation before a later observer:\n{diagnostics}"
    );

    for (inner_name, inner_arena) in [
        (
            "anonymous-inner",
            "    arena {\n      copied := \"outer-owned\".bytes().clone_in(outer)\n      alias = copied\n    }",
        ),
        (
            "named-inner",
            "    arena inner {\n      copied := \"outer-owned\".bytes().clone_in(outer)\n      alias = copied\n    }",
        ),
    ] {
        let retained = format!(
            "fn main() -> i32 {{\n  seed := \"seed\".bytes()\n  mut alias: slice<u8> := seed\n  arena outer {{\n    mut iteration := 0\n    loop {{\n      if iteration > 0 {{ _ := alias[0] }}\n{inner_arena}\n      iteration = iteration + 1\n      if iteration > 1 {{ break }}\n    }}\n    _ := alias[0]\n  }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("borrow-outer-clone-in-{inner_name}-retained"),
            &retained,
        );
        assert!(
            diagnostics.is_empty(),
            "clone_in(outer) formed in {inner_name} must survive that arena's exit and the loop iteration edge:\n{diagnostics}"
        );

        let ended = retained.replace(
            "    _ := alias[0]\n  }\n  return 0",
            "    _ := alias[0]\n  }\n  _ := alias[0]\n  return 0",
        );
        let diagnostics =
            check_diagnostics(&format!("borrow-outer-clone-in-{inner_name}-ended"), &ended);
        assert!(
            diagnostics.contains("use of invalidated borrow 'alias'"),
            "the outer arena exit must end clone_in(outer) formed in {inner_name}:\n{diagnostics}"
        );
    }
}

// --- The other half of a loop iteration's drop set: MIR's HIDDEN owners. A Move value with no
// binding of its own (`"…".clone()`, a call result, a just-materialized array) gets a synthetic
// owner slot whose cleanup joins the innermost loop frame, so it is freed by the same two edges as
// a named local. Sema records those as `BorrowRoot::IterTemp(depth)` — the source has no `LocalId`
// to name, which is exactly why the first cut of the scope-end-drop rule missed it.

#[test]
fn a_view_of_an_unbound_temporary_dies_at_the_iteration_edge() {
    // No helper function, no std handle, no `unsafe`: this printed freed heap bytes.
    let src = "\
fn main() -> i32 {
  mut keep: str := \"start\"
  mut n := 0
  loop {
    print(keep)
    keep = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\".clone()
    n = n + 1
    if n > 3 { break }
  }
  print(keep)
  return 0
}
";
    let diags = check_diagnostics("borrow-loop-temp-clone", src);
    assert!(
        diags.contains("it borrows a temporary value created inside the loop"),
        "a view of an unbound Move temporary must be rejected at the iteration edge, and say so \
         (the source has no name to report): {diags}"
    );
}

#[test]
fn every_shape_that_materializes_a_temporary_owner_is_covered() {
    // One assertion per way to produce an unbound Move value that a view can point into: a direct
    // call, a view-returning call *over* a temporary, `?`-unwrapping one, and a materialized array
    // sliced in place. All four printed garbage before the `IterTemp` root existed.
    let cases: [(&str, &str); 4] = [
        ("call", "keep = mk(\"AAAAAAAAAAAAAAAAAAAAAAAA\")"),
        (
            "view-of-temp",
            "keep = identity(mk(\"AAAAAAAAAAAAAAAAAAAAAAAA\"))",
        ),
        ("try", "keep = mkr(\"AAAAAAAAAAAAAAAAAAAAAAAA\")?"),
        ("array-slice", "ks = [1, 2, 3].map(dbl).to_array()[..]"),
    ];
    for (name, assign) in cases {
        let (decl, read) = if name == "array-slice" {
            ("mut ks: slice<i64> := [0][..]", "print(ks[0])")
        } else {
            ("mut keep: str := \"start\"", "print(keep)")
        };
        let src = format!(
            "fn mk(a: str) -> string = a.clone()\n\
             fn identity(s: str) -> str = s\n\
             fn mkr(a: str) -> Result<string, Error> = Ok(a.clone())\n\
             fn dbl(x: i64) -> i64 = x * 2\n\
             fn run() -> Result<i32, Error> {{\n  {decl}\n  mut n := 0\n  loop {{\n    {assign}\n    \
             n = n + 1\n    if n > 3 {{ break }}\n  }}\n  {read}\n  return Ok(0)\n}}\n\
             fn main() -> i32 = 0\n"
        );
        assert!(
            check_errs(&format!("borrow-loop-temp-{name}"), &src),
            "the `{name}` temporary must not outlive its iteration"
        );
    }
}

#[test]
fn a_temporary_created_outside_a_loop_lives_to_function_exit() {
    // The control for the depth rule: with no loop, the hidden owner is dropped only by the exit
    // cleanup, and nothing outlives that — so binding a view to a bare temporary stays legal.
    let src = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  v: str := mk(\"hello\")
  print(v)
  return v.len() as i32
}
";
    assert!(!check_errs("borrow-temp-no-loop", src));

    // ...and inside a loop, a temporary used only within the iteration that created it is fine too.
    let same_pass = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut n := 0
  loop {
    v: str := mk(\"hello\")
    n = n + v.len() as i32
    if n > 100 { break }
  }
  return n
}
";
    assert!(!check_errs("borrow-temp-same-iteration", same_pass));
}

/// **KNOWN OVER-REJECTION, PRE-EXISTING — pinned here because the loop rule widened its reach.**
///
/// `ch[0]` is a `{ptr,len}` view into `data`, not into the chunks header: dropping (or reassigning)
/// `ch` frees the header array only, and the bytes `keep` points at belong to `data`, which
/// outlives both. `local_owns_view_storage` nevertheless counts a `DynSliceArray` local as owning
/// its elements' storage, so the header contributes itself as a root and any end of its generation
/// invalidates the element views.
///
/// This predates the scope-end-drop rule — the plain reassignment below is rejected on the same
/// path `MoveCheck` has always had — but that rule made the far more common loop shape hit it too.
/// The fix belongs with `local_owns_view_storage`, not here; changing it is a type-class question
/// (which owned containers actually own the bytes their elements view) that deserves its own slice.
#[test]
fn over_rejects_a_view_into_the_source_of_a_dropped_chunks_header() {
    // Pre-existing: no loop, just a reassignment of the header.
    let reassign = "\
fn dbl(x: i64) -> i64 = x * 2
fn main() -> i32 {
  data := [1, 2, 3, 4].map(dbl).to_array()
  mut ch := data.chunks(2)
  keep := ch[0]
  ch = data.chunks(2)
  return keep[0] as i32
}
";
    assert!(
        check_errs("borrow-chunks-reassign-over-reject", reassign),
        "KNOWN OVER-REJECTION (pre-existing): `keep` views `data`, which is still live. If this now \
         checks, `local_owns_view_storage` stopped over-claiming the header — flip both assertions."
    );

    // The loop shape the scope-end-drop rule exposed, same root cause.
    let in_loop = "\
fn dbl(x: i64) -> i64 = x * 2
fn main() -> i32 {
  data := [1, 2, 3, 4].map(dbl).to_array()
  mut keep: slice<i64> := data[..]
  mut n := 0
  loop {
    ch := data.chunks(2)
    keep = ch[0]
    n = n + 1
    if n > 3 { break }
  }
  return keep[0] as i32
}
";
    assert!(check_errs("borrow-chunks-loop-over-reject", in_loop));
}

/// **The borrow-vs-move split, and the false positive that proved it is needed.**
///
/// A hidden owner exists only where MIR *borrows* a fresh Move value (`lower_borrowed_owned`).
/// Moving one into a local that OWNS it transfers the storage to that named local instead — nothing
/// joins the loop's drop set. `names` below is declared outside the loop and owns its array; the
/// array's `str` elements view `src`, which also outlives the loop. Nothing is freed at the
/// back-edge, and the program prints `aa` / `bb`.
///
/// The first cut of the temporary rule added the root at the top of `borrow_sources`, which every
/// materializing consumer also reaches — so this idiom ("rebuild a collection each pass, use the
/// latest after the loop") was rejected. The root now comes from `storage_roots`, the borrowing
/// position, which a move never passes through.
#[test]
fn a_move_into_an_owning_local_is_not_a_borrow_of_a_temporary() {
    let src = "\
fn up(s: str) -> str = s
fn main() -> i32 {
  src := [\"aa\", \"bb\"]
  mut names: array<str> := src.map(up).to_array()
  mut n := 0
  loop {
    names = src.map(up).to_array()
    n = n + 1
    if n > 3 { break }
  }
  print(names[0])
  return names[1].len() as i32
}
";
    let diags = check_diagnostics("borrow-loop-move-into-owner", src);
    assert!(
        !diags.contains("invalidated borrow"),
        "a fresh array MOVED into an owning local is not a borrowed temporary: {diags}"
    );

    // The same shape one step further: a struct that owns a `string` and views a `str`.
    let owned_struct = "\
Rec { name: string, tag: str }
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  tag := \"t\"
  mut r := Rec { name: mk(\"a\"), tag: tag }
  mut n := 0
  loop {
    r = Rec { name: mk(\"b\"), tag: tag }
    n = n + 1
    if n > 3 { break }
  }
  return r.name.len() as i32
}
";
    assert!(!check_errs(
        "borrow-loop-move-struct-into-owner",
        owned_struct
    ));
}

#[test]
fn a_json_view_over_a_temporary_input_dies_at_the_iteration_edge() {
    // The borrowing side of the same split: `json.doc` VIEWS its input, so a doc over a temporary
    // — or over a `string` local the iteration drops — cannot outlive the pass that made it.
    let over_temp = "\
import core.json
fn mk(a: str) -> string = a.clone()
fn main() -> Result<(), Error> {
  arena {
    mut keep: str := \"x\"
    mut n := 0
    loop {
      d := json.doc(mk(\"{\\\"a\\\": \\\"hello world\\\"}\"))?
      keep = d.get(\"a\").as_str() else { \"\" }
      n = n + 1
      if n > 3 { break }
    }
    print(keep)
  }
  return Ok(())
}
";
    assert!(check_errs("borrow-json-temp-input", over_temp));

    // Same, with the input bound to a loop-body local — reported against the name.
    let over_local = "\
import core.json
fn mk(a: str) -> string = a.clone()
fn main() -> Result<(), Error> {
  arena {
    mut keep: str := \"x\"
    mut n := 0
    loop {
      src := mk(\"{\\\"a\\\": \\\"hello world\\\"}\")
      d := json.doc(src)?
      keep = d.get(\"a\").as_str() else { \"\" }
      n = n + 1
      if n > 3 { break }
    }
    print(keep)
  }
  return Ok(())
}
";
    let diags = check_diagnostics("borrow-json-local-input", over_local);
    assert!(
        diags.contains("its source 'src' was dropped at the end of the loop iteration"),
        "a doc over a dropped loop-body input must be rejected against that name: {diags}"
    );
}

/// **A wrapper must not launder a borrow.** `may_need_synthetic_owner` is transparent through
/// `{ }` / `unsafe { }` — a block whose value is a bound place borrows that place and mints no
/// hidden owner — so `storage_roots` has to reach the same place. Until it did, a block recorded
/// **no** root at all: not `IterTemp` (the predicate correctly said "not a temporary") and not the
/// place's `Local` (the fallback's `borrow_sources` short-circuits on an owned, non-borrowing type).
/// `keep = { inner }` therefore walked straight past the whole scope-end-drop rule and printed freed
/// heap bytes — two characters away from the rejected `keep = inner`.
///
/// `arena {}` remains region-bound. A `task_group` bound tail place is transparent for ordinary
/// storage roots because the group owns task storage only; a fresh tail still takes `IterTemp`.
#[test]
fn a_block_wrapper_does_not_launder_a_view_of_a_dropped_source() {
    let cases: [(&str, &str); 7] = [
        ("bare", "keep = { inner }"),
        (
            "decl-inside",
            "keep = { made := mk(\"AAAAAAAAAAAAAAAA\"); made }",
        ),
        ("unsafe", "keep = unsafe { inner }"),
        ("task-group", "keep = task_group { inner }"),
        (
            "task-group-decl-inside",
            "keep = task_group { made := mk(\"AAAAAAAAAAAAAAAA\"); made }",
        ),
        ("through-call", "keep = identity({ inner })"),
        ("nested", "keep = { { { inner } } }"),
    ];
    for (name, assign) in cases {
        let src = format!(
            "fn mk(a: str) -> string = a.clone()\n\
             fn identity(s: str) -> str = s\n\
             fn main() -> i32 {{\n  mut keep: str := \"start\"\n  mut n := 0\n  loop {{\n    \
             inner := mk(\"AAAAAAAAAAAAAAAA\")\n    {assign}\n    n = n + 1\n    \
             if n > 3 {{ break }}\n  }}\n  print(keep)\n  return 0\n}}\n"
        );
        assert!(
            check_errs(&format!("borrow-block-launder-{name}"), &src),
            "a `{name}` block wrapper must not hide that the borrow's source dies with the iteration"
        );
    }

    // A place *reached through* a block — the field of a Move struct the iteration drops.
    let field = "\
Holder { text: string }
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  mut keep: str := \"start\"
  mut n := 0
  loop {
    h := Holder { text: mk(\"AAAAAAAAAAAAAAAA\") }
    keep = { h.text }
    n = n + 1
    if n > 3 { break }
  }
  print(keep)
  return 0
}
";
    assert!(check_errs("borrow-block-launder-field", field));

    // The control: a block over a source that OUTLIVES the loop stays legal — transparency must not
    // turn into blanket rejection.
    let ok = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  outer := mk(\"hello\")
  mut keep: str := \"start\"
  mut n := 0
  loop {
    keep = { outer }
    n = n + 1
    if n > 3 { break }
  }
  print(keep)
  return 0
}
";
    assert!(!check_errs("borrow-block-outer-source", ok));

    let task_group_ok = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  outer := mk(\"hello\")
  mut keep: str := \"start\"
  mut n := 0
  loop {
    keep = task_group { outer }
    n = n + 1
    if n > 3 { break }
  }
  print(keep)
  return 0
}
";
    assert!(!check_errs("borrow-task-group-outer-source", task_group_ok));
}

/// **KNOWN OVER-REJECTION — the third of the family, pinned so a reader doesn't assume otherwise.**
///
/// `may_need_synthetic_owner` is conservatively `true` for the wrappers whose *runtime* value can
/// still be a bound place — `if`, `match`, `else`-unwrap, `arena {}`, or control flow in a
/// `task_group` tail — so borrowing
/// one over sources declared OUTSIDE the loop mints a spurious `IterTemp`. MIR proves it safe: the
/// synthetic owner's temporary flag is stored `false` on every bound-arm path, so the drop-if-live
/// folds away and neither loop edge emits a drop (the only drops are the sources at function exit).
///
/// This is the same family as the arena and chunks pins: sema uses a static shape predicate where
/// MIR gates the free on a per-path runtime flag (`temporary_drop_flag`) that `MoveCheck` cannot
/// see. Deliberately conservative — making the arms already-`str` views is accepted and runs.
/// Widening it means giving borrow liveness the ownership bit, i.e. the recorded structural
/// follow-up (borrow liveness in the checked-HIR escape CFG).
#[test]
fn over_rejects_a_control_flow_borrow_over_outer_bound_places() {
    let if_over_outer = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  a := mk(\"AAAA-OUTER\")
  b := mk(\"BBBB-OUTER\")
  mut keep: str := \"start\"
  mut n := 0
  loop {
    keep = if n > 1 { a } else { b }
    n = n + 1
    if n > 3 { break }
  }
  print(keep)
  return 0
}
";
    assert!(
        check_errs("borrow-if-over-outer-places", if_over_outer),
        "KNOWN OVER-REJECTION: both arms are outer locals, so nothing is freed at the edge. If this \
         now checks, borrow liveness gained the per-path ownership bit — flip this assertion."
    );

    // The workaround, and the control that the rule is not simply rejecting all `if`-borrows: with
    // the arms already `str` views there is no owned value to mint an owner for.
    let via_views = "\
fn mk(a: str) -> string = a.clone()
fn main() -> i32 {
  a := mk(\"AAAA-OUTER\")
  b := mk(\"BBBB-OUTER\")
  va: str := a
  vb: str := b
  mut keep: str := \"start\"
  mut n := 0
  loop {
    keep = if n > 1 { va } else { vb }
    n = n + 1
    if n > 3 { break }
  }
  print(keep)
  return 0
}
";
    assert!(!check_errs("borrow-if-over-outer-views", via_views));
}

// ─────────────────────────────────────────────────────────────────────────────────────────────
// The hidden owner a `template` mints for ITSELF, and the two siblings the same fail-open tail
// swallowed. All three were accepted on `main` and read freed memory at runtime.
// ─────────────────────────────────────────────────────────────────────────────────────────────

/// `template "…"` is the one expression whose value (`str`) views storage the expression itself
/// allocates: MIR mints a hidden owned `string` at the node, unconditionally, and that owner joins
/// the innermost loop's per-iteration drops. `MoveCheck::temp_owner_root` could not see it — it
/// keys on `needs_drop_flag` of the value's OWN type, and a `str` is not droppable — and
/// `borrow_sources_inner` had no `Template` arm, so the fail-open `_` tail gave the result no
/// provenance at all. `region_of(Template) = Frame` blocks a `return` but is not borrow provenance,
/// so the loop-edge invalidation never fired and this checked ok while reading freed heap.
#[test]
fn a_str_accumulated_by_a_template_across_loop_iterations_is_rejected() {
    let src = "\
fn main() -> i32 {
  mut acc: str := \"start\"
  mut c := 0
  loop {
    acc = template \"{acc}-{c}\"
    c = c + 1
    if c >= 5 { break }
  }
  print(acc)
  return 0
}
";
    let diags = check_diagnostics("borrow-template-accumulator", src);
    assert!(
        diags.contains("use of invalidated borrow 'acc'")
            && diags.contains("it borrows a temporary value created inside the loop"),
        "a `str` bound from a `template` must die on the loop edge that frees the template's \
         hidden owner, with the existing temporary wording: {diags}"
    );
}

/// The owned-`string`-hole spelling of the same loop — reachable through #620's new capability
/// (interpolating an owned `string` borrows it to a `str`). The hole is the template's own hidden
/// owner, not `h`, so the message is the same temporary one.
#[test]
fn a_template_over_an_owned_string_hole_escaping_the_loop_is_rejected() {
    let src = "\
fn main() -> i32 {
  mut keep: str := \"start\"
  mut c := 0
  loop {
    h := \"hello\".clone()
    keep = template \"v={h}\"
    c = c + 1
    if c >= 3 { break }
  }
  print(keep)
  return 0
}
";
    let diags = check_diagnostics("borrow-template-owned-hole", src);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("it borrows a temporary value created inside the loop"),
        "the owned-`string`-hole spelling is the same hidden-owner hole: {diags}"
    );
}

/// `json.encode` desugars to `ExprKind::Template`, so it inherits the fix rather than needing its
/// own rule — the reason the provenance edge belongs on the node and not on the surface syntax.
#[test]
fn json_encode_escaping_a_loop_is_rejected_like_its_template_desugaring() {
    let src = "\
import core.json
P { a: i64 }
fn main() -> i32 {
  mut keep: str := \"start\"
  mut c := 0
  loop {
    p := P { a: c }
    keep = json.encode(p)
    c = c + 1
    if c >= 3 { break }
  }
  print(keep)
  return 0
}
";
    let diags = check_diagnostics("borrow-json-encode-loop", src);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("it borrows a temporary value created inside the loop"),
        "`json.encode` is a `template`, so it must be rejected identically: {diags}"
    );
}

/// **The controls.** A template's storage dies at the iteration edge, and nowhere else — every one
/// of these was legal before the fix and must stay legal. In particular the last two: MIR mints NO
/// hidden owner inside an `arena {}` (the bytes are bump-allocated and live to the arena's end), so
/// `MoveCheck` mirrors that with its own arena depth. Without that mirror an arena-scoped
/// accumulator — the idiomatic way to write the rejected loop above — would have been rejected too.
#[test]
fn a_template_that_does_not_outlive_its_iteration_stays_legal() {
    let cases: [(&str, &str); 8] = [
        (
            "same-iteration bind and use",
            "\
fn main() -> i32 {
  mut c := 0
  loop {
    s := template \"iter {c}\"
    print(s)
    c = c + 1
    if c >= 3 { break }
  }
  return 0
}
",
        ),
        (
            "outside any loop",
            "\
fn main() -> i32 {
  c := 7
  s := template \"n={c}\"
  print(s)
  return 0
}
",
        ),
        (
            "literal-only holes",
            "\
fn main() -> i32 {
  mut c := 0
  loop {
    s := template \"no holes at all\"
    print(s)
    c = c + 1
    if c >= 3 { break }
  }
  return 0
}
",
        ),
        (
            "a template is a COPY of its holes, not a view of them",
            "\
fn main() -> i32 {
  mut h := \"hello\".clone()
  t := template \"v={h}\"
  h = \"world\".clone()
  print(t)
  print(h)
  return 0
}
",
        ),
        (
            "written straight into a builder",
            "\
fn main() -> i32 {
  mut c := 0
  loop {
    mut b := builder()
    b.write(template \"row {c}\")
    print(b.to_string())
    c = c + 1
    if c >= 3 { break }
  }
  return 0
}
",
        ),
        (
            "an outer loop's template read by an inner loop",
            "\
fn main() -> i32 {
  mut c := 0
  loop {
    s := template \"outer {c}\"
    mut d := 0
    loop {
      print(s)
      d = d + 1
      if d >= 2 { break }
    }
    c = c + 1
    if c >= 2 { break }
  }
  return 0
}
",
        ),
        (
            "an arena-scoped accumulator (the arena outlives the loop)",
            "\
fn main() -> i32 {
  arena {
    mut acc: str := \"start\"
    mut c := 0
    loop {
      acc = template \"{acc}-{c}\"
      c = c + 1
      if c >= 5 { break }
    }
    print(acc)
  }
  return 0
}
",
        ),
        (
            "an arena opened inside the loop",
            "\
fn main() -> i32 {
  mut c := 0
  loop {
    arena {
      s := template \"iter {c}\"
      print(s)
    }
    c = c + 1
    if c >= 3 { break }
  }
  return 0
}
",
        ),
    ];
    for (what, src) in cases {
        let diags = check_diagnostics("borrow-template-legal", src);
        assert!(diags.is_empty(), "{what} must stay legal, got: {diags}");
    }

    // And the arena accumulator really does accumulate — the control against "legal but wrong".
    let out = build_and_run(
        "borrow-template-arena-accumulator",
        "\
fn main() -> i32 {
  arena {
    mut acc: str := \"start\"
    mut c := 0
    loop {
      acc = template \"{acc}-{c}\"
      c = c + 1
      if c >= 5 { break }
    }
    print(acc)
  }
  return 0
}
",
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "start-0-1-2-3-4"
    );
}

/// **The second hole the exhaustiveness sweep found.** A sum-type constructor is the exact sibling
/// of `StructLit` / `Tuple` / `OptionSome`, all three of which forward their operands' provenance —
/// `EnumValue` was the one aggregate constructor the `_` tail swallowed, so `C.Text(view)` laundered
/// the borrow and outlived the `string` the view pointed into. Accepted on `main`, printed garbage.
#[test]
fn a_sum_type_carrying_a_view_of_a_dropped_loop_local_is_rejected() {
    let src = "\
C { Text(str), Num(i64) }
fn main() -> i32 {
  mut keep: C := C.Num(0)
  mut c := 0
  loop {
    h := \"hello-world\".clone()
    keep = C.Text(h.trim())
    c = c + 1
    if c >= 3 { break }
  }
  match keep {
    Text(s) => print(s),
    Num(n) => print(n),
  }
  return 0
}
";
    let diags = check_diagnostics("borrow-enum-payload-loop-local", src);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("its source 'h'")
            && diags.contains("was dropped at the end of the loop iteration"),
        "a sum-type payload borrows what it was constructed from, and must name that source: \
         {diags}"
    );

    // The control: the same constructor over a source that outlives the loop stays legal.
    let legal = "\
C { Text(str), Num(i64) }
fn label(c: C) -> i64 = match c { Text(s) => s.len(), Num(n) => n }
fn main() -> i32 {
  xs := [\"alpha\", \"beta\"]
  mut i := 0
  mut total := 0
  loop {
    v := C.Text(xs[i])
    total = total + label(v)
    i = i + 1
    if i >= 2 { break }
  }
  print(total)
  return 0
}
";
    assert!(!check_errs("borrow-enum-payload-outer-source", legal));
}

/// **The third.** `r.sample(xs, k)` copies element VALUES into a fresh owned array — with
/// `slice<str>` elements those values are views, so the result borrows exactly what `xs` borrows.
/// That is the `.to_array()` shape, which does forward provenance; `RandSample` did not.
#[test]
fn a_sampled_array_of_views_into_a_dropped_loop_local_is_rejected() {
    let src = "\
import std.rand
fn main() -> i32 {
  mut r := rand.seed()
  mut keep: array<str> := r.sample([\"x\", \"y\"][0..2], 1)
  mut c := 0
  loop {
    h := \"hello-world\".clone()
    xs := [h.trim(), h.trim()]
    keep = r.sample(xs[0..2], 1)
    c = c + 1
    if c >= 3 { break }
  }
  print(keep[0])
  return 0
}
";
    let diags = check_diagnostics("borrow-rand-sample-loop-local", src);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("its source 'h'")
            && diags.contains("was dropped at the end of the loop iteration"),
        "a sampled `array<str>` holds views into its source and must inherit its provenance: \
         {diags}"
    );

    // The control: sampling views of a source that outlives the loop stays legal.
    let legal = "\
import std.rand
fn main() -> i32 {
  mut r := rand.seed()
  names := [\"alpha\", \"beta\", \"gamma\"]
  mut keep: array<str> := r.sample(names[0..3], 1)
  mut c := 0
  loop {
    keep = r.sample(names[0..3], 1)
    c = c + 1
    if c >= 3 { break }
  }
  print(keep[0].len())
  return 0
}
";
    assert!(!check_errs("borrow-rand-sample-outer-source", legal));
}

/// **The `task_group` half of the arena mirror, pinned rather than left to the region rule.**
/// `MoveCheck::arena_depth` counts `arena` ONLY, unlike the two identically-named region counters in
/// `align_sema` which also count `task_group` — because MIR keeps task groups on a stack separate
/// from `Builder::arenas`, so a `template` inside one still gets its hidden owned `string` and still
/// dies on the enclosing loop's edge. On `main` this shape reported only the region errors; the
/// borrow error is the one that would vanish if someone "harmonized" the three counters, so it is
/// asserted specifically.
#[test]
fn a_template_returned_out_of_a_task_group_in_a_loop_is_rejected() {
    // The assignment is INSIDE the `task_group`, which is what makes this row sensitive to the
    // counter: the walk is what maintains `arena_depth`, so counting `task_group` there would
    // suppress the root here. (The tail-value spelling below goes through the `borrow_sources`
    // query instead, which reads the outer depth — see the non-lexical note on that arm.)
    let inside = "\
fn main() -> i32 {
  mut keep: str := \"start\"
  mut c := 0
  loop {
    task_group {
      keep = template \"v={c}\"
    }
    c = c + 1
    if c >= 3 { break }
  }
  print(keep)
  return 0
}
";
    let diags = check_diagnostics("borrow-template-task-group", inside);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("it borrows a temporary value created inside the loop"),
        "a `task_group` is NOT an arena for this rule — the template's hidden owner still dies on \
         the loop edge, and the borrow diagnostic must say so rather than leaving the shape to the \
         region rule: {diags}"
    );

    let as_the_value = "\
fn main() -> i32 {
  mut keep: str := \"start\"
  mut c := 0
  loop {
    keep = task_group { template \"v={c}\" }
    c = c + 1
    if c >= 3 { break }
  }
  print(keep)
  return 0
}
";
    let diags = check_diagnostics("borrow-template-task-group-value", as_the_value);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("it borrows a temporary value created inside the loop"),
        "the tail-value spelling must be rejected too: {diags}"
    );
}

/// **The blast radius, documented with one row.** The `Template` arm is provenance on the *node*,
/// so every way of laundering that `str` onward inherits it: a tuple, `Some`/`Ok`, an `if`/`match`
/// value, a match-payload binding, a struct literal or a later field/tuple-index read of one,
/// destructuring, `else`-unwrap, a nested template, `.trim()`, slicing, `.bytes()`. All of those
/// checked ok on `main` and read freed heap. This pins the struct-literal spelling — the shape that
/// most looks like it should be safe, because the `str` is stored into a *field* rather than
/// assigned — as the representative.
#[test]
fn a_template_laundered_through_a_struct_field_is_rejected() {
    let src = "\
S { a: str }
fn main() -> i32 {
  mut keep: S := S { a: \"start\" }
  mut c := 0
  loop {
    keep = S { a: template \"v={c}\" }
    c = c + 1
    if c >= 3 { break }
  }
  print(keep.a)
  return 0
}
";
    let diags = check_diagnostics("borrow-template-struct-field", src);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("it borrows a temporary value created inside the loop"),
        "storing the template's `str` into a struct field must carry its provenance, not launder \
         it: {diags}"
    );
}

#[test]
fn storage_generation_move_replacement_cleanup_control_matrix() {
    let mut failures = Vec::new();

    for (name, src, expected_invalidated) in [
        (
            "move-preserves-alias",
            "fn main() -> i32 {\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  moved := source\n  _ := moved.len()\n  _ := alias.len()\n  return 0\n}\n",
            false,
        ),
        (
            "replacement-ends-alias",
            "fn main() -> i32 {\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  source = [3, 4].to_array()\n  _ := alias.len()\n  return 0\n}\n",
            true,
        ),
        (
            "replacement-reborrow",
            "fn main() -> i32 {\n  mut source := [1, 2].to_array()\n  mut alias: slice<i64> := source\n  source = [3, 4].to_array()\n  alias = source\n  _ := alias.len()\n  return 0\n}\n",
            false,
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("storage-generation-{name}"), src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'alias'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "move/replacement case `{name}` had the wrong generation result:\n{diagnostics}"
            ));
        }
    }

    for (name, src) in [
        (
            "dynamic-local",
            "fn main() -> i32 {\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  source = source\n  _ := alias.len()\n  return 0\n}\n",
        ),
        (
            "fixed-local",
            "fn main() -> i32 {\n  mut source := [1, 2]\n  alias: slice<i64> := source\n  source = source\n  _ := alias.len()\n  return 0\n}\n",
        ),
        (
            "copy-view",
            "fn main() -> i32 {\n  mut source := [1, 2]\n  mut view: slice<i64> := source\n  alias := view\n  view = view\n  _ := alias.len()\n  return 0\n}\n",
        ),
        (
            "copy-view-field",
            "Holder { values: slice<i64> }\nfn main() -> i32 {\n  source := [1, 2]\n  mut holder := Holder { values: source }\n  alias := holder.values\n  holder.values = holder.values\n  _ := alias.len()\n  return 0\n}\n",
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("storage-generation-self-{name}"), src);
        if !diagnostics.is_empty() {
            failures.push(format!(
                "exact self-assignment `{name}` must be a generation no-op:\n{diagnostics}"
            ));
        }
    }

    for (name, src, expected) in [
        (
            "transparent-wrapper",
            "fn main() -> i32 {\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  source = { source }\n  _ := alias.len()\n  return 0\n}\n",
            "use of invalidated borrow 'alias'",
        ),
        (
            "different-place",
            "fn main() -> i32 {\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  replacement := [3, 4].to_array()\n  source = replacement\n  _ := alias.len()\n  return 0\n}\n",
            "use of invalidated borrow 'alias'",
        ),
        (
            "projected-owned-rejected",
            "Holder { values: array<i64> }\nfn main() -> i32 {\n  mut holder := Holder { values: [1, 2].to_array() }\n  holder.values = holder.values\n  return 0\n}\n",
            "field replacement of array<i64> is not supported yet",
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("storage-generation-self-{name}"), src);
        if !diagnostics.contains(expected) {
            failures.push(format!(
                "non-exact or unsupported self-assignment control `{name}` must retain ordinary assignment behavior:\n{diagnostics}"
            ));
        }
    }

    for (shape, action) in [
        (
            "aggregate",
            "_ := Holder { values: source, ignored: {sink} }",
        ),
        ("call", "_ := consume(source, {sink})"),
    ] {
        for (sink_name, sink) in [
            ("later-return", "{ _ := alias.len()\n    return 7\n  }"),
            ("later-exit", "{ _ := alias.len()\n    process.exit(7)\n  }"),
            (
                "later-abort",
                "{ _ := alias.len()\n    process.abort()\n  }",
            ),
        ] {
            let action = action.replace("{sink}", sink);
            let process_import = if sink_name == "later-return" {
                ""
            } else {
                "import std.process\n"
            };
            let src = format!(
                "{process_import}Holder {{ values: array<i64>, ignored: i32 }}\nfn consume(values: array<i64>, ignored: i32) -> i32 = ignored\nfn probe() -> i32 {{\n  source := [1, 2].to_array()\n  alias: slice<i64> := source\n  {action}\n}}\nfn main() -> i32 = 0\n"
            );
            let diagnostics = check_diagnostics(
                &format!("storage-generation-staged-{shape}-{sink_name}"),
                &src,
            );
            if !diagnostics.is_empty() {
                failures.push(format!(
                    "the source-order owner for staged {shape} with {sink_name} must not perform its Move action before the later operand:\n{diagnostics}"
                ));
            }
        }
    }

    let staged_shapes = [
        ("direct-call", "_ := consume(source, {sink})"),
        (
            "fn-value-call",
            "callee := consume\n  _ := callee(source, {sink})",
        ),
        ("struct", "_ := Holder { values: source, ignored: {sink} }"),
        ("tuple", "_ := (source, {sink})"),
        ("user-sum", "_ := Choice.Pair(source, {sink})"),
    ];
    let staged_prelude = "Holder { values: array<i64>, ignored: i64 }\nChoice { Pair(array<i64>, i64), Empty }\nfn consume(values: array<i64>, ignored: i64) -> i64 = ignored\n";

    for &(shape, action) in &staged_shapes {
        let nonfallthrough = action.replace("{sink}", "{ _ := source.len()\n    return 7; 0\n  }");
        let src = format!(
            "{staged_prelude}fn probe() -> i32 {{\n  source := [1, 2].to_array()\n  {nonfallthrough}\n}}\nfn main() -> i32 = 0\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-staged-bound-{shape}-later-return"),
            &src,
        );
        if !diagnostics.is_empty() {
            failures.push(format!(
                "a bound Move in {shape} must remain available to a later non-fallthrough operand until the parent action completes:\n{diagnostics}"
            ));
        }

        let successful = action.replace("{sink}", "0");
        let src = format!(
            "{staged_prelude}fn main() -> i32 {{\n  source := [1, 2].to_array()\n  {successful}\n  return source.len() as i32\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-staged-bound-{shape}-success"),
            &src,
        );
        if !diagnostics.contains("use of moved value 'source'") {
            failures.push(format!(
                "a successful {shape} must perform the staged bound Move after every operand completes:\n{diagnostics}"
            ));
        }
    }

    for &(shape, action) in &staged_shapes {
        let fallthrough = action.replace("{sink}", "{ source = [3, 4].to_array()\n    0\n  }");
        let src = format!(
            "{staged_prelude}fn main() -> i32 {{\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  {fallthrough}\n  _ := source.len()\n  _ := alias.len()\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-staged-rebind-{shape}-fallthrough"),
            &src,
        );
        if !diagnostics.contains("use of invalidated borrow 'alias'")
            || diagnostics.contains("use of moved value 'source'")
        {
            failures.push(format!(
                "a later fallthrough rebind in {shape} must invalidate the completed old snapshot without consuming the rebound local:\n{diagnostics}"
            ));
        }

        let nonfallthrough = action.replace(
            "{sink}",
            "{ source = [3, 4].to_array()\n    _ := source.len()\n    return 7; 0\n  }",
        );
        let src = format!(
            "{staged_prelude}fn probe() -> i32 {{\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  {nonfallthrough}\n}}\nfn main() -> i32 = 0\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-staged-rebind-{shape}-later-return"),
            &src,
        );
        if !diagnostics.is_empty() {
            failures.push(format!(
                "a later non-fallthrough rebind in {shape} must publish neither its old snapshot action nor a phantom Move of the rebound local:\n{diagnostics}"
            ));
        }
    }

    for (shape, action) in [
        (
            "aggregate",
            "_ := Holder { values: [1, 2].to_array(), ignored: { return 7 } }",
        ),
        ("call", "_ := consume([1, 2].to_array(), { return 7 })"),
    ] {
        let src = format!(
            "Holder {{ values: array<i64>, ignored: i32 }}\nfn consume(values: array<i64>, ignored: i32) -> i32 = ignored\nfn probe() -> i32 {{\n  {action}\n}}\nfn main() -> i32 = 0\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-staged-{shape}-fresh-first-return"),
            &src,
        );
        if !diagnostics.is_empty() {
            failures.push(format!(
                "a fresh first operand followed by a non-fallthrough operand must leave no staged action ({shape}):\n{diagnostics}"
            ));
        }
    }

    for (shape, action) in [
        ("aggregate", "_ := Holder { values: source, ignored: 0 }"),
        ("call", "_ := consume(source, 0)"),
    ] {
        let src = format!(
            "Holder {{ values: array<i64>, ignored: i32 }}\nfn consume(values: array<i64>, ignored: i32) -> i32 = ignored\nfn main() -> i32 {{\n  source := [1, 2].to_array()\n  {action}\n  return source.len() as i32\n}}\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-staged-{shape}-successful-action"),
            &src,
        );
        if !diagnostics.contains("use of moved value 'source'") {
            failures.push(format!(
                "a successful staged {shape} must perform exactly one source Move after all operands complete:\n{diagnostics}"
            ));
        }
    }

    for (name, operator, rhs, expected_invalidated) in [
        (
            "and-joined-replacement",
            "&&",
            "{ source = [3, 4].to_array()\n    true\n  }",
            true,
        ),
        (
            "and-nonfallthrough",
            "&&",
            "{ source = [3, 4].to_array()\n    return 1\n  }",
            false,
        ),
        (
            "or-nonfallthrough",
            "||",
            "{ source = [3, 4].to_array()\n    return 1\n  }",
            false,
        ),
    ] {
        let src = format!(
            "fn probe(condition: bool) -> i32 {{\n  mut source := [1, 2].to_array()\n  alias: slice<i64> := source\n  _ := condition {operator} {rhs}\n  _ := alias.len()\n  return 0\n}}\nfn main() -> i32 = 0\n"
        );
        let diagnostics =
            check_diagnostics(&format!("storage-generation-short-circuit-{name}"), &src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'alias'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "short-circuit case `{name}` joined the wrong post-expression state:\n{diagnostics}"
            ));
        }
    }

    for (name, body, expected_invalidated) in [
        (
            "ok-forwards-receiver",
            "mut owner := \"ok\".clone()\n  view: str := owner\n  result: Result<str, ViewError> := Ok(view)\n  mapped := result.map_err(keep_error)\n  owner = \"replacement\".clone()\n  _ := match mapped { Ok(value) => value.len(), Err(_) => 0 }",
            true,
        ),
        (
            "ok-does-not-select-mapper-capture",
            "mut owner := \"capture\".clone()\n  view: str := owner\n  result: Result<str, ViewError> := Ok(\"fixed\")\n  mapper := fn _: ViewError { ViewError.Text(view) }\n  mapped := result.map_err(mapper)\n  owner = \"replacement\".clone()\n  _ := match mapped { Ok(value) => value.len(), Err(_) => 0 }",
            false,
        ),
        (
            "err-selects-mapper-capture",
            "mut owner := \"capture\".clone()\n  view: str := owner\n  result: Result<str, ViewError> := Err(ViewError.Fixed)\n  mapper := fn _: ViewError { ViewError.Text(view) }\n  mapped := result.map_err(mapper)\n  owner = \"replacement\".clone()\n  _ := match mapped { Ok(value) => value.len(), Err(error) => match error { Text(value) => value.len(), Fixed => 0 } }",
            true,
        ),
    ] {
        let src = format!(
            "ViewError {{ Text(str), Fixed }}\nfn keep_error(error: ViewError) -> ViewError = error\nfn main() -> i32 {{\n  {body}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("storage-generation-map-err-{name}"), &src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'mapped'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "ResultMapErr case `{name}` selected the wrong projected owner:\n{diagnostics}"
            ));
        }
    }

    let owned_err = "\
fn replace(values: array<i64>) -> array<i64> = [values.len()].to_array()
fn main() -> i32 {
  source := [1, 2].to_array()
  alias: slice<i64> := source
  result: Result<(), array<i64>> := Err(source)
  mapped := result.map_err(replace)
  _ := match mapped { Ok(_) => 0, Err(values) => values.len() }
  _ := alias.len()
  return 0
}
";
    let diagnostics = check_diagnostics("storage-generation-map-err-owned-input", owned_err);
    if !diagnostics.contains("use of invalidated borrow 'alias'") {
        failures.push(format!(
            "ResultMapErr must consume the owned Err input while publishing a distinct usable mapper result:\n{diagnostics}"
        ));
    }

    let terminating_mapper = "\
import std.process
fn discard(values: array<i64>) -> i64 = values.len()
fn probe() -> i32 {
  source := [1, 2].to_array()
  alias: slice<i64> := source
  result: Result<(), array<i64>> := Err(source)
  _ := result.map_err(if false { discard } else {
    _ := alias.len()
    process.abort()
  })
  return 0
}
fn main() -> i32 = 0
";
    let diagnostics = check_diagnostics(
        "storage-generation-map-err-terminating-mapper",
        terminating_mapper,
    );
    if !diagnostics.is_empty() {
        failures.push(format!(
            "a non-fallthrough mapper operand must leave its completed Err input live and publish no call action or result generation:\n{diagnostics}"
        ));
    }

    for (name, read_prior, expected_invalidated) in [
        ("latest-current", "", false),
        ("ended-prior", "  _ := prior.len()\n", true),
    ] {
        let src = format!(
            "fn main() -> i32 {{\n  mut values := [0].to_array()\n  mut prior: slice<i64> := values\n  mut latest: slice<i64> := values\n  mut n := 0\n  loop {{\n    values = [n].to_array()\n    if n == 0 {{\n      prior = values\n    }} else {{\n      latest = values\n      break\n    }}\n    n = n + 1\n  }}\n  _ := latest.len()\n{read_prior}  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("storage-generation-recency-{name}"), &src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'prior'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "Current/Prior case `{name}` resolved the wrong producer generation:\n{diagnostics}"
            ));
        }
    }

    for (name, read_prior, expected_invalidated) in [
        ("latest-content", "", false),
        ("prior-content", "    _ := prior[0].len()\n", true),
    ] {
        let src = format!(
            "fn main() -> i32 {{\n  mut old_owner := \"old\".clone()\n  old_view: str := old_owner\n  new_owner := \"new\".clone()\n  new_view: str := new_owner\n  arena {{\n    mut values := [\"seed\"].to_array()\n    mut prior: slice<str> := values\n    mut latest: slice<str> := values\n    mut first := true\n    loop {{\n      values = [\"seed\"].to_array()\n      if first {{\n        prior = values\n        prior[0] = old_view\n        first = false\n      }} else {{\n        latest = values\n        latest[0] = new_view\n        break\n      }}\n    }}\n    old_owner = \"replacement\".clone()\n    _ := latest[0].len()\n{read_prior}  }}\n  return 0\n}}\n"
        );
        let diagnostics = check_diagnostics(&format!("storage-generation-recency-{name}"), &src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'prior'")
            && diagnostics.contains("source 'old_owner'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "Current/Prior content case `{name}` must keep new Current content distinct from the arena-live Prior generation:\n{diagnostics}"
            ));
        }
    }

    for (name, src) in [
        (
            "fixed-copy-keeps-source",
            "fn main() -> i32 {\n  mut source := [1, 2]\n  alias: slice<i64> := source\n  copied := source\n  _ := copied[0]\n  _ := alias.len()\n  return 0\n}\n",
        ),
        (
            "assigned-view-field-move",
            "Holder { values: slice<i64> }\nfn main() -> i32 {\n  first := [1, 2].to_array()\n  second := [3, 4].to_array()\n  second_view: slice<i64> := second\n  mut holder := Holder { values: first }\n  holder.values = second_view\n  alias := holder.values\n  moved := second\n  _ := moved.len()\n  _ := alias.len()\n  return 0\n}\n",
        ),
        (
            "tuple-binding-move",
            "fn keep(value: i64) -> bool = value > 0\nfn main() -> i32 {\n  (selected, _) := [1, 2].partition(keep)\n  alias: slice<i64> := selected\n  moved := selected\n  _ := moved.len()\n  _ := alias.len()\n  return 0\n}\n",
        ),
        (
            "match-binding-move",
            "Choice { Values(array<i64>), Empty }\nfn main() -> i32 {\n  choice := Choice.Values([1, 2].to_array())\n  return match choice {\n    Values(values) => {\n      alias: slice<i64> := values\n      moved := values\n      _ := moved.len()\n      _ := alias.len()\n      0\n    }\n    Empty => 0\n  }\n}\n",
        ),
    ] {
        let diagnostics = check_diagnostics(
            &format!("storage-generation-projected-transfer-{name}"),
            src,
        );
        if !diagnostics.is_empty() {
            failures.push(format!(
                "projected transfer case `{name}` must preserve the pre-Move alias:\n{diagnostics}"
            ));
        }
    }

    for (name, src) in [
        (
            "dynamic-index",
            "fn main() -> i32 {\n  old_owner := \"old\".clone()\n  old_view: str := old_owner\n  mut values := [old_view].to_array()\n  snapshot := values[0]\n  mut new_owner := \"new\".clone()\n  new_view: str := new_owner\n  values[0] = new_view\n  new_owner = \"replacement\".clone()\n  _ := snapshot.len()\n  return 0\n}\n",
        ),
        (
            "soa-elem-field",
            "import core.json\nRow { value: str }\nfn main() -> Result<(), Error> {\n  old_owner := \"old\".clone()\n  old_view: str := old_owner\n  mut new_owner := \"new\".clone()\n  new_view: str := new_owner\n  arena {\n    mut rows: soa<Row> := json.decode(\"[{\\\"value\\\":\\\"seed\\\"}]\")?\n    rows[0].value = old_view\n    snapshot := rows[0].value\n    rows[0].value = new_view\n    new_owner = \"replacement\".clone()\n    _ := snapshot.len()\n  }\n  return Ok(())\n}\n",
        ),
    ] {
        let diagnostics =
            check_diagnostics(&format!("storage-generation-scalar-snapshot-{name}"), src);
        if !diagnostics.is_empty() {
            failures.push(format!(
                "scalar snapshot `{name}` must not become a live header observer:\n{diagnostics}"
            ));
        }
    }

    for (name, selection) in [
        ("if", "if condition { live } else { return 1; dead }"),
        (
            "match",
            "match Some(condition) { Some(_) => live, None => { return 1; dead } }",
        ),
        ("else-unwrap", "Some(live) else { return 1; dead }"),
    ] {
        let src = format!(
            "fn probe(condition: bool) -> i32 {{\n  live_owner := [1].to_array()\n  mut dead_owner := [2].to_array()\n  live: slice<i64> := live_owner\n  dead: slice<i64> := dead_owner\n  selected := {selection}\n  dead_owner = [3].to_array()\n  _ := selected.len()\n  return 0\n}}\nfn main() -> i32 = 0\n"
        );
        let diagnostics = check_diagnostics(
            &format!("storage-generation-fallthrough-header-{name}"),
            &src,
        );
        if !diagnostics.is_empty() {
            failures.push(format!(
                "control result `{name}` must exclude the non-fallthrough tail header:\n{diagnostics}"
            ));
        }
    }

    for (name, src, ending) in [
        (
            "consumed-after-move",
            "fn consume(values: array<i64>) -> i64 = values.len()\nfn main() -> i32 {\n  source := [1, 2].to_array()\n  alias: slice<i64> := source\n  moved := source\n  _ := consume(moved)\n  _ := alias.len()\n  return 0\n}\n",
            "was moved or reassigned",
        ),
        (
            "dropped-after-move",
            "fn main() -> i32 {\n  seed := [0]\n  mut alias: slice<i64> := seed\n  loop {\n    source := [1, 2].to_array()\n    alias = source\n    moved := source\n    _ := moved.len()\n    break\n  }\n  _ := alias.len()\n  return 0\n}\n",
            "was dropped at the end of the loop iteration",
        ),
    ] {
        let diagnostics = check_diagnostics(&format!("storage-generation-release-end-{name}"), src);
        if !(diagnostics.contains("use of invalidated borrow 'alias'")
            && diagnostics.contains("source 'moved'")
            && diagnostics.contains(ending))
        {
            failures.push(format!(
                "release ending `{name}` must follow the post-Move owner:\n{diagnostics}"
            ));
        }
    }

    for (name, after_loop, expected_invalidated) in [
        (
            "break-transfer-keeps-alias",
            "_ := result.len()\n  _ := alias.len()",
            false,
        ),
        (
            "result-consume-ends-alias",
            "_ := consume(result)\n  _ := alias.len()",
            true,
        ),
    ] {
        let src = format!(
            "fn consume(values: array<i64>) -> i64 = values.len()\nfn main() -> i32 {{\n  seed := [0]\n  mut alias: slice<i64> := seed\n  result := loop {{\n    source := [1, 2].to_array()\n    alias = source\n    break source\n  }}\n  {after_loop}\n  return 0\n}}\n"
        );
        let diagnostics =
            check_diagnostics(&format!("storage-generation-loop-result-{name}"), &src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'alias'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "loop-result release transfer `{name}` ended the wrong generation:\n{diagnostics}"
            ));
        }
    }

    for (name, body, expected_invalidated) in [
        (
            "old-backing-ended",
            "mut old_source := [\"old\"].to_array()\n  alias: slice<str> := old_source\n  holder := fn { alias[0] }\n  old_source = [\"replacement\"].to_array()\n  _ := holder().len()",
            true,
        ),
        (
            "new-backing-only-ended",
            "old_source := [\"old\"].to_array()\n  mut alias: slice<str> := old_source\n  holder := fn { alias[0] }\n  mut new_source := [\"new\"].to_array()\n  alias = new_source\n  new_source = [\"replacement\"].to_array()\n  _ := holder().len()",
            false,
        ),
    ] {
        let src = format!("fn main() -> i32 {{\n  {body}\n  return 0\n}}\n");
        let diagnostics =
            check_diagnostics(&format!("storage-generation-closure-capture-{name}"), &src);
        let invalidated = diagnostics.contains("use of invalidated borrow 'holder'");
        if invalidated != expected_invalidated || (!expected_invalidated && !diagnostics.is_empty())
        {
            failures.push(format!(
                "closure capture `{name}` must retain the backing selected when the closure was formed:\n{diagnostics}"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
