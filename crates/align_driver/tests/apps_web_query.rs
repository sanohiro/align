//! `pkg.web` query-string support — locating a parameter in an `a=1&b=2` string.
//!
//! The shipped module `apps/web/pkg/web/internal/query.align` is compiled via `include_str!` under a
//! minimal `pkg.web` root + a `main` entry. What is pinned here: lookup is **zero-allocation** (the
//! value comes back as a `str` view into the input and decoding stays the caller's explicit
//! `encoding.form_decode` step), an **escaped key** still matches its decoded name, and the
//! presence/empty distinction a bare `?flag` creates.

mod common;
use common::*;

const QUERY: &str = include_str!("../../../apps/web/pkg/web/internal/query.align");

const WEB_ROOT: &str = "module pkg.web\n\
import pkg.web.internal.query\n\
pub fn query_raw(q: str, name: str) -> str = pkg.web.internal.query.raw(q, name)\n\
pub fn query_has(q: str, name: str) -> bool = pkg.web.internal.query.has(q, name)\n";

fn run_query(name: &str, entry_main: &str) -> std::process::Output {
    build_and_run_multi(
        name,
        &[
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", entry_main),
        ],
        "main.align",
    )
}

#[test]
fn finds_values_and_distinguishes_absent_from_empty() {
    if !backend_available() {
        return;
    }
    // First / middle / last parameter; an absent name and an explicitly empty value both read as ""
    // (that is what `has` is for); a valueless `?flag` is present.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  q := \"a=1&name=John+Doe&empty=&flag&last=9\"\n\
  print(pkg.web.query_raw(q, \"a\"))              // 1\n\
  print(pkg.web.query_raw(q, \"name\"))           // John+Doe (raw — not yet decoded)\n\
  print(pkg.web.query_raw(q, \"last\"))           // 9\n\
  print(pkg.web.query_raw(q, \"missing\").len())  // 0\n\
  print(pkg.web.query_raw(q, \"empty\").len())    // 0\n\
  print(pkg.web.query_has(q, \"flag\"))           // true  — present, no value\n\
  print(pkg.web.query_has(q, \"missing\"))        // false\n\
  return Ok(())\n\
}\n";
    let out = run_query("webq-basic", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\nJohn+Doe\n9\n0\n0\ntrue\nfalse\n");
}

#[test]
fn matches_an_escaped_key_without_allocating() {
    if !backend_available() {
        return;
    }
    // `enc%20key=v` must be found by its DECODED name `enc key`; the comparison resolves escapes as
    // it walks, so no decoded key is ever materialized. `+` in a key means space too.
    let main = "module main\n\
import pkg.web\n\
fn main() -> Result<(), Error> {\n\
  print(pkg.web.query_raw(\"enc%20key=v&x=1\", \"enc key\"))   // v\n\
  print(pkg.web.query_raw(\"a+b=2\", \"a b\"))                 // 2\n\
  print(pkg.web.query_has(\"enc%20key=v\", \"enc key\"))       // true\n\
  print(pkg.web.query_has(\"a%ZZb=1\", \"a\"))                 // false — malformed escape never matches\n\
  return Ok(())\n\
}\n";
    let out = run_query("webq-escaped-key", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "v\n2\ntrue\nfalse\n");
}

#[test]
fn value_decoding_is_the_callers_explicit_step() {
    if !backend_available() {
        return;
    }
    // Lookup stays a view; turning `John+Doe` into `John Doe` allocates, so it is written out —
    // "nothing hidden" at the allocation boundary.
    let main = "module main\n\
import pkg.web\n\
import std.encoding\n\
fn main() -> Result<(), Error> {\n\
  q := \"name=John+Doe&city=S%C3%A3o+Paulo\"\n\
  d := encoding.form_decode(pkg.web.query_raw(q, \"name\"))?\n\
  print(d.bytes().as_str()?)\n\
  c := encoding.form_decode(pkg.web.query_raw(q, \"city\"))?\n\
  print(c.bytes().as_str()?)\n\
  return Ok(())\n\
}\n";
    let out = run_query("webq-decode", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "John Doe\nSão Paulo\n");
}
