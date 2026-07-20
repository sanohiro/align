//! `pkg.jwt` — JSON Web Tokens (RFC 7519) over compact JWS (RFC 7515), HS256.
//!
//! The package source `apps/jwt/pkg/jwt.align` is compiled as the real shipped code (via
//! `include_str!`) under a `main` entry that exercises it. Two things are pinned here: **interop**
//! (the token is byte-identical to the canonical jwt.io HS256 vector, so any other JWT library
//! accepts it) and the **security posture** (algorithm confusion and tampering are rejected, not
//! accepted).

mod common;
use common::*;

/// The real, shipped package source.
const JWT: &str = include_str!("../../../apps/jwt/pkg/jwt.align");

fn run_jwt(name: &str, entry_main: &str) -> std::process::Output {
    build_and_run_multi(name, &[("pkg/jwt.align", JWT), ("main.align", entry_main)], "main.align")
}

#[test]
fn hs256_matches_the_canonical_interop_vector() {
    if !backend_available() {
        return;
    }
    // The jwt.io reference token for payload {"sub":"1234567890","name":"John Doe","iat":1516239022}
    // signed with "your-256-bit-secret". Byte-equality proves header bytes, unpadded base64url, the
    // signing input, and HMAC-SHA256 all match every other implementation.
    let main = "module main\n\
import pkg.jwt\n\
fn main() -> Result<(), Error> {\n\
  payload := \"{\\\"sub\\\":\\\"1234567890\\\",\\\"name\\\":\\\"John Doe\\\",\\\"iat\\\":1516239022}\"\n\
  print(pkg.jwt.encode_hs256(payload, \"your-256-bit-secret\"))\n\
  return Ok(())\n\
}\n";
    let out = run_jwt("jwt-vector", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.\
SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c\n"
    );
}

#[test]
fn decode_round_trips_and_rejects_forgeries() {
    if !backend_available() {
        return;
    }
    // 1 = accepted, 0 = rejected. Only the authentic token may verify: a wrong secret, a payload
    // swapped under a captured signature, an `alg: none` re-header (the classic algorithm-confusion
    // attack), and a structurally malformed token must all be refused.
    let main = "module main\n\
import pkg.jwt\n\
import std.encoding\n\
fn verdict(r: Result<string, Error>) -> i64 = match r {\n\
  Ok(_) => 1,\n\
  Err(_) => 0,\n\
}\n\
fn main() -> Result<(), Error> {\n\
  payload := \"{\\\"sub\\\":\\\"1234567890\\\",\\\"name\\\":\\\"John Doe\\\",\\\"iat\\\":1516239022}\"\n\
  secret := \"your-256-bit-secret\"\n\
  tok := pkg.jwt.encode_hs256(payload, secret)\n\
  got := pkg.jwt.decode_hs256(tok, secret)?\n\
  g: str := got\n\
  print(g == payload)\n\
  print(verdict(pkg.jwt.decode_hs256(tok, \"wrong\")))\n\
  bad_payload := encoding.base64url_encode(\"{\\\"sub\\\":\\\"admin\\\"}\")\n\
  mut t := builder()\n\
  t.write(encoding.base64url_encode(\"{\\\"alg\\\":\\\"HS256\\\",\\\"typ\\\":\\\"JWT\\\"}\"))\n\
  t.write(\".\")\n\
  t.write(bad_payload)\n\
  t.write(\".\")\n\
  t.write(\"SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c\")\n\
  print(verdict(pkg.jwt.decode_hs256(t.to_string(), secret)))\n\
  mut a := builder()\n\
  a.write(encoding.base64url_encode(\"{\\\"alg\\\":\\\"none\\\",\\\"typ\\\":\\\"JWT\\\"}\"))\n\
  a.write(\".\")\n\
  a.write(encoding.base64url_encode(payload))\n\
  a.write(\".\")\n\
  print(verdict(pkg.jwt.decode_hs256(a.to_string(), secret)))\n\
  print(verdict(pkg.jwt.decode_hs256(\"aaa.bbb\", secret)))\n\
  return Ok(())\n\
}\n";
    let out = run_jwt("jwt-verify", main);
    assert_eq!(out.status.code(), Some(0));
    // authentic round-trip, then four refusals (wrong secret / tampered payload / alg:none / malformed)
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n0\n0\n0\n0\n");
}

#[test]
fn time_claims_honour_exp_and_nbf() {
    if !backend_available() {
        return;
    }
    // `exp` and `nbf` are both OPTIONAL (RFC 7519 §4.1): honoured when present, ignored when absent.
    let main = "module main\n\
import pkg.jwt\n\
fn main() -> Result<(), Error> {\n\
  print(pkg.jwt.time_claims_valid(\"{\\\"exp\\\":2000}\", 1000))   // true  — not yet expired\n\
  print(pkg.jwt.time_claims_valid(\"{\\\"exp\\\":500}\", 1000))    // false — expired\n\
  print(pkg.jwt.time_claims_valid(\"{\\\"nbf\\\":2000}\", 1000))   // false — not valid yet\n\
  print(pkg.jwt.time_claims_valid(\"{\\\"nbf\\\":500}\", 1000))    // true\n\
  print(pkg.jwt.time_claims_valid(\"{\\\"sub\\\":\\\"x\\\"}\", 1000)) // true  — neither claim present\n\
  print(pkg.jwt.time_claims_valid(\"{\\\"exp\\\":\\\"soon\\\"}\", 1000)) // false — present but not a NumericDate\n\
  print(pkg.jwt.time_claims_valid(\"not json\", 1000))          // false — unparseable payload\n\
  return Ok(())\n\
}\n";
    let out = run_jwt("jwt-claims", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\nfalse\nfalse\ntrue\ntrue\nfalse\nfalse\n");
}
