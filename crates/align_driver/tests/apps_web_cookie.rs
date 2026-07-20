//! `pkg.web.cookie` — RFC 6265 cookies.
//!
//! A **public** submodule (not `internal`): `CookieOpts` is part of the API an application
//! constructs, so consumers must be able to name it. Reading is zero-copy (a `str` view into the
//! header); writing spells every security attribute out explicitly.

mod common;
use common::*;

const COOKIE: &str = include_str!("../../../apps/web/pkg/web/cookie.align");

fn run_cookie(name: &str, entry_main: &str) -> std::process::Output {
    build_and_run_multi(name, &[("pkg/web/cookie.align", COOKIE), ("main.align", entry_main)], "main.align")
}

#[test]
fn get_reads_values_and_distinguishes_absent_from_empty() {
    if !backend_available() {
        return;
    }
    // First / middle / last cookie; the space after each ';' is skipped; an absent name and an
    // explicitly empty value both read as "".
    let main = "module main\n\
import pkg.web.cookie\n\
fn main() -> Result<(), Error> {\n\
  h := \"sid=abc123; theme=dark; empty=; last=9\"\n\
  print(pkg.web.cookie.get(h, \"sid\"))\n\
  print(pkg.web.cookie.get(h, \"theme\"))\n\
  print(pkg.web.cookie.get(h, \"last\"))\n\
  print(pkg.web.cookie.get(h, \"empty\").len())\n\
  print(pkg.web.cookie.get(h, \"none\").len())\n\
  return Ok(())\n\
}\n";
    let out = run_cookie("cookie-get", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "abc123\ndark\n9\n0\n0\n");
}

#[test]
fn build_emits_requested_attributes_only() {
    if !backend_available() {
        return;
    
    }
    // Every attribute is explicit — nothing is defaulted in, because a silently-missing HttpOnly or
    // Secure is a real vulnerability. An empty path / negative max_age / empty same_site emit nothing.
    let main = "module main\n\
import pkg.web.cookie\n\
fn main() -> Result<(), Error> {\n\
  o := pkg.web.cookie.CookieOpts { path: \"/\", max_age: 3600, http_only: true, secure: true, same_site: \"Lax\" }\n\
  print(pkg.web.cookie.build(\"sid\", \"abc123\", o)?)\n\
  bare := pkg.web.cookie.CookieOpts { path: \"\", max_age: -1, http_only: false, secure: false, same_site: \"\" }\n\
  print(pkg.web.cookie.build(\"t\", \"v\", bare)?)\n\
  expire := pkg.web.cookie.CookieOpts { path: \"/\", max_age: 0, http_only: true, secure: false, same_site: \"Strict\" }\n\
  print(pkg.web.cookie.build(\"sid\", \"\", expire)?)\n\
  return Ok(())\n\
}\n";
    let out = run_cookie("cookie-build", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "sid=abc123; Path=/; Max-Age=3600; HttpOnly; Secure; SameSite=Lax\n\
t=v\n\
sid=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict\n"
    );
}

#[test]
fn build_rejects_header_injection() {
    if !backend_available() {
        return;
    }
    // Building a Set-Cookie from untrusted input is normal, so the grammar-breaking bytes are
    // CHECKED, not trusted: a ';' would inject an attribute, a CR/LF would inject a whole header,
    // and an '='/space in the NAME would corrupt the name=value split. 1 = accepted, 0 = rejected.
    let main = "module main\n\
import pkg.web.cookie\n\
fn ok(r: Result<string, Error>) -> i64 = match r {\n\
  Ok(_) => 1,\n\
  Err(_) => 0,\n\
}\n\
fn main() -> Result<(), Error> {\n\
  o := pkg.web.cookie.CookieOpts { path: \"/\", max_age: -1, http_only: true, secure: true, same_site: \"Lax\" }\n\
  print(ok(pkg.web.cookie.build(\"sid\", \"good\", o)))                      // 1\n\
  print(ok(pkg.web.cookie.build(\"sid\", \"x; Path=/evil\", o)))            // 0 — attribute injection\n\
  print(ok(pkg.web.cookie.build(\"sid\", \"a\\r\\nSet-Cookie: evil=1\", o)))  // 0 — header injection\n\
  print(ok(pkg.web.cookie.build(\"bad=name\", \"v\", o)))                    // 0 — '=' in the name\n\
  bad_path := pkg.web.cookie.CookieOpts { path: \"/x; HttpOnly\", max_age: -1, http_only: false, secure: false, same_site: \"\" }\n\
  print(ok(pkg.web.cookie.build(\"sid\", \"v\", bad_path)))                  // 0 — injected via path\n\
  return Ok(())\n\
}\n";
    let out = run_cookie("cookie-inject", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n0\n0\n0\n0\n");
}
