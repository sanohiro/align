//! `pkg.web.cors` — WHATWG Fetch CORS decisions as pure functions over a policy + the request
//! `Origin`. Emitting the headers is serve's job; deciding what may be emitted is testable without
//! a socket, which is what this pins — including the two rules that are security-relevant rather
//! than cosmetic: exact origin matching (no prefix/suffix bypass) and the forbidden
//! wildcard-with-credentials pair.

mod common;
use common::*;

const CORS: &str = include_str!("../../../apps/web/pkg/web/cors.align");

fn run_cors(name: &str, entry_main: &str) -> std::process::Output {
    build_and_run_multi(name, &[("pkg/web/cors.align", CORS), ("main.align", entry_main)], "main.align")
}

#[test]
fn allowlist_reflects_exactly_and_rejects_lookalikes() {
    if !backend_available() {
        return;
    }
    // A listed origin is reflected; an unlisted one yields "" (send no header at all). Matching is
    // EXACT — `https://app.example.com.evil.com` must not be granted by resembling a listed origin,
    // which is the classic CORS bypass.
    let main = "module main\n\
import pkg.web.cors\n\
fn main() -> Result<(), Error> {\n\
  origins := [\"https://app.example.com\", \"https://admin.example.com\"]\n\
  p := pkg.web.cors.CorsPolicy { allow_origins: origins, allow_credentials: true,\n\
        allow_methods: \"GET, POST, OPTIONS\", allow_headers: \"Content-Type\", max_age: 600 }\n\
  print(pkg.web.cors.valid(p))\n\
  print(pkg.web.cors.allow_origin(p, \"https://app.example.com\"))\n\
  print(pkg.web.cors.allow_origin(p, \"https://admin.example.com\"))\n\
  print(pkg.web.cors.allow_origin(p, \"https://evil.example.com\").len())\n\
  print(pkg.web.cors.allow_origin(p, \"https://app.example.com.evil.com\").len())\n\
  print(pkg.web.cors.vary_origin(p))\n\
  return Ok(())\n\
}\n";
    let out = run_cors("cors-allowlist", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\nhttps://app.example.com\nhttps://admin.example.com\n0\n0\ntrue\n"
    );
}

#[test]
fn wildcard_with_credentials_is_rejected() {
    if !backend_available() {
        return;
    }
    // `*` alone is fine and needs no `Vary: Origin` (the value does not depend on the request).
    // `*` WITH credentials is forbidden by the Fetch spec — it would expose every credentialed
    // response to any origin — so the policy is invalid and grants nothing, rather than being
    // silently downgraded to something that looks like it works.
    let main = "module main\n\
import pkg.web.cors\n\
fn main() -> Result<(), Error> {\n\
  star := [\"*\"]\n\
  w := pkg.web.cors.CorsPolicy { allow_origins: star, allow_credentials: false,\n\
        allow_methods: \"GET\", allow_headers: \"\", max_age: -1 }\n\
  print(pkg.web.cors.valid(w))\n\
  print(pkg.web.cors.allow_origin(w, \"https://anything.test\"))\n\
  print(pkg.web.cors.vary_origin(w))\n\
  bad := pkg.web.cors.CorsPolicy { allow_origins: star, allow_credentials: true,\n\
        allow_methods: \"GET\", allow_headers: \"\", max_age: -1 }\n\
  print(pkg.web.cors.valid(bad))\n\
  print(pkg.web.cors.allow_origin(bad, \"https://anything.test\").len())\n\
  return Ok(())\n\
}\n";
    let out = run_cors("cors-wildcard", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n*\nfalse\nfalse\n0\n");
}

#[test]
fn method_allowed_parses_the_wire_list() {
    if !backend_available() {
        return;
    }
    // `allow_methods` is the header value as sent (comma-separated, space-padded), so preflight
    // checking must parse it the same way — entries are compared trimmed and whole, not by substring.
    let main = "module main\n\
import pkg.web.cors\n\
fn main() -> Result<(), Error> {\n\
  origins := [\"https://a.test\"]\n\
  p := pkg.web.cors.CorsPolicy { allow_origins: origins, allow_credentials: false,\n\
        allow_methods: \"GET, POST, OPTIONS\", allow_headers: \"\", max_age: -1 }\n\
  print(pkg.web.cors.method_allowed(p, \"GET\"))\n\
  print(pkg.web.cors.method_allowed(p, \"POST\"))\n\
  print(pkg.web.cors.method_allowed(p, \"OPTIONS\"))\n\
  print(pkg.web.cors.method_allowed(p, \"DELETE\"))\n\
  print(pkg.web.cors.method_allowed(p, \"GE\"))\n\
  return Ok(())\n\
}\n";
    let out = run_cors("cors-methods", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\ntrue\ntrue\nfalse\nfalse\n");
}
