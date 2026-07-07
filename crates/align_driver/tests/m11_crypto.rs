//! M11 std.crypto — Slice 1: `constant_time_equal` (self-hosted, branchless) + `crypto.random`
//! (OS CSPRNG). The slice opens the `std.crypto` module and validates the constant-time discipline
//! (crypto.md P1: constant-time is CORRECTNESS). No external library dependency.
//!
//! `crypto.constant_time_equal(a: bytes, b: bytes) -> bool` — a constant-time byte-equality test
//! over two byte views; the input length is **public** (differing lengths → `false`), the CT
//! guarantee is over equal-length content. **Pure** (a branchless self-hosted computation), so it is
//! allowed inside a `par_map` closure. `crypto.random(out: mut buffer)` fills the whole buffer from
//! the OS CSPRNG (abort on the rare failure); **Impure** (reads OS entropy), so `par_map` rejects it.
//!
//! These integration tests pin the **language-level contract**: import gate, arity, the byte-view
//! input forms, the truth table (first/middle/last-byte differences, length mismatch, empty cases),
//! purity (ct_equal accepted / random rejected by `par_map`), and that `random` actually fills a
//! caller-owned buffer with distinct, non-zero bytes. The branchless-reduction discipline and the
//! no-length-leak behavior are pinned exhaustively in the `align_runtime` unit tests
//! (`ct_equal_*` / `crypto_random_*`), which drive the wrappers directly.
//! (`docs/impl/std-design/crypto.md`.)

mod common;
use common::*;

// --- constant_time_equal: truth table ---------------------------------------------------------

/// Equal content is `true`; a difference at the first, middle, or last byte is each `false`. Prints
/// one bool per line.
#[test]
fn ct_equal_truth_table() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  print(crypto.constant_time_equal(\"abcdef\", \"abcdef\"))
  print(crypto.constant_time_equal(\"Xbcdef\", \"abcdef\"))
  print(crypto.constant_time_equal(\"abcXef\", \"abcdef\"))
  print(crypto.constant_time_equal(\"abcdeX\", \"abcdef\"))
  return Ok(())
}
";
    let out = build_and_run("m11cr-truth", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\nfalse\nfalse\nfalse\n");
}

/// Length is public (crypto.md P1): differing lengths return `false` immediately, even when one is a
/// prefix of the other. Empty vs empty is `true`; empty vs non-empty is `false`.
#[test]
fn ct_equal_length_and_empty_cases() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  print(crypto.constant_time_equal(\"abc\", \"abcd\"))
  print(crypto.constant_time_equal(\"abcd\", \"abc\"))
  print(crypto.constant_time_equal(\"\", \"\"))
  print(crypto.constant_time_equal(\"\", \"a\"))
  return Ok(())
}
";
    let out = build_and_run("m11cr-length", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "false\nfalse\ntrue\nfalse\n");
}

/// `constant_time_equal` accepts every byte-view form — a `str` literal, an owned `string` (here from
/// `hex_encode`, auto-borrowed), and `bytes` (a `slice<u8>` from `buffer.bytes()`) — mixed freely.
#[test]
fn ct_equal_byte_view_forms() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  s := encoding.hex_encode(\"AB\")            // owned string \"4142\"
  raw := encoding.hex_decode(\"4142\")?        // buffer of bytes {0x41, 0x42}
  print(crypto.constant_time_equal(s, \"4142\"))          // string vs str
  print(crypto.constant_time_equal(raw.bytes(), \"AB\"))   // slice<u8> vs str
  print(crypto.constant_time_equal(raw.bytes(), s))      // slice<u8> vs string (\"AB\" != \"4142\")
  return Ok(())
}
";
    let out = build_and_run("m11cr-views", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\ntrue\nfalse\n");
}

/// `constant_time_equal` is a Pure self-hosted computation (no I/O), so a closure that uses it IS
/// accepted by `par_map` (which requires a Pure closure) — the encoding-is-pure side of the line,
/// unlike the impure `crypto.random`. The program compiles and runs.
#[test]
fn ct_equal_pure_in_par_map() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
fn f(x: i64) -> i64 {
  if crypto.constant_time_equal(\"tag\", \"tag\") { return x * 2 }
  return 0
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    let out = build_and_run("m11cr-parmap-pure", prog);
    assert_eq!(out.status.code(), Some(0), "a pure ct_equal closure must be accepted by par_map; stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "4\n");
}

// --- crypto.random ----------------------------------------------------------------------------

/// `crypto.random(out)` fills the whole buffer: `out.len()` becomes the capacity, and two 32-byte
/// fills are (almost surely) different (compared as hex). Confirms the buffer is caller-owned and
/// usable after the fill (`.bytes()` / `.len()`).
#[test]
fn random_fills_and_two_fills_differ() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  b := buffer(32)
  crypto.random(b)
  print(b.len())
  h1 := encoding.hex_encode(b.bytes())
  crypto.random(b)
  h2 := encoding.hex_encode(b.bytes())
  print(crypto.constant_time_equal(h1, h2))   // two independent fills differ → false
  return Ok(())
}
";
    let out = build_and_run("m11cr-random-fill", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "32\nfalse\n");
}

/// A 4096-byte fill is not all-zeros: the CSPRNG loop drains getrandom's per-call cap over the whole
/// capacity. The hex of an all-zero 4096-byte buffer would be 8192 `0`s; a real fill is not.
#[test]
fn random_large_fill_not_all_zero() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  b := buffer(4096)
  crypto.random(b)
  print(b.len())
  print(encoding.hex_encode(b.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-random-large", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (len_line, hex_line) = stdout.split_once('\n').expect("two output lines");
    assert_eq!(len_line, "4096", "the whole capacity is filled");
    let hex = hex_line.trim_end();
    assert_eq!(hex.len(), 8192, "4096 bytes → 8192 hex chars");
    assert!(hex.bytes().any(|c| c != b'0'), "a CSPRNG fill is (almost surely) not all-zero");
}

/// `crypto.random` reads OS entropy — Impure. A closure that fills a buffer via `crypto.random` is
/// never `Pure`, so `par_map` (which requires a Pure closure) rejects it.
#[test]
fn random_impure_rejected_by_par_map() {
    let src = "\
import std.crypto
fn f(x: i64) -> i64 {
  b := buffer(8)
  crypto.random(b)
  return x + b.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11cr-parmap-impure", src), "an impure crypto.random closure must be rejected by par_map");
}

// --- capability header + arity ----------------------------------------------------------------

/// Every `crypto.*` use requires `import std.crypto` (the capability-header rule); the diagnostic
/// names the capability.
#[test]
fn crypto_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  print(crypto.constant_time_equal(\"a\", \"a\"))
  return Ok(())
}
";
    assert!(check_errs("m11cr-noimport", src), "crypto.* without `import std.crypto` must error");
    let diags = check_diagnostics(
        "m11cr-diag",
        "pub fn main() -> Result<(), Error> {\n  b := buffer(8)\n  crypto.random(b)\n  return Ok(())\n}\n",
    );
    assert!(diags.contains("import std.crypto"), "diagnostic should name the capability: {diags}");
}

/// `constant_time_equal` takes exactly 2 args; `random` exactly 1. A wrong arity is a compile error,
/// as is a non-buffer `random` argument.
#[test]
fn crypto_wrong_shape_rejected() {
    assert!(
        check_errs("m11cr-cteq-arity", "import std.crypto\npub fn main() -> Result<(), Error> {\n  print(crypto.constant_time_equal(\"a\"))\n  return Ok(())\n}\n"),
        "constant_time_equal with 1 argument must error"
    );
    assert!(
        check_errs("m11cr-rand-arity", "import std.crypto\npub fn main() -> Result<(), Error> {\n  crypto.random()\n  return Ok(())\n}\n"),
        "random with 0 arguments must error"
    );
    assert!(
        check_errs("m11cr-rand-type", "import std.crypto\npub fn main() -> Result<(), Error> {\n  crypto.random(\"not a buffer\")\n  return Ok(())\n}\n"),
        "random on a non-buffer must error"
    );
    // A non-byte-view argument to constant_time_equal (an i64) is a type error.
    assert!(
        check_errs("m11cr-cteq-type", "import std.crypto\npub fn main() -> Result<(), Error> {\n  print(crypto.constant_time_equal(1, 2))\n  return Ok(())\n}\n"),
        "constant_time_equal on non-byte-views must error"
    );
}

// --- sha256 / sha512 (Slice 2) ----------------------------------------------------------------
//
// `crypto.sha256(data: bytes) -> array<u8>` (32-byte digest) / `crypto.sha512(data) -> array<u8>`
// (64-byte digest) via OpenSSL libcrypto's EVP one-shot. The owned digest array slices to a
// `slice<u8>` (u8 is not a Move element), which `encoding.hex_encode` renders for comparison against
// the NIST/RFC known-answer vectors. Impure (a C-engine call), so `par_map` rejects a hashing closure.

/// `sha256` matches the NIST known-answer vectors for the empty string and `"abc"`, and the digest
/// is 32 bytes.
#[test]
fn sha256_known_vectors() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  e := crypto.sha256(\"\")
  print(e.len())
  print(encoding.hex_encode(e[0..e.len()]))
  a := crypto.sha256(\"abc\")
  print(encoding.hex_encode(a[0..a.len()]))
  return Ok(())
}
";
    let out = build_and_run("m11cr-sha256-vec", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "32\n\
         e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
    );
}

/// `sha512` matches the RFC/FIPS known-answer vectors for `"abc"` and the empty string, and the
/// digest is 64 bytes.
#[test]
fn sha512_known_vectors() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  a := crypto.sha512(\"abc\")
  print(a.len())
  print(encoding.hex_encode(a[0..a.len()]))
  e := crypto.sha512(\"\")
  print(encoding.hex_encode(e[0..e.len()]))
  return Ok(())
}
";
    let out = build_and_run("m11cr-sha512-vec", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "64\n\
         ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f\n\
         cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e\n"
    );
}

/// The digest accepts every byte-view form — a `str` literal, an owned `string` (from `hex_encode`,
/// auto-borrowed), and `bytes` (a `slice<u8>` from `buffer.bytes()`) — and hashing the same bytes
/// through different forms yields the same digest (compared in constant time). `sha256("41")`'s
/// input `"41"` is the two ASCII bytes 0x34 0x31, matching neither the raw byte 0x41 nor `hex_encode`.
#[test]
fn sha256_byte_view_forms_agree() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  raw := encoding.hex_decode(\"6162\")?     // buffer of bytes {0x61, 0x62} == \"ab\"
  from_str := crypto.sha256(\"ab\")          // str
  from_slice := crypto.sha256(raw.bytes())  // slice<u8>
  print(crypto.constant_time_equal(from_str[0..from_str.len()], from_slice[0..from_slice.len()]))
  // An owned string as input (auto-borrowed): sha256(\"4142\") over the four ASCII digit bytes.
  s := encoding.hex_encode(\"AB\")           // owned string \"4142\"
  from_string := crypto.sha256(s)
  from_literal := crypto.sha256(\"4142\")
  print(crypto.constant_time_equal(from_string[0..from_string.len()], from_literal[0..from_literal.len()]))
  return Ok(())
}
";
    let out = build_and_run("m11cr-sha256-views", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\ntrue\n");
}

/// A large (~1 MiB) input hashes without crashing and is **deterministic**: hashing the same bytes
/// twice yields identical digests (constant-time equal → `true`), while a one-byte-different input
/// yields a different digest (`false`). Uses `crypto.random` to fill the big buffer (no giant literal).
#[test]
fn sha256_large_input_deterministic() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  b := buffer(1048576)
  crypto.random(b)
  print(b.len())                          // the input is 1 MiB
  d1 := crypto.sha256(b.bytes())
  d2 := crypto.sha256(b.bytes())
  print(crypto.constant_time_equal(d1[0..d1.len()], d2[0..d2.len()]))  // same input → same digest
  c := buffer(1048576)
  crypto.random(c)
  e := crypto.sha256(c.bytes())
  print(crypto.constant_time_equal(d1[0..d1.len()], e[0..e.len()]))    // different input → differ
  return Ok(())
}
";
    let out = build_and_run("m11cr-sha256-large", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1048576\ntrue\nfalse\n");
}

/// `crypto.sha256`/`sha512` call the libcrypto engine — Impure. A closure that hashes is never
/// `Pure`, so `par_map` (which requires a Pure closure) rejects it (unlike the Pure `constant_time_equal`).
#[test]
fn sha_impure_rejected_by_par_map() {
    let src = "\
import std.crypto
fn f(x: i64) -> i64 {
  d := crypto.sha256(\"data\")
  return x + d.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11cr-sha-parmap-impure", src), "an impure sha256 closure must be rejected by par_map");
}

/// `sha256`/`sha512` require `import std.crypto` and take exactly one byte-view argument.
#[test]
fn sha_wrong_shape_rejected() {
    assert!(
        check_errs("m11cr-sha-noimport", "pub fn main() -> Result<(), Error> {\n  d := crypto.sha256(\"a\")\n  print(d.len())\n  return Ok(())\n}\n"),
        "sha256 without `import std.crypto` must error"
    );
    assert!(
        check_errs("m11cr-sha-arity", "import std.crypto\npub fn main() -> Result<(), Error> {\n  d := crypto.sha256(\"a\", \"b\")\n  print(d.len())\n  return Ok(())\n}\n"),
        "sha256 with 2 arguments must error"
    );
    assert!(
        check_errs("m11cr-sha-type", "import std.crypto\npub fn main() -> Result<(), Error> {\n  d := crypto.sha512(42)\n  print(d.len())\n  return Ok(())\n}\n"),
        "sha512 on a non-byte-view must error"
    );
}

// --- hmac_sha256 / hkdf_sha256 (Slice 3) ------------------------------------------------------
//
// `crypto.hmac_sha256(key: bytes, data: bytes) -> array<u8>` (32-byte tag) and
// `crypto.hkdf_sha256(salt, ikm, info, len) -> Result<buffer, Error>` (HKDF over libcrypto's
// `EVP_KDF`). Both Impure (a C-engine call), so `par_map` rejects them. Byte keys/inputs for the
// RFC vectors are built with `encoding.hex_decode(...).bytes()` (a `slice<u8>`); the outputs are
// `hex_encode`d and compared against the RFC 4231 (HMAC) / RFC 5869 (HKDF) known-answer vectors.

/// `hmac_sha256` matches RFC 4231 Test Case 1 (key = 0x0b x 20, data = "Hi There") and Test Case 2
/// (key = "Jefe", data = "what do ya want for nothing?"); the tag is 32 bytes.
#[test]
fn hmac_sha256_rfc4231_vectors() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  key1 := encoding.hex_decode(\"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b\")?   // 0x0b x 20
  t1 := crypto.hmac_sha256(key1.bytes(), \"Hi There\")
  print(t1.len())
  print(encoding.hex_encode(t1[0..t1.len()]))
  t2 := crypto.hmac_sha256(\"Jefe\", \"what do ya want for nothing?\")
  print(encoding.hex_encode(t2[0..t2.len()]))
  return Ok(())
}
";
    let out = build_and_run("m11cr-hmac-vec", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "32\n\
         b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7\n\
         5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843\n"
    );
}

/// Empty key and empty data are both valid HMAC inputs (no abort); the tag is the well-defined
/// HMAC-SHA256(key="", msg="") value.
#[test]
fn hmac_sha256_empty_key_and_data() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  t := crypto.hmac_sha256(\"\", \"\")
  print(t.len())
  print(encoding.hex_encode(t[0..t.len()]))
  return Ok(())
}
";
    let out = build_and_run("m11cr-hmac-empty", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "32\nb613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad\n"
    );
}

/// `hkdf_sha256` matches RFC 5869 Test Case 1 (ikm = 0x0b x 22, salt = 0x00..0c, info = 0xf0..f9,
/// L = 42) and Test Case 3 (empty salt + empty info, L = 42); the output is 42 bytes.
#[test]
fn hkdf_sha256_rfc5869_vectors() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  ikm := encoding.hex_decode(\"0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b\")?   // 0x0b x 22
  salt := encoding.hex_decode(\"000102030405060708090a0b0c\")?
  info := encoding.hex_decode(\"f0f1f2f3f4f5f6f7f8f9\")?
  ok1 := crypto.hkdf_sha256(salt.bytes(), ikm.bytes(), info.bytes(), 42)?
  print(ok1.len())
  print(encoding.hex_encode(ok1.bytes()))
  // TC3: empty salt + empty info (salt defaults to zeros, info absent).
  ok3 := crypto.hkdf_sha256(\"\", ikm.bytes(), \"\", 42)?
  print(encoding.hex_encode(ok3.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-hkdf-vec", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "42\n\
         3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865\n\
         8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8\n"
    );
}

/// The exact RFC 5869 `L` limit (8160 = 255*32) is valid: it derives 8160 bytes.
#[test]
fn hkdf_sha256_max_length_ok() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  ikm := \"input keying material\"
  out := crypto.hkdf_sha256(\"salt\", ikm, \"info\", 8160)?
  print(out.len())
  return Ok(())
}
";
    let out = build_and_run("m11cr-hkdf-max", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "8160\n");
}

/// A non-positive (`0`, negative) or over-limit (`8161 > 8160`) `len` is a public-value error →
/// `Error.Invalid` (tag 1 → exit 2 via the propagated `?` at the `main` boundary), rejected before
/// any engine call. Empty `salt`/`info` are still fine (covered by the vectors' TC3).
#[test]
fn hkdf_sha256_len_bounds_are_invalid() {
    if !backend_available() {
        return;
    }
    for (tag, len) in [("zero", "0"), ("neg", "-1"), ("over", "8161")] {
        let prog = format!(
            "\
import std.crypto
pub fn main() -> Result<(), Error> {{
  out := crypto.hkdf_sha256(\"salt\", \"ikm\", \"info\", {len})?
  print(out.len())
  return Ok(())
}}
"
        );
        let out = build_and_run(&format!("m11cr-hkdf-len-{tag}"), &prog);
        assert_eq!(
            out.status.code(),
            Some(2),
            "hkdf len={len} → Error.Invalid (exit 2); stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// `hmac_sha256` / `hkdf_sha256` call the libcrypto engine — Impure. A closure that uses either is
/// never `Pure`, so `par_map` (which requires a Pure closure) rejects it.
#[test]
fn hmac_hkdf_impure_rejected_by_par_map() {
    let hmac_src = "\
import std.crypto
fn f(x: i64) -> i64 {
  t := crypto.hmac_sha256(\"k\", \"data\")
  return x + t.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11cr-hmac-parmap", hmac_src), "an impure hmac_sha256 closure must be rejected by par_map");

    let hkdf_src = "\
import std.crypto
fn f(x: i64) -> i64 {
  r := crypto.hkdf_sha256(\"s\", \"ikm\", \"i\", 32)
  return x + match r { Ok(b) => b.len(), Err(_) => 0 }
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11cr-hkdf-parmap", hkdf_src), "an impure hkdf_sha256 closure must be rejected by par_map");
}

/// `hmac_sha256` (2 byte-view args) / `hkdf_sha256` (3 byte-views + an i64) require `import
/// std.crypto` and reject the wrong arity / argument types.
#[test]
fn hmac_hkdf_wrong_shape_rejected() {
    // import gate.
    assert!(
        check_errs("m11cr-hmac-noimport", "pub fn main() -> Result<(), Error> {\n  t := crypto.hmac_sha256(\"k\", \"d\")\n  print(t.len())\n  return Ok(())\n}\n"),
        "hmac_sha256 without `import std.crypto` must error"
    );
    // hmac arity + type.
    assert!(
        check_errs("m11cr-hmac-arity", "import std.crypto\npub fn main() -> Result<(), Error> {\n  t := crypto.hmac_sha256(\"k\")\n  print(t.len())\n  return Ok(())\n}\n"),
        "hmac_sha256 with 1 argument must error"
    );
    assert!(
        check_errs("m11cr-hmac-type", "import std.crypto\npub fn main() -> Result<(), Error> {\n  t := crypto.hmac_sha256(\"k\", 42)\n  print(t.len())\n  return Ok(())\n}\n"),
        "hmac_sha256 on a non-byte-view data must error"
    );
    // hkdf arity + len-type (a non-i64 len).
    assert!(
        check_errs("m11cr-hkdf-arity", "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.hkdf_sha256(\"s\", \"i\", \"n\")?\n  print(r.len())\n  return Ok(())\n}\n"),
        "hkdf_sha256 with 3 arguments must error"
    );
    assert!(
        check_errs("m11cr-hkdf-lentype", "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.hkdf_sha256(\"s\", \"i\", \"n\", \"42\")?\n  print(r.len())\n  return Ok(())\n}\n"),
        "hkdf_sha256 with a non-i64 len must error"
    );
}
