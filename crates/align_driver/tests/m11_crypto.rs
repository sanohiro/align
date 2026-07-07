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
