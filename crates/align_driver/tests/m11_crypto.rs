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

// --- AEAD: aes_gcm + chacha20_poly1305 (Slice 4) ----------------------------------------------
//
// `crypto.aes_gcm_seal/open` + `crypto.chacha20_poly1305_seal/open(key, nonce, data, aad) ->
// Result<buffer, Error>` via OpenSSL libcrypto's EVP_CIPHER. Combined format: seal → `ciphertext ||
// tag` (one buffer, 16-byte tag appended); open takes that same combined input. key = 32 bytes,
// nonce = 12 bytes (public values, validated before the engine → Error.Invalid on a mismatch).
// All-or-nothing open (P2): any tamper / truncation → the single opaque Error.Invalid, never partial
// plaintext. Both Impure (a C-engine call), so par_map rejects them. Byte inputs for the known
// vectors are built with `encoding.hex_decode(...).bytes()`; outputs are `hex_encode`d and compared.

/// `aes_gcm_seal` matches the NIST GCM spec Test Case 16 (AES-256-GCM, 60-byte plaintext + 20-byte
/// AAD); the combined output is the 60-byte ciphertext followed by the 16-byte tag, and `aes_gcm_open`
/// round-trips it back to the original plaintext.
#[test]
fn aes_gcm_seal_open_nist_vector() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  key := encoding.hex_decode(\"feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308\")?
  nonce := encoding.hex_decode(\"cafebabefacedbaddecaf888\")?
  pt := encoding.hex_decode(\"d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39\")?
  aad := encoding.hex_decode(\"feedfacedeadbeeffeedfacedeadbeefabaddad2\")?
  sealed := crypto.aes_gcm_seal(key.bytes(), nonce.bytes(), pt.bytes(), aad.bytes())?
  print(sealed.len())
  print(encoding.hex_encode(sealed.bytes()))
  opened := crypto.aes_gcm_open(key.bytes(), nonce.bytes(), sealed.bytes(), aad.bytes())?
  print(encoding.hex_encode(opened.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-aesgcm-vec", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "76\n\
         522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f66276fc6ece0f4e1768cddf8853bb2d551b\n\
         d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39\n"
    );
}

/// `chacha20_poly1305_seal` matches the RFC 8439 §2.8.2 known-answer vector (combined ciphertext ||
/// tag), and `chacha20_poly1305_open` round-trips it back to the original plaintext.
#[test]
fn chacha20_poly1305_seal_open_rfc8439_vector() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  key := encoding.hex_decode(\"808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f\")?
  nonce := encoding.hex_decode(\"070000004041424344454647\")?
  aad := encoding.hex_decode(\"50515253c0c1c2c3c4c5c6c7\")?
  pt := \"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.\"
  sealed := crypto.chacha20_poly1305_seal(key.bytes(), nonce.bytes(), pt, aad.bytes())?
  print(encoding.hex_encode(sealed.bytes()))
  opened := crypto.chacha20_poly1305_open(key.bytes(), nonce.bytes(), sealed.bytes(), aad.bytes())?
  print(encoding.hex_encode(opened.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-chacha-vec", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "d31a8d34648e60db7b86afbc53ef7ec2a4aded51296e08fea9e2b5a736ee62d63dbea45e8ca9671282fafb69da92728b1a71de0a9e060b2905d6a5b67ecd3b3692ddbd7f2d778b8c9803aee328091b58fab324e4fad675945585808b4831d7bc3ff4def08e4b7a9de576d26586cec64b61161ae10b594f09e26a7e902ecbd0600691\n\
         4c616469657320616e642047656e746c656d656e206f662074686520636c617373206f66202739393a204966204920636f756c64206f6666657220796f75206f6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73637265656e20776f756c642062652069742e\n"
    );
}

/// Round-trip edge shapes for both ciphers: empty plaintext (→ a 16-byte tag-only output that opens
/// back to empty), empty aad, and a large (~1 MiB, filled by `crypto.random`) plaintext. Prints one
/// `true` per successful round-trip (compared in constant time).
#[test]
fn aead_round_trips_both_ciphers() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  k := buffer(32)
  crypto.random(k)
  n := buffer(12)
  crypto.random(n)
  // Empty plaintext → 16-byte tag-only output; opens back to empty.
  s0 := crypto.aes_gcm_seal(k.bytes(), n.bytes(), \"\", \"aad\")?
  print(s0.len())
  o0 := crypto.aes_gcm_open(k.bytes(), n.bytes(), s0.bytes(), \"aad\")?
  print(o0.len())
  // Empty aad round-trips (chacha).
  s1 := crypto.chacha20_poly1305_seal(k.bytes(), n.bytes(), \"hello world\", \"\")?
  o1 := crypto.chacha20_poly1305_open(k.bytes(), n.bytes(), s1.bytes(), \"\")?
  print(crypto.constant_time_equal(o1.bytes(), \"hello world\"))
  // Large ~1 MiB plaintext round-trips (aes).
  big := buffer(1048576)
  crypto.random(big)
  s2 := crypto.aes_gcm_seal(k.bytes(), n.bytes(), big.bytes(), \"meta\")?
  print(s2.len())
  o2 := crypto.aes_gcm_open(k.bytes(), n.bytes(), s2.bytes(), \"meta\")?
  print(crypto.constant_time_equal(o2.bytes(), big.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-aead-roundtrip", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // s0=16, o0=0, chacha rt true, s2 = 1048576+16 = 1048592, aes rt true.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "16\n0\ntrue\n1048592\ntrue\n");
}

/// All-or-nothing (P2), observed at the language level: an `open` that fails returns
/// `Err(Error.Invalid)`, propagated by `?` to exit code 2, and the plaintext binding is never reached
/// — no partial plaintext is observable. The exhaustive byte-flip tamper cases (tag / ciphertext /
/// aad, and truncation to 15 / 0) are pinned in the `align_runtime` unit tests (`aead_open_*`), which
/// can mutate individual bytes; here we cover the realistic in-language failure inputs: a wrong aad,
/// a wrong key, and truncation via slicing. Each case exits 2 (Error.Invalid via `?`); a matching
/// `match` variant prints nothing before the failing `open`, so a leak would show as stdout.
#[test]
fn aead_open_failures_are_invalid_and_yield_nothing() {
    if !backend_available() {
        return;
    }
    // Body fragment that seals `"top secret"` under a fresh key/nonce, then runs the `<open>`
    // expression with `?` — reaching `print` only if the (wrong) open unexpectedly succeeded.
    let mk = |open_expr: &str| -> String {
        format!(
            "\
import std.crypto
pub fn main() -> Result<(), Error> {{
  k := buffer(32)
  crypto.random(k)
  n := buffer(12)
  crypto.random(n)
  sealed := crypto.aes_gcm_seal(k.bytes(), n.bytes(), \"top secret\", \"ctx\")?
  wrong := buffer(32)
  crypto.random(wrong)
  pt := {open_expr}?
  print(pt.len())
  return Ok(())
}}
"
        )
    };
    let cases: [(&str, &str); 4] = [
        // Correct key/nonce/ciphertext, but a different aad → auth fails.
        ("wrong-aad", "crypto.aes_gcm_open(k.bytes(), n.bytes(), sealed.bytes(), \"CTX\")"),
        // A wrong key → auth fails.
        ("wrong-key", "crypto.aes_gcm_open(wrong.bytes(), n.bytes(), sealed.bytes(), \"ctx\")"),
        // Truncated to 15 bytes (< the 16-byte tag) → Invalid.
        ("trunc-15", "crypto.aes_gcm_open(k.bytes(), n.bytes(), sealed.bytes()[0..15], \"ctx\")"),
        // Truncated to empty → Invalid.
        ("trunc-0", "crypto.aes_gcm_open(k.bytes(), n.bytes(), sealed.bytes()[0..0], \"ctx\")"),
    ];
    for (tag, expr) in cases {
        let out = build_and_run(&format!("m11cr-aead-fail-{tag}"), &mk(expr));
        assert_eq!(
            out.status.code(),
            Some(2),
            "{tag}: a failed open must be Error.Invalid (exit 2), never partial plaintext; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "", "{tag}: no plaintext must be observable before the failing open");
    }
}

/// Cross-cipher confusion (P2): sealing with AES-256-GCM and opening with ChaCha20-Poly1305 under the
/// same key/nonce fails — the tag never verifies under the wrong cipher — → `Error.Invalid` (exit 2),
/// with no plaintext observable.
#[test]
fn aead_cross_cipher_confusion_is_invalid() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  k := buffer(32)
  crypto.random(k)
  n := buffer(12)
  crypto.random(n)
  sealed := crypto.aes_gcm_seal(k.bytes(), n.bytes(), \"secret\", \"aad\")?
  pt := crypto.chacha20_poly1305_open(k.bytes(), n.bytes(), sealed.bytes(), \"aad\")?
  print(pt.len())
  return Ok(())
}
";
    let out = build_and_run("m11cr-aead-crosscipher", prog);
    assert_eq!(out.status.code(), Some(2), "cross-cipher open must be Error.Invalid; stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

/// Public-value validation (P1): a wrong key length (16/31/33) or wrong nonce length (11/13/16) is
/// `Error.Invalid` (exit 2), rejected before any engine call, for both seal and open.
#[test]
fn aead_wrong_key_or_nonce_length_is_invalid() {
    if !backend_available() {
        return;
    }
    // `k` is the right length; we slice it to make wrong-length views (a slice<u8> is a byte view).
    let mk = |op: &str, key_slice: &str, nonce_slice: &str| -> String {
        format!(
            "\
import std.crypto
pub fn main() -> Result<(), Error> {{
  k := buffer(64)
  crypto.random(k)
  input := buffer(64)
  crypto.random(input)
  r := crypto.{op}(k.bytes()[0..{key_slice}], k.bytes()[0..{nonce_slice}], input.bytes(), \"aad\")?
  print(r.len())
  return Ok(())
}}
"
        )
    };
    // seal: wrong key lengths (16/31/33), then wrong nonce lengths (11/13/16). open: two samples.
    let cases: [(&str, &str, &str); 8] = [
        ("aes_gcm_seal", "16", "12"),
        ("aes_gcm_seal", "31", "12"),
        ("aes_gcm_seal", "33", "12"),
        ("chacha20_poly1305_seal", "32", "11"),
        ("chacha20_poly1305_seal", "32", "13"),
        ("chacha20_poly1305_seal", "32", "16"),
        ("aes_gcm_open", "31", "12"),
        ("chacha20_poly1305_open", "32", "13"),
    ];
    for (i, (op, kl, nl)) in cases.iter().enumerate() {
        let out = build_and_run(&format!("m11cr-aead-len-{i}"), &mk(op, kl, nl));
        assert_eq!(
            out.status.code(),
            Some(2),
            "{op} key[0..{kl}] nonce[0..{nl}] must be Error.Invalid; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The AEAD ops call the libcrypto engine — Impure. A closure that seals or opens is never `Pure`, so
/// `par_map` (which requires a Pure closure) rejects it (one direction suffices per the slice plan;
/// here seal + open).
#[test]
fn aead_impure_rejected_by_par_map() {
    let seal_src = "\
import std.crypto
fn f(x: i64) -> i64 {
  k := buffer(32)
  crypto.random(k)
  n := buffer(12)
  crypto.random(n)
  r := crypto.aes_gcm_seal(k.bytes(), n.bytes(), \"pt\", \"\")
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
    assert!(check_errs("m11cr-aesgcm-parmap", seal_src), "an impure aes_gcm_seal closure must be rejected by par_map");

    let open_src = "\
import std.crypto
fn f(x: i64) -> i64 {
  k := buffer(32)
  crypto.random(k)
  n := buffer(12)
  crypto.random(n)
  r := crypto.chacha20_poly1305_open(k.bytes(), n.bytes(), \"0123456789abcdef\", \"\")
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
    assert!(check_errs("m11cr-chacha-parmap", open_src), "an impure chacha20_poly1305_open closure must be rejected by par_map");
}

/// The four AEAD surfaces require `import std.crypto` and take exactly 4 byte-view arguments; a wrong
/// arity or a non-byte-view argument is a compile error.
#[test]
fn aead_wrong_shape_rejected() {
    // import gate.
    assert!(
        check_errs(
            "m11cr-aead-noimport",
            "pub fn main() -> Result<(), Error> {\n  r := crypto.aes_gcm_seal(\"k\", \"n\", \"p\", \"a\")?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "aes_gcm_seal without `import std.crypto` must error"
    );
    // arity (3 args).
    assert!(
        check_errs(
            "m11cr-aead-arity",
            "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.aes_gcm_open(\"k\", \"n\", \"c\")?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "aes_gcm_open with 3 arguments must error"
    );
    // a non-byte-view argument (an i64 nonce).
    assert!(
        check_errs(
            "m11cr-aead-type",
            "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.chacha20_poly1305_seal(\"k\", 12, \"p\", \"a\")?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "chacha20_poly1305_seal on a non-byte-view nonce must error"
    );
}

// --- argon2id (Slice 5) -----------------------------------------------------------------------
//
// `crypto.argon2id(password: bytes, salt: bytes, params: argon2_params) -> Result<buffer, Error>`
// via OpenSSL libcrypto's `EVP_KDF_fetch("ARGON2ID")`. `argon2_params` is a builtin **Copy** struct
// `{ m_cost, t_cost, parallelism, len }` (all i64), constructed with an ordinary struct literal so
// the security-tuning knobs are named, never positional. The canonical KAT is the phc-winner-argon2
// reference test.c argon2id vector (v=0x13): password "password", salt "somesalt", t=2, m=65536
// KiB, p=1, len=32. Public param bounds are validated before the engine → Error.Invalid. The full
// bound matrix + determinism + param sensitivity are pinned in the align_runtime unit tests
// (`argon2id_*`); these tests pin the language-level contract (the struct-literal call site, the
// import/arity/type gates, purity, and one KAT end-to-end). (`docs/impl/std-design/crypto.md`.)

/// The canonical reference vector round-trips end-to-end through an `argon2_params` struct literal:
/// the 32-byte tag renders to the phc-winner-argon2 reference hex.
#[test]
fn argon2id_canonical_vector() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  tag := crypto.argon2id(\"password\", \"somesalt\", argon2_params{m_cost: 65536, t_cost: 2, parallelism: 1, len: 32})?
  print(tag.len())
  print(encoding.hex_encode(tag.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-argon-kat", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "32\n09316115d5cf24ed5a15a31a3ba326e5cf32edc24702987c02b6566f61913cf7\n"
    );
}

/// The `argon2_params` struct can be built in a variable (not only inline at the call site) and
/// passed by value — it is an ordinary Copy struct. Same tiny-cost derivation twice is deterministic.
#[test]
fn argon2id_params_struct_is_a_first_class_value() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
import std.encoding
pub fn main() -> Result<(), Error> {
  p := argon2_params{m_cost: 64, t_cost: 1, parallelism: 1, len: 32}
  a := crypto.argon2id(\"password\", \"somesalt\", p)?
  b := crypto.argon2id(\"password\", \"somesalt\", p)?
  print(crypto.constant_time_equal(a.bytes(), b.bytes()))   // same params/inputs → same tag
  print(encoding.hex_encode(a.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11cr-argon-var", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\n729c7a54441bc13559bdca71348c4e554599e719c08a952601ed5c83618c1bbd\n"
    );
}

/// Empty password is valid (salt still >= 8); it derives a 32-byte tag rather than erroring.
#[test]
fn argon2id_empty_password_ok() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  tag := crypto.argon2id(\"\", \"somesalt\", argon2_params{m_cost: 64, t_cost: 1, parallelism: 1, len: 32})?
  print(tag.len())
  return Ok(())
}
";
    let out = build_and_run("m11cr-argon-emptypw", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "32\n");
}

/// A violated public bound (t_cost < 1, parallelism < 1, m_cost < 8*parallelism, len < 4) or a salt
/// shorter than the RFC 8-byte Argon2 minimum → `Error.Invalid` (tag 1 → exit 2 via the propagated
/// `?` at the `main` boundary), before/at the engine, never a partial result.
#[test]
fn argon2id_invalid_params_are_error_invalid() {
    if !backend_available() {
        return;
    }
    // (tag, params-body, salt) — each row violates exactly one rule.
    let cases = [
        ("tcost0", "m_cost: 64, t_cost: 0, parallelism: 1, len: 32", "somesalt"),
        ("par0", "m_cost: 64, t_cost: 1, parallelism: 0, len: 32", "somesalt"),
        ("mtoolow", "m_cost: 15, t_cost: 1, parallelism: 2, len: 32", "somesalt"),
        ("len3", "m_cost: 64, t_cost: 1, parallelism: 1, len: 3", "somesalt"),
        ("shortsalt", "m_cost: 64, t_cost: 1, parallelism: 1, len: 32", "short"),
    ];
    for (tag, params, salt) in cases {
        let prog = format!(
            "\
import std.crypto
pub fn main() -> Result<(), Error> {{
  out := crypto.argon2id(\"pw\", \"{salt}\", argon2_params{{{params}}})?
  print(out.len())
  return Ok(())
}}
"
        );
        let out = build_and_run(&format!("m11cr-argon-inv-{tag}"), &prog);
        assert_eq!(
            out.status.code(),
            Some(2),
            "argon2id {tag} → Error.Invalid (exit 2); stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The just-valid boundary of each bound derives (proves `>=`/`<=`, not `>`/`<`): m_cost == 8,
/// m_cost == 8*parallelism, len == 4, salt == 8 bytes.
#[test]
fn argon2id_boundary_valid_params_ok() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.crypto
pub fn main() -> Result<(), Error> {
  a := crypto.argon2id(\"pw\", \"exactly8\", argon2_params{m_cost: 8, t_cost: 1, parallelism: 1, len: 32})?
  b := crypto.argon2id(\"pw\", \"exactly8\", argon2_params{m_cost: 16, t_cost: 1, parallelism: 2, len: 4})?
  print(a.len())
  print(b.len())
  return Ok(())
}
";
    let out = build_and_run("m11cr-argon-boundary", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "32\n4\n");
}

/// `argon2id` calls the libcrypto engine — Impure. A closure that uses it is never `Pure`, so
/// `par_map` (which requires a Pure closure) rejects it.
#[test]
fn argon2id_impure_rejected_by_par_map() {
    let src = "\
import std.crypto
fn f(x: i64) -> i64 {
  r := crypto.argon2id(\"pw\", \"somesalt\", argon2_params{m_cost: 64, t_cost: 1, parallelism: 1, len: 32})
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
    assert!(check_errs("m11cr-argon-parmap", src), "an impure argon2id closure must be rejected by par_map");
}

/// `argon2id` requires `import std.crypto` and takes exactly `(password, salt, params)` — a wrong
/// arity, a non-byte-view password/salt, or a non-`argon2_params` third argument is a compile error.
/// The `argon2_params` type name is reserved (a user cannot redeclare it).
#[test]
fn argon2id_wrong_shape_rejected() {
    // import gate.
    assert!(
        check_errs(
            "m11cr-argon-noimport",
            "pub fn main() -> Result<(), Error> {\n  r := crypto.argon2id(\"pw\", \"somesalt\", argon2_params{m_cost: 64, t_cost: 1, parallelism: 1, len: 32})?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "argon2id without `import std.crypto` must error"
    );
    // arity (2 args).
    assert!(
        check_errs(
            "m11cr-argon-arity",
            "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.argon2id(\"pw\", \"somesalt\")?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "argon2id with 2 arguments must error"
    );
    // a non-byte-view password (an i64).
    assert!(
        check_errs(
            "m11cr-argon-pwtype",
            "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.argon2id(42, \"somesalt\", argon2_params{m_cost: 64, t_cost: 1, parallelism: 1, len: 32})?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "argon2id on a non-byte-view password must error"
    );
    // a non-argon2_params third argument (an i64).
    assert!(
        check_errs(
            "m11cr-argon-paramstype",
            "import std.crypto\npub fn main() -> Result<(), Error> {\n  r := crypto.argon2id(\"pw\", \"somesalt\", 64)?\n  print(r.len())\n  return Ok(())\n}\n"
        ),
        "argon2id on a non-argon2_params third argument must error"
    );
    // `argon2_params` is a reserved builtin type name — a user struct declaration of it is rejected.
    assert!(
        check_errs(
            "m11cr-argon-reserved",
            "argon2_params { a: i64 }\npub fn main() -> i32 {\n  return 0\n}\n"
        ),
        "redeclaring the reserved `argon2_params` type must error"
    );
}
