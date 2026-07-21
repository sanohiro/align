//! `response_builder` as a nameable type and a `Result` Ok payload.
//!
//! This is the compiler enabler behind pkg.web's ownership decision (2026-07-20): the request
//! handle belongs to the FRAMEWORK, and a handler BUILDS a response and hands it back
//! (`fn(Ctx) -> Result<response_builder, Error>`) instead of writing it. That inverts who can
//! answer a failed request — with the handler owning the handle, a handler that fails has already
//! consumed it and the framework has nothing left to respond through, so "handler Err -> 500" is
//! not implementable. Here it is, and `handler_err_still_gets_a_response` is that proof.
//!
//! Before this, `response_builder` was not spellable in source at all, and `scalar_arg` rejected it
//! as a payload outright — a deliberate "no API returns one in a `Result`" restriction, not a
//! soundness rule (`http_request_ctx`, an equally owned Move handle, has always ridden a `Result`
//! from `srv.accept()`). It is now a payload on exactly those terms: allowed in an `Option`/`Result`,
//! still refused as an array/slice/box element, where an element read copies the handle and both
//! copies would free it.

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

fn exchange(port: u16, req: &[u8]) -> String {
    // One request per connection: keep-alive would leave the socket parked and this read blocked.
    let req = &one_shot(req)[..];
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
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

/// A server in the framework-owns-the-handle shape: `Ctx` is a **Copy** struct of views, the
/// handler returns a built response through a fn-VALUE field, and only `main` — which still holds
/// the request handle — writes it. `/boom`'s handler fails, and the server answers 500 anyway.
const SERVER: &str = "module main\n\
import std.http\n\
import std.cli\n\
\n\
Ctx { method: str, path: str }\n\
Route { path: str, handler: fn(Ctx) -> Result<response_builder, Error> }\n\
\n\
fn hello(c: Ctx) -> Result<response_builder, Error> {\n\
  // The body is built from a LOCAL that dies when this function returns. The framework writes the\n\
  // response afterwards, so the byte-exact assertion on the client side is what proves the builder\n\
  // copied those bytes rather than keeping a view of them.\n\
  mut bb := builder()\n\
  bb.write(\"echo:\")\n\
  bb.write(c.path)\n\
  bv := bb.to_string()\n\
  rb := http.response(200)\n\
  rb.header(\"X-Route\", \"hello\")\n\
  rb.body(bv)\n\
  return Ok(rb)\n\
}\n\
\n\
fn boom(c: Ctx) -> Result<response_builder, Error> {\n\
  // Builds a response and then fails: the builder is dropped un-consumed (the Err path frees the\n\
  // payload it never returned), and the framework still owns the request handle.\n\
  rb := http.response(200)\n\
  rb.body(\"never sent\")\n\
  return Err(Error.Invalid)\n\
}\n\
\n\
pub fn main(args: array<str>) -> Result<(), Error> {\n\
  cmd := cli.command(\"srv\")\n\
  cmd.flag_i64(\"port\", 0)\n\
  p := cmd.parse(args)?\n\
  routes := [\n\
    Route { path: \"/hello\", handler: hello },\n\
    Route { path: \"/boom\", handler: boom },\n\
  ]\n\
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?\n\
  ctx := srv.accept()?\n\
  c := Ctx { method: ctx.method(), path: ctx.path() }\n\
  mut idx := 0\n\
  mut found := -1\n\
  loop {\n\
    if idx >= routes.len() { break }\n\
    ri := routes[idx]\n\
    if ri.path == c.path { found = idx }\n\
    idx = idx + 1\n\
  }\n\
  if found < 0 {\n\
    rb := http.response(404)\n\
    return ctx.respond(rb)\n\
  }\n\
  r := routes[found]\n\
  // The handler's Err becomes a 500 HERE — only possible because `ctx` never left this frame.\n\
  // `match`, not `else`: this INSPECTS the outcome to answer differently, rather than falling back.\n\
  // Both arms unify to a `response_builder`, so the handle is moved into `respond` exactly once.\n\
  rb := match r.handler(c) {\n\
    Ok(built) => built\n\
    Err(e) => {\n\
      fallback := http.response(500)\n\
      fallback.body(\"handler failed\")\n\
      fallback\n\
    }\n\
  }\n\
  return ctx.respond(rb)\n\
}\n";

fn run_server(name: &str, request: &[u8]) -> String {
    let port = free_loopback_port();
    // The basename MUST be per-test: `build_exe` derives the executable path from it (plus the
    // pid), and these tests run concurrently in one binary. Sharing one name made both build and
    // spawn the SAME file — one spawned while the other was still writing it (`ETXTBSY`), or after
    // the other's `TempArtifacts` drop had deleted it (`NotFound`). Both were observed on `main`.
    let built = build_exe(&format!("srv-rb-{name}"), SERVER);
    let mut child = std::process::Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let resp = exchange(port, request);
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    resp
}

#[test]
fn a_handler_returns_a_built_response_and_the_framework_writes_it() {
    if !backend_available() {
        return;
    }
    // The Ok payload survives the fn-value call, the match join, and the move into `respond`,
    // and is freed exactly once (a double free would abort before the clean exit asserted above).
    let text = run_server("ok", b"GET /hello HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status: {text:?}");
    assert!(text.contains("X-Route: hello\r\n"), "header: {text:?}");
    assert!(text.ends_with("\r\n\r\necho:/hello"), "body: {text:?}");
}

#[test]
fn handler_err_still_gets_a_response() {
    if !backend_available() {
        return;
    }
    // The point of the whole ownership decision: the handler failed AFTER building (and dropping)
    // a response, and the client still gets an answer, because the request handle never left the
    // framework's frame. Under the old handler-owns-the-handle shape this request would have hung.
    let text = run_server("err", b"GET /boom HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(text.starts_with("HTTP/1.1 500 "), "status: {text:?}");
    assert!(text.ends_with("\r\n\r\nhandler failed"), "body: {text:?}");
}

#[test]
fn a_dropped_ok_payload_is_freed_exactly_once() {
    if !backend_available() {
        return;
    }
    // `Result<response_builder, Error>` values that are never unwrapped, in a loop: the payload must
    // be freed by the Result's Drop. A double free aborts; a leak grows RSS without bound.
    //
    // This is not hypothetical — it is the bug this test was written against. `Scalar::is_move`
    // initially omitted `ResponseBuilder`, so the `Result` was classified Copy and its payload was
    // never dropped: the program still type-checked and ran, leaking ~81 bytes per iteration
    // (measured 17.5 MB at 200k iterations vs 162 MB at 2M — 10x the loop, 10x the RSS). Only a
    // differential measurement catches that class, so the loop count here is large on purpose.
    let src = "import std.http\n\
fn make(i: i64) -> Result<response_builder, Error> {\n\
  rb := http.response(200)\n\
  rb.body(\"payload\")\n\
  if i < 0 { return Err(Error.Invalid) }\n\
  return Ok(rb)\n\
}\n\
fn main() -> i32 {\n\
  mut i := 0\n\
  loop {\n\
    if i >= 200000 { break }\n\
    r := make(i)\n\
    i = i + 1\n\
  }\n\
  return 0\n\
}\n";
    let out = build_and_run("rb-drop", src);
    assert_eq!(
        out.status.code(),
        Some(0),
        "200k dropped Ok(response_builder) payloads must neither abort nor fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_response_builder_is_still_not_an_array_element() {
    // The payload relaxation is exactly that — a payload. As an array/slice/box element an element
    // read COPIES the handle, and both copies would free the header list and body buffer.
    let src = "import std.http\n\
fn main() -> i32 {\n\
  a := http.response(200)\n\
  b := http.response(201)\n\
  xs := [a, b]\n\
  return xs.len() as i32\n\
}\n";
    assert!(
        check_errs("rb-array", src),
        "a response_builder must still be refused as an array element"
    );
}

#[test]
fn a_response_builder_is_spellable_as_a_type() {
    // The name resolves in every position a handle type may appear — a parameter, a return, and a
    // `Result` payload — the same set `http_request_ctx` already had.
    let src = "import std.http\n\
fn build() -> Result<response_builder, Error> {\n\
  rb := http.response(200)\n\
  return Ok(rb)\n\
}\n\
fn send(ctx: http_request_ctx, rb: response_builder) -> Result<(), Error> = ctx.respond(rb)\n\
fn main() -> i32 { return 0 }\n";
    assert!(!check_errs("rb-spell", src), "response_builder must name a type");
}

#[test]
fn a_returned_builder_outlives_the_locals_its_body_came_from() {
    if !backend_available() {
        return;
    }
    // The safety property the whole payload relaxation rests on: `rb.header`/`rb.body` COPY their
    // arguments (`String::from_utf8_lossy(..).into_owned()` / `.to_vec()` in the runtime), so a
    // builder owns its bytes and borrows nothing. That is why it is not region-tracked and why
    // returning one across a function boundary is sound.
    //
    // Here the body and header values are built from locals that DIE when `make` returns. This
    // test observes only that the program survives (no abort, no double free) — the byte-exact
    // half of the proof is `a_handler_returns_a_built_response_and_the_framework_writes_it`, whose
    // handler builds its body the same way and whose client asserts the exact bytes off the wire.
    // Together they mean a zero-copy `rb.body` — a plausible performance change — cannot land
    // silently.
    let src = "import std.http\n\
fn make() -> Result<response_builder, Error> {\n\
  mut hb := builder()\n\
  hb.write(\"head\")\n\
  hb.write(\"er-value\")\n\
  hv := hb.to_string()\n\
  mut bb := builder()\n\
  bb.write(\"body-\")\n\
  bb.write(\"from-a-dead-local\")\n\
  bv := bb.to_string()\n\
  rb := http.response(200)\n\
  rb.header(\"X-Test\", hv)\n\
  rb.body(bv)\n\
  return Ok(rb)\n\
}\n\
fn main() -> i32 {\n\
  rb := make() else { return 1 }\n\
  return 0\n\
}\n";
    let out = build_and_run("rb-escape", src);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a builder must outlive the locals its header/body were copied from; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
