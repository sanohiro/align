//! `http_stream` as a nameable type — streaming enabler ① (pkg.web W6, `docs/impl/pkg-design/web.md`
//! → "Streaming" enabler list).
//!
//! The stream-route handler signature is `fn(Ctx, http_stream) -> Result<(), Error>`: the framework
//! keeps the request handle for the whole request and the pump owns only the response stream. Writing
//! that signature needs `http_stream` to be spellable in source — it already exists as a full
//! `Scalar`/`Ty` (it is the `Ok` payload of `ctx.respond_stream(rb)` and carries `.send`/`.finish`),
//! but was not a source type name, so `unknown type: 'http_stream'` was the diagnostic. This is the
//! same `resolve_type` relaxation `response_builder` got in #583, and strictly smaller: every
//! payload/`is_move`/scalar-mapping path already knew `Scalar::HttpStream`.

mod common;
use common::*;

#[test]
fn http_stream_is_spellable_as_a_param_and_return() {
    // The name resolves in every position a Move handle may appear — a by-value parameter (the pump's
    // second argument) and a return — the same set `http_request_ctx` / `response_builder` already had.
    // `respond_stream` yields one, so the pump can be fed a real stream.
    let src = "import std.http\n\
fn pump(s: http_stream) -> Result<(), Error> {\n\
  s.send(\"data: x\\n\\n\")?\n\
  s.finish()\n\
}\n\
fn open(ctx: http_request_ctx) -> Result<http_stream, Error> {\n\
  rb := http.response(200)\n\
  return ctx.respond_stream(rb)\n\
}\n\
fn main() -> i32 { return 0 }\n";
    assert!(
        !check_errs("hs-spell", src),
        "http_stream must name a type in a param and a return position"
    );
}

#[test]
fn http_stream_takes_no_type_arguments() {
    // A handle type name is nullary — `http_stream<i64>` is a clean diagnostic, not a panic.
    let src = "import std.http\n\
fn pump(s: http_stream<i64>) -> Result<(), Error> = s.finish()\n\
fn main() -> i32 { return 0 }\n";
    assert!(
        check_errs("hs-args", src),
        "http_stream must reject type arguments"
    );
}

#[test]
fn http_stream_is_still_not_an_array_element() {
    // The relaxation is spelling only — as an array element an element read COPIES the handle, and
    // both copies would close the same stream fd. Still refused, like every owned I/O handle.
    let src = "import std.http\n\
fn two(a: http_stream, b: http_stream) -> i64 {\n\
  xs := [a, b]\n\
  return xs.len()\n\
}\n\
fn main() -> i32 { return 0 }\n";
    assert!(
        check_errs("hs-array", src),
        "http_stream must still be refused as an array element"
    );
}
