//! M10 Slice 1 — std.encoding. Base64 (standard + URL-safe), hex, and UTF-8 validation — pure byte
//! transforms over `bytes`/`str`: encode -> owned `string`, decode -> `Result<buffer, Error>`
//! (invalid input -> `Error.Invalid`), `utf8_valid` -> `bool`. The completion condition: encode/
//! decode round-trip for all three encodings (including empty input and non-block-aligned lengths),
//! known RFC 4648 vectors, invalid input rejected as `Error.Invalid`, and `utf8_valid`
//! positive/negative cases. (`docs/impl/07-roadmap.md` M10 Slice 1; `draft.md` §18.2.)

mod common;
use common::*;

// --- known RFC 4648 vectors + the encode->decode->re-encode round trip -------------------------

/// Standard Base64 encode matches the RFC 4648 vectors across every block boundary (empty, 1/2/3
/// residue bytes -> `=`/`==`/none padding), and decoding an encoded string then re-hex-encoding the
/// decoded `buffer` recovers the input's bytes — a full round trip through `string` and `buffer`.
#[test]
fn base64_known_vectors_and_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.base64_encode(\"\"))
  print(encoding.base64_encode(\"f\"))
  print(encoding.base64_encode(\"fo\"))
  print(encoding.base64_encode(\"foo\"))
  print(encoding.base64_encode(\"foob\"))
  print(encoding.base64_encode(\"fooba\"))
  print(encoding.base64_encode(\"foobar\"))
  dec := encoding.base64_decode(\"Zm9vYmFy\")?
  print(encoding.hex_encode(dec.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m10-base64-vectors", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "\nZg==\nZm8=\nZm9v\nZm9vYg==\nZm9vYmE=\nZm9vYmFy\n666f6f626172\n"
    );
}

/// URL-safe Base64 uses the `-_` alphabet and emits **no** padding, distinct from the standard
/// `+/` + `=` form on the same bytes; a URL-safe encode->decode->hex-encode round trip recovers the
/// bytes. The two byte inputs (`0xfbf0`, `0xffffff`) are exactly the ones that exercise `62`/`63`.
#[test]
fn base64url_alphabet_and_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  a := encoding.hex_decode(\"fbf0\")?
  print(encoding.base64_encode(a.bytes()))
  print(encoding.base64url_encode(a.bytes()))
  b := encoding.hex_decode(\"ffffff\")?
  print(encoding.base64_encode(b.bytes()))
  print(encoding.base64url_encode(b.bytes()))
  dec := encoding.base64url_decode(\"-_A\")?
  print(encoding.hex_encode(dec.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m10-base64url", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "+/A=\n-_A\n////\n____\nfbf0\n"
    );
}

/// Hex encode is lower-case; decode accepts both cases and round-trips through a `buffer`.
#[test]
fn hex_known_vector_and_case_insensitive_decode() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.hex_encode(\"foobar\"))
  lower := encoding.hex_decode(\"666f6f626172\")?
  print(encoding.base64_encode(lower.bytes()))
  upper := encoding.hex_decode(\"666F6F626172\")?
  print(encoding.base64_encode(upper.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m10-hex", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // Both decodes yield "foobar", so both re-encode to the same standard Base64.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "666f6f626172\nZm9vYmFy\nZm9vYmFy\n");
}

// --- invalid input -> Error.Invalid -----------------------------------------------------------

/// The main boundary exits `tag + 1` on a propagated `Err`; `Error.Invalid` is tag 1 -> exit 2. Each
/// bad decode input (a symbol outside the alphabet, a bad length, a non-hex digit, an odd hex
/// length) must therefore surface as `Error.Invalid` and exit 2.
fn assert_invalid(name: &str, expr: &str) {
    if !backend_available() {
        return;
    }
    let prog = format!(
        "\
import std.encoding
pub fn main() -> Result<(), Error> {{
  d := {expr}?
  return Ok(())
}}
"
    );
    let out = build_and_run(name, &prog);
    assert_eq!(
        out.status.code(),
        Some(2),
        "{expr} should be Error.Invalid (tag 1 -> exit 2); stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn base64_decode_bad_char_is_invalid() {
    assert_invalid("m10-b64-badchar", "encoding.base64_decode(\"Zm9v!mFy\")");
}

#[test]
fn base64_decode_bad_length_is_invalid() {
    // 5 symbols -> a residue of 1, which no valid encoding produces.
    assert_invalid("m10-b64-badlen", "encoding.base64_decode(\"Zm9vY\")");
}

#[test]
fn base64url_decode_rejects_standard_alphabet() {
    // `+`/`/` are not in the URL-safe alphabet.
    assert_invalid("m10-b64url-stdchar", "encoding.base64url_decode(\"+/A=\")");
}

#[test]
fn hex_decode_odd_length_is_invalid() {
    assert_invalid("m10-hex-odd", "encoding.hex_decode(\"abc\")");
}

#[test]
fn hex_decode_non_hex_is_invalid() {
    assert_invalid("m10-hex-nonhex", "encoding.hex_decode(\"zz\")");
}

// --- utf8_valid -------------------------------------------------------------------------------

/// `utf8_valid(bytes)` distinguishes valid UTF-8 from invalid bytes. The inputs are built via
/// `hex_decode` (the only way to name raw `bytes`): `48656c6c6f` = "Hello" (valid), a lone `ff`
/// (never a valid UTF-8 lead byte), and a truncated 2-byte sequence `c3` (valid lead, missing
/// continuation).
#[test]
fn utf8_valid_positive_and_negative() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  ok := encoding.hex_decode(\"48656c6c6f\")?
  print(encoding.utf8_valid(ok.bytes()))
  bad := encoding.hex_decode(\"ff\")?
  print(encoding.utf8_valid(bad.bytes()))
  trunc := encoding.hex_decode(\"c3\")?
  print(encoding.utf8_valid(trunc.bytes()))
  empty := encoding.hex_decode(\"\")?
  print(encoding.utf8_valid(empty.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m10-utf8-valid", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\nfalse\nfalse\ntrue\n");
}

// --- capability header (import required) -------------------------------------------------------

/// Every `encoding.*` use requires `import std.encoding` (the capability-header rule), like the
/// other `std` namespaces.
#[test]
fn encoding_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  print(encoding.hex_encode(\"x\"))
  return Ok(())
}
";
    assert!(check_errs("m10-encoding-noimport", src), "encoding.* without `import std.encoding` must error");
}

/// `utf8_valid` takes `bytes` (`slice<u8>`), not a `str` — passing a string literal is a type error.
#[test]
fn utf8_valid_rejects_str() {
    let src = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.utf8_valid(\"hi\"))
  return Ok(())
}
";
    assert!(check_errs("m10-utf8-valid-str", src), "utf8_valid on a str must be a type error");
}

// ── RFC 3986 percent-encoding (the URI-component codec) ───────────────────────────────────────

/// `percent_encode` escapes every byte outside the unreserved set (`A-Za-z0-9-._~`) as upper-case
/// `%XX`; `percent_decode` reverses it and passes unescaped bytes through. Multi-byte UTF-8 is
/// escaped per byte, which is what a URI carries.
#[test]
fn percent_encode_decode_round_trip() {
    if !backend_available() {
        return;
    }
    let src = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.percent_encode(\"a b&c=d/e?f\"))
  print(encoding.percent_encode(\"safe-._~AZaz09\"))
  print(encoding.percent_encode(\"日本\"))
  d := encoding.percent_decode(\"a%20b%26c%3Dd\")?
  print(d.bytes().as_str()?)
  p := encoding.percent_decode(\"hello\")?
  print(p.bytes().as_str()?)
  orig := \"key=va lue/%?#&x\"
  rt := encoding.percent_decode(encoding.percent_encode(orig))?
  print(rt.bytes().as_str()? == orig)
  return Ok(())
}
";
    let out = build_and_run("m10-percent-rt", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a%20b%26c%3Dd%2Fe%3Ff\nsafe-._~AZaz09\n%E6%97%A5%E6%9C%AC\na b&c=d\nhello\ntrue\n"
    );
}

/// A `%` that is not followed by two hex digits makes the whole input invalid (`Error.Invalid`) —
/// a truncated escape, a single digit, and a non-hex digit are each rejected rather than guessed at.
#[test]
fn percent_decode_rejects_malformed_escapes() {
    if !backend_available() {
        return;
    }
    let src = "\
import std.encoding
fn bad(s: str) -> i64 = match encoding.percent_decode(s) {
  Ok(_) => 1,
  Err(_) => 0,
}
pub fn main() -> Result<(), Error> {
  print(bad(\"%\"))
  print(bad(\"%A\"))
  print(bad(\"a%ZZb\"))
  print(bad(\"ok%41\"))
  return Ok(())
}
";
    let out = build_and_run("m10-percent-bad", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n0\n0\n1\n");
}

// ── application/x-www-form-urlencoded ─────────────────────────────────────────────────────────

/// `form_encode`/`form_decode` are the HTML-form / query-string payload rule: space is `+`, every
/// other non-unreserved byte is `%XX`. Encode one key or value at a time — the `=`/`&` joining them
/// are structure, not data.
#[test]
fn form_encode_decode_round_trip() {
    if !backend_available() {
        return;
    }
    let src = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.form_encode(\"a b&c=d\"))
  print(encoding.form_encode(\"日本 語\"))
  d := encoding.form_decode(\"a+b%26c%3Dd\")?
  print(d.bytes().as_str()?)
  p := encoding.form_decode(\"hello+world\")?
  print(p.bytes().as_str()?)
  orig := \"name=John Doe & Co/50%\"
  rt := encoding.form_decode(encoding.form_encode(orig))?
  print(rt.bytes().as_str()? == orig)
  return Ok(())
}
";
    let out = build_and_run("m10-form-rt", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a+b%26c%3Dd\n%E6%97%A5%E6%9C%AC+%E8%AA%9E\na b&c=d\nhello world\ntrue\n"
    );
}

/// The two codecs differ in exactly one place: a space. `percent_encode` writes `%20` (RFC 3986),
/// `form_encode` writes `+`; each decoder accepts `%20`, only `form_decode` reads `+` as a space.
#[test]
fn form_and_percent_differ_only_on_space() {
    if !backend_available() {
        return;
    }
    let src = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.percent_encode(\"a b\"))
  print(encoding.form_encode(\"a b\"))
  p := encoding.percent_decode(\"a+b\")?
  print(p.bytes().as_str()?)
  f := encoding.form_decode(\"a+b\")?
  print(f.bytes().as_str()?)
  return Ok(())
}
";
    let out = build_and_run("m10-form-vs-percent", src);
    assert_eq!(out.status.code(), Some(0));
    // percent leaves '+' alone (it is not an escape there); form reads it as a space.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a%20b\na+b\na+b\na b\n");
}

// ── HTML escaping ─────────────────────────────────────────────────────────────────────────────

/// `html_escape` neutralizes all five of `& < > " '`, so one escaped string is safe in BOTH element
/// text and a quoted attribute — a caller never has to pick a context-specific variant (the mistake
/// that produces XSS). Other bytes, including multi-byte UTF-8, pass through unchanged.
#[test]
fn html_escape_neutralizes_markup() {
    if !backend_available() {
        return;
    }
    let src = "\
import std.encoding
pub fn main() -> Result<(), Error> {
  print(encoding.html_escape(\"<script>alert('xss')</script>\"))
  print(encoding.html_escape(\"a & b\"))
  print(encoding.html_escape(\"say \\\"hi\\\"\"))
  print(encoding.html_escape(\"plain 日本\"))
  return Ok(())
}
";
    let out = build_and_run("m10-html-escape", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "&lt;script&gt;alert(&#39;xss&#39;)&lt;/script&gt;\na &amp; b\nsay &quot;hi&quot;\nplain 日本\n"
    );
}
