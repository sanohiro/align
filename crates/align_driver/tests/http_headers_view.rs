//! std.http item 10 — `ctx.headers()`, the **detached header-table view**.
//!
//! `ctx.headers()` yields a `http_headers`: a **Copy, non-owning, region-tracked** value whose
//! representation IS the `http_request_ctx` pointer, so it costs a pointer copy and adds no runtime
//! code — `hs.get(name)` reuses the very `align_rt_http_ctx_header` call the removed
//! `ctx.header(name)` used. The whole point is that a Copy per-request context struct (pkg.web's
//! `Ctx`) can CARRY it: a header lookup is the one accessor that cannot ride a stored span, because
//! the header NAME is not known until the handler asks, so the value being borrowed is the whole
//! parsed table.
//!
//! The design turns on ONE region rule (http.md item 10 ④), and both halves are pinned here:
//!   - `ctx.headers()` is `Frame`-capped at the ctx (a view minted from a LOCAL handle cannot leave
//!     the frame that owns the handle — no return, no `break`, no surviving a serve iteration);
//!   - `hs.get(name)` **inherits** the receiver's region instead of re-capping, which is exactly what
//!     lets the pkg.web wrapper `fn header(c: Ctx, name: str) = c.headers.get(name)` compile:
//!     through a parameter the caller provably outlives the call.
//!
//! Adding a `Ty` is compiler-forced through only four passes; the rest fail **open**. The two that
//! form a fatal pair (`ty_may_borrow` + `borrow_sources_inner` — either one missing gives the same
//! silent use-after-free) are pinned by `lookup_after_respond_is_rejected_on_a_bare_local`, which is
//! deliberately written on a BARE LOCAL: any `str` field in an enclosing struct supplies a borrow
//! root of its own and masks the hole.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// One request over its own connection, retrying the connect until the server is up.
fn client_exchange(port: u16, req: &[u8]) -> String {
    let req = &one_shot(req)[..];
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.set_read_timeout(Some(Duration::from_secs(30))).expect("read timeout");
                sock.write_all(req).expect("write request");
                let mut resp = Vec::new();
                let _ = sock.read_to_end(&mut resp);
                return String::from_utf8_lossy(&resp).into_owned();
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    }
}

/// **The property that motivates the whole split.** A free function takes the Copy context struct
/// **by parameter** and looks a header up through the view it carries — the pkg.web `web.header`
/// shape. Through a parameter the caller provably outlives the call, so `hs.get(name)` is `Static`
/// there and the wrapper compiles; the equivalent wrapper over the old `ctx.header(name)` did NOT
/// (its result was `Frame`-capped), which is the entire reason `ctx.headers()` exists.
///
/// End to end: the wrapper's `Option<str>` is a real zero-copy view of the request buffer, the
/// lookup is case-insensitive (RFC 9110 §5.1), and an absent header is `None` (not `""`).
#[test]
fn a_wrapper_through_a_parameter_compiles_and_reads_the_table() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli

Req { path: str, headers: http_headers }

fn header(r: Req, name: str) -> Option<str> = r.headers.get(name)

fn header_or(r: Req, name: str, dflt: str) -> str {
  match header(r, name) {
    Some(v) => v
    None => dflt
  }
}

pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  r := Req { path: ctx.path(), headers: ctx.headers() }
  rb := http.response(200)
  // Exact-case hit, differently-cased hit (the lookup is case-insensitive), and a miss.
  rb.header(\"X-Exact\", header_or(r, \"X-Trace\", \"MISSING\"))
  rb.header(\"X-Folded\", header_or(r, \"x-TRACE\", \"MISSING\"))
  rb.header(\"X-Absent\", header_or(r, \"X-Nope\", \"MISSING\"))
  rb.header(\"X-Path\", r.path)
  ctx.respond(rb)?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("http-headers-wrapper", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let text = client_exchange(port, b"GET /hi HTTP/1.1\r\nHost: h\r\nX-Trace: abc123\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status line: {text:?}");
    assert!(text.contains("X-Exact: abc123\r\n"), "exact-case hit through the wrapper: {text:?}");
    assert!(text.contains("X-Folded: abc123\r\n"), "case-insensitive hit through the wrapper: {text:?}");
    assert!(text.contains("X-Absent: MISSING\r\n"), "an absent header is None, not empty: {text:?}");
    assert!(text.contains("X-Path: /hi\r\n"), "the struct's other views still work: {text:?}");
}

/// A view minted from a **local** handle is `Frame`-capped, so it cannot outlive the frame that owns
/// the handle — by `return`, by `break`, or wrapped in a struct. Without the cap, every one of these
/// hands back a pointer to a freed request buffer.
#[test]
fn a_view_from_a_local_handle_cannot_escape_its_frame() {
    // `return ctx.headers()` — the handle is dropped at frame exit.
    let ret = "\
import std.http
fn mint(fallback: http_headers) -> http_headers {
  srv := http.serve(\"127.0.0.1\", 8080) else { return fallback }
  ctx := srv.accept() else { return fallback }
  return ctx.headers()
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(check_errs("http-headers-escape-return", ret), "returning a view of a local handle must be rejected");

    // The same escape wrapped in a struct — `region_of(StructLit)` folds in each field's region, so
    // the struct inherits the `Frame` cap and the return is still rejected.
    let via_struct = "\
import std.http
Held { headers: http_headers }
fn mint(fallback: http_headers) -> Held {
  srv := http.serve(\"127.0.0.1\", 8080) else { return Held { headers: fallback } }
  ctx := srv.accept() else { return Held { headers: fallback } }
  return Held { headers: ctx.headers() }
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(
        check_errs("http-headers-escape-struct", via_struct),
        "returning a struct carrying a view of a local handle must be rejected"
    );

    // `break ctx.headers()` — the handle is dropped at the end of the iteration.
    let brk = "\
import std.http
fn mint(fallback: http_headers) -> Result<http_headers, Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  hs := loop {
    ctx := srv.accept() else { break fallback }
    break ctx.headers()
  }
  return Ok(hs)
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(check_errs("http-headers-escape-break", brk), "breaking a view of a local handle out of a loop must be rejected");
}

/// A view cannot survive its ctx across a serve iteration **when the ctx is CONSUMED in that
/// iteration** — the pkg.web shape, where `ctx.respond(rb)` ends every pass. Assigning the view to a
/// local declared outside the loop and reading it on the NEXT pass is a use of an invalidated
/// borrow. This is the loop-fixpoint half of `MoveCheck`'s borrow flow: the read comes *before* the
/// assignment in source order, so only the back-edge join catches it.
///
/// **What this does NOT cover:** a ctx that is merely DROPPED at the end of the iteration rather
/// than moved. See `known_hole_scope_end_drop_does_not_invalidate_a_view` below — that is a
/// pre-existing gap in `MoveCheck`, not specific to this type.
#[test]
fn a_consumed_ctx_invalidates_a_view_held_across_a_serve_iteration() {
    let src = "\
import std.http
fn run(fallback: http_headers) -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  mut keep := fallback
  loop {
    h := keep.get(\"host\") else { \"\" }
    print(h.len())
    ctx := srv.accept()?
    keep = ctx.headers()
    rb := http.response(200)
    ctx.respond(rb)?
  }
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(
        check_errs("http-headers-cross-iteration", src),
        "a view held across a serve iteration must be rejected (its ctx is gone)"
    );
}

/// **The scope-end-drop half of borrow liveness — was a KNOWN HOLE, FIXED.**
///
/// `MoveCheck` used to end a borrow generation only when its owner was **moved or reassigned**
/// (`invalidate_storage` / `invalidate_owner`), and `Region::Frame` cannot distinguish "this frame"
/// from "this loop iteration". Neither noticed a Move handle bound INSIDE a loop body being
/// **dropped at the end of the iteration**, so a view assigned out to a longer-lived local survived
/// into the next pass and read freed memory. `loop_moves` now applies the loop's per-iteration drop
/// set (`iteration_drops`, the same `needs_drop_flag` boundary predicate MIR's `loop_iter_drops`
/// uses) to the back-edge state and to every `break` snapshot.
///
/// The gap was never a `http_headers` one: the second case is a plain `str` from `ctx.path()` and it
/// compiled identically (the `str` version read a freed buffer and printed stale lengths). The
/// `http_headers` version could be louder, because the dangling value IS the freed
/// `http_request_ctx` pointer that `align_rt_http_ctx_header` dereferences to walk the offset
/// table — but that was never a guarantee, only shape- and allocator-dependent UB.
#[test]
fn a_view_of_a_handle_dropped_at_the_end_of_an_iteration_is_rejected() {
    // The header-table view. `ctx` is dropped at the end of each iteration, never moved.
    let headers = "\
import std.http
fn run(fallback: http_headers) -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  mut keep := fallback
  loop {
    h := keep.get(\"host\") else { \"\" }
    print(h.len())
    ctx := srv.accept()?
    keep = ctx.headers()
  }
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    let diags = check_diagnostics("http-headers-scope-end-drop", headers);
    assert!(
        diags.contains("use of invalidated borrow 'keep'")
            && diags.contains("was dropped at the end of the loop iteration"),
        "a header view outliving the ctx dropped at the end of its iteration must be rejected, \
         with the drop-specific wording: {diags}"
    );

    // The same shape with a plain `str` view, which predates item 10 entirely — the proof that the
    // gap is `MoveCheck`'s, not this type's.
    let str_view = "\
import std.http
fn run(fallback: str) -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  mut keep := fallback
  loop {
    print(keep.len())
    ctx := srv.accept()?
    keep = ctx.path()
  }
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(
        check_errs("http-str-view-scope-end-drop", str_view),
        "a `str` view outliving its dropped ctx must be rejected too — the gap was MoveCheck's, \
         not this type's"
    );
}

/// **The fail-open pair.** `ctx.respond(rb)` consumes the handle and frees the request buffer, so a
/// view minted before it is dead afterwards. Catching that needs BOTH `ty_may_borrow(HttpHeaders)`
/// (so the `Let` records borrow provenance at all) and a `borrow_sources_inner` arm mapping
/// `HttpCtxHeaders` to the ctx's storage roots — `borrow_sources_inner`'s tail is
/// `_ => BorrowRoots::new()`, so a new node is NOT compiler-forced there. Either one missing gives
/// the same silent use-after-free.
///
/// Written on a **bare local** on purpose: put the view in a struct alongside any `str` field and
/// that field supplies a borrow root of its own, so the enclosing struct is invalidated anyway and
/// the test passes even with the hole wide open.
#[test]
fn a_lookup_after_respond_is_rejected_on_a_bare_local() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  hs := ctx.headers()
  rb := http.response(200)
  ctx.respond(rb)?
  h := hs.get(\"host\") else { \"\" }
  print(h.len())
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-after-respond", src);
    assert!(
        diags.contains("use of invalidated borrow 'hs'"),
        "a lookup through a view whose ctx was consumed by `respond` must be rejected: {diags}"
    );

    // **The other half of the pair, and the half that was NOT covered.** Above, the invalidated
    // binding is the VIEW; here it is the `str` the lookup returned, which borrows the same buffer
    // one step further out. That provenance comes from `borrow_sources_inner`'s
    // `HttpCtxHeader { headers: buffer, .. }` arm — a `_ => BorrowRoots::new()` tail means deleting
    // that arm is silent, and the case above still passes without it (its root comes from the
    // `HttpCtxHeaders` arm instead). Adversarial review caught exactly that: one arm of the pair had
    // no test at all.
    let via_result = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  hs := ctx.headers()
  v := hs.get(\"host\") else { \"\" }
  rb := http.response(200)
  ctx.respond(rb)?
  print(v.len())
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-result-after-respond", via_result);
    assert!(
        diags.contains("use of invalidated borrow 'v'"),
        "the `str` a lookup returned is dead once the ctx is consumed, too: {diags}"
    );

    // And the same through the mandated chained spelling, where the view is a temporary and the
    // chain has to be walked to reach the ctx (`storage_roots`'s `_ => borrow_sources(e)` tail).
    let chained = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  v := ctx.headers().get(\"host\") else { \"\" }
  rb := http.response(200)
  ctx.respond(rb)?
  print(v.len())
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-chained-after-respond", chained);
    assert!(
        diags.contains("use of invalidated borrow 'v'"),
        "a temporary view's lookup result is tracked back to the ctx as well: {diags}"
    );
}

/// The mirror image: `ctx.respond_stream(rb)` only **borrows** the ctx (it lifts the fd into the
/// stream and leaves the ctx spent but alive — http.md item 8 ①), so a view minted before it stays
/// valid and a lookup inside the stream pump must COMPILE and WORK. A blanket "the ctx is spent, kill
/// its views" rule would break this, so it is asserted end-to-end rather than by inspection.
#[test]
fn a_lookup_inside_a_stream_pump_after_respond_stream_works() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  hs := ctx.headers()
  rb := http.response(200)
  rb.header(\"Content-Type\", \"text/plain\")
  s := ctx.respond_stream(rb)?
  // The ctx is SPENT but not freed: the view minted before the head was committed still reads the
  // request buffer.
  s.send(hs.get(\"X-Trace\") else { \"none\" })?
  s.send(hs.get(\"x-trace\") else { \"none\" })?
  s.finish()?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("http-headers-stream-pump", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let text = client_exchange(port, b"GET /s HTTP/1.1\r\nHost: h\r\nX-Trace: live\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status line: {text:?}");
    assert!(text.contains("live"), "the view still reads the request buffer after respond_stream: {text:?}");
    // Both chunks carry the same value — the second is the case-folded lookup.
    assert_eq!(text.matches("live").count(), 2, "both chunks were streamed: {text:?}");
}

/// The view has deliberately **no `Scalar` variant**, so it is kept out of `Option`/`Result` payloads
/// and array/slice/box elements by fail-closed default — it lives in a local or a struct field, and
/// nowhere else. Pinned with the tailored diagnostic, so a future `Scalar::HttpHeaders` added without
/// the aggregate ABI work fails here rather than silently.
#[test]
fn the_view_is_not_a_payload_or_an_element() {
    let opt = "\
import std.http
fn wrap(hs: http_headers) -> Option<http_headers> = Some(hs)
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    let diags = check_diagnostics("http-headers-option-payload", opt);
    assert!(diags.contains("cannot be `http_headers`"), "an Option payload must be rejected: {diags}");

    let arr = "\
import std.http
fn collect(hs: http_headers) -> i64 {
  xs := [hs, hs]
  return xs.len()
}
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    let diags = check_diagnostics("http-headers-array-element", arr);
    assert!(diags.contains("cannot be `http_headers`"), "an array element must be rejected: {diags}");

    let res = "\
import std.http
fn wrap(hs: http_headers) -> Result<http_headers, Error> = Ok(hs)
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    let diags = check_diagnostics("http-headers-result-payload", res);
    assert!(diags.contains("cannot be `http_headers`"), "a Result payload must be rejected: {diags}");
}

/// A struct carrying the view stays **Copy** — the load-bearing property, since a Move `Ctx` would be
/// consumed by its own accessors (the shape pkg.web rejected). Asserted two ways: the struct is used
/// again after being passed by value (a Move struct would be a use-after-move error), and the
/// function that builds and reads it emits no free/drop call at all.
#[test]
fn a_struct_carrying_the_view_stays_copy() {
    let src = "\
import std.http
Req { path: str, headers: http_headers }
fn peek(r: Req, name: str) -> i64 {
  match r.headers.get(name) {
    Some(v) => v.len()
    None => 0 - 1
  }
}
pub fn probe(hs: http_headers, path: str) -> i64 {
  r := Req { path: path, headers: hs }
  // `r` passed by value TWICE and read again afterwards: only legal because it is Copy.
  a := peek(r, \"host\")
  b := peek(r, \"x-trace\")
  return a + b + r.path.len()
}

pub fn build(ctx: http_request_ctx) -> i64 = probe(ctx.headers(), ctx.path())
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(!check_errs("http-headers-copy-struct", src), "a struct carrying the view must stay Copy");

    let ir = emit_llvm_with_exports(src, &["probe", "build"]);
    // `probe` neither owns nor borrows any Move handle — it only builds and reads the Copy struct —
    // so if `Req` were Move (or the view were classified as owning anything) a drop would appear here.
    let body = ir.split("define").find(|f| f.contains(" @probe(")).unwrap_or_else(|| panic!("no `probe` in the IR:\n{ir}"));
    for freed in ["_free", "align_rt_drop"] {
        assert!(!body.contains(freed), "a Copy context struct must emit no drop ({freed}):\n{body}");
    }
    // The view is a bare pointer: minting it is a plain load/store of the ctx pointer, with no call.
    let build = ir.split("define").find(|f| f.contains(" @build(")).unwrap_or_else(|| panic!("no `build` in the IR:\n{ir}"));
    assert!(!build.contains("align_rt_http_ctx_headers"), "`ctx.headers()` is a pointer copy, not a call:\n{build}");
    // The view really is a bare pointer: `ctx.headers()` is a pointer copy, and the lookup is the
    // SAME runtime call the removed `ctx.header(name)` made — no new runtime entry point exists.
    assert!(ir.contains("align_rt_http_ctx_header("), "the lookup reuses the existing runtime call:\n{ir}");
    for added in ["align_rt_http_headers", "align_rt_http_ctx_headers"] {
        assert!(!ir.contains(added), "item 10 adds no runtime code at all, but found {added}:\n{ir}");
    }
}

/// `ctx.headers()` keeps the receiver **place-gate** (a temporary owned handle is nothing anyone
/// drops), while `hs.get(name)` deliberately does NOT inherit it — the mandated spelling
/// `ctx.headers().get(name)` has a non-place receiver, and the view owns nothing to drop.
#[test]
fn the_place_gate_applies_to_headers_but_not_to_get() {
    let temp_ctx = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  hs := srv.accept()?.headers()
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-temp-receiver", temp_ctx);
    assert!(
        diags.contains("bind the http request context to a local first"),
        "`ctx.headers()` on a temporary handle must be rejected: {diags}"
    );

    let chained = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  h := ctx.headers().get(\"host\") else { \"\" }
  print(h.len())
  rb := http.response(200)
  ctx.respond(rb)?
  return Ok(())
}
";
    assert!(!check_errs("http-headers-chained-get", chained), "`ctx.headers().get(name)` is the mandated spelling and must compile");
}

/// `ctx.header(name)` is **replaced**, not supplemented, so the lookup has one spelling. There were
/// zero Align call sites, so the removal is outright (no compat alias — pre-release).
#[test]
fn ctx_header_is_gone() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  h := ctx.header(\"host\") else { \"\" }
  print(h.len())
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-old-spelling", src);
    assert!(
        diags.contains("`ctx.header(name)` was replaced by `ctx.headers().get(name)`"),
        "the removed spelling must say where the lookup went, not just 'unknown method': {diags}"
    );
    // ...and the suggestion list a bad ctx method gets now names `headers`, not `header`.
    let arity = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  hs := ctx.headers(\"host\")
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-arity", arity);
    assert!(diags.contains("'.headers()' takes no arguments"), "arity is checked on `ctx.headers()`: {diags}");

    let get_arity = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  hs := ctx.headers()
  h := hs.get() else { \"\" }
  return Ok(())
}
";
    let diags = check_diagnostics("http-headers-get-arity", get_arity);
    assert!(
        diags.contains("'.get()' takes 1 argument (the header name)"),
        "`hs.get()` must reach the header arm, not the box-get catch-all: {diags}"
    );
}
