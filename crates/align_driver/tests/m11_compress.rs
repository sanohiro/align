//! M11 std.compress Slice 1 — gzip via libz. `compress.gzip_compress(data, level)` /
//! `compress.gzip_decompress(data)` are pure byte→byte codecs wrapping the tuned C engine
//! (`draft.md` §15 keystone strategy: own the memory — the owned `buffer` output — borrow the
//! engine). Input `bytes` is a borrowed view; output is an owned `buffer` (reusing the existing
//! buffer machinery, no new Move type). Strict gzip framing (windowBits 15+16) both ways.
//!
//! These integration tests pin the **language-level contract**: import gate, the byte-view input
//! forms, round-trip correctness through `string`/`buffer`, the gzip magic bytes, the error policy
//! (corrupt/truncated → `Error.Invalid`), the level-out-of-range runtime abort, and buffer
//! ownership/usability. The exhaustive size/level matrix (empty / small / highly-compressible /
//! ~1 MB pseudo-random, at every level 0..=9) + the decompress-bomb cap live in the `align_runtime`
//! unit tests (`gzip_*`), which drive the wrappers directly. (`docs/impl/std-design/compress.md`.)

mod common;
use common::*;

// --- round trip + format ----------------------------------------------------------------------

/// A small string round-trips: compress at level 6, decompress, and the decoded bytes match the
/// original (compared as hex, the only way to name raw `bytes` in a print). The buffer output is
/// owned and usable (`.bytes()` views it, `.len()` measures it).
#[test]
fn gzip_round_trip_small_string() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
import std.encoding
pub fn main() -> Result<(), Error> {
  data := \"The quick brown fox jumps over the lazy dog\"
  comp := compress.gzip_compress(data, 6)?
  back := compress.gzip_decompress(comp.bytes())?
  print(back.len())
  print(encoding.hex_encode(back.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11c-roundtrip", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // "The quick brown fox jumps over the lazy dog" is 43 bytes; its lower-case hex follows.
    let expected_hex = "The quick brown fox jumps over the lazy dog"
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("43\n{expected_hex}\n"));
}

/// An empty input round-trips: `gzip_compress("")` yields a valid (non-empty, header+trailer) gzip
/// stream, and decompressing it recovers zero bytes.
#[test]
fn gzip_round_trip_empty() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
pub fn main() -> Result<(), Error> {
  comp := compress.gzip_compress(\"\", 6)?
  print(comp.len() > 0)
  back := compress.gzip_decompress(comp.bytes())?
  print(back.len())
  return Ok(())
}
";
    let out = build_and_run("m11c-empty", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n0\n");
}

/// The compressed output carries the gzip magic bytes (RFC 1952: `0x1f 0x8b`), pinning the format
/// (not raw DEFLATE / zlib). Checked by hex-encoding the whole stream and inspecting its prefix.
#[test]
fn gzip_output_has_magic_bytes() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
import std.encoding
pub fn main() -> Result<(), Error> {
  comp := compress.gzip_compress(\"pin the format\", 6)?
  print(encoding.hex_encode(comp.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11c-magic", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let hex = String::from_utf8_lossy(&out.stdout);
    assert!(hex.starts_with("1f8b"), "gzip stream must start with the magic bytes 1f 8b, got: {hex}");
}

/// Highly-compressible input actually shrinks (the engine really runs DEFLATE, not stored blocks):
/// 2000 identical bytes compress to well under a tenth of the size, and decompress back to 2000.
#[test]
fn gzip_compresses_repetitive_input() {
    if !backend_available() {
        return;
    }
    let big = "A".repeat(2000);
    let prog = format!(
        "\
import std.compress
pub fn main() -> Result<(), Error> {{
  data := \"{big}\"
  comp := compress.gzip_compress(data, 9)?
  print(comp.len() < 200)
  back := compress.gzip_decompress(comp.bytes())?
  print(back.len())
  return Ok(())
}}
"
    );
    let out = build_and_run("m11c-repetitive", &prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n2000\n");
}

/// The output `buffer` is a real owned, usable handle: after decompress, indexing its `.bytes()`
/// view yields the original bytes (here the first byte of "Zig" is `0x5a` = 90).
#[test]
fn decompressed_buffer_is_owned_and_indexable() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
pub fn main() -> Result<(), Error> {
  comp := compress.gzip_compress(\"Zig\", 6)?
  back := compress.gzip_decompress(comp.bytes())?
  b := back.bytes()
  print(b[0])
  print(back.len())
  return Ok(())
}
";
    let out = build_and_run("m11c-indexable", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "90\n3\n");
}

/// A compress→decompress round trip accepts `bytes` (a `slice<u8>`) as the compress input too, not
/// only a `str` — here the raw bytes come from `hex_decode`. Proves the byte-view input forms.
#[test]
fn gzip_accepts_bytes_input() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
import std.encoding
pub fn main() -> Result<(), Error> {
  raw := encoding.hex_decode(\"deadbeef00ff\")?
  comp := compress.gzip_compress(raw.bytes(), 6)?
  back := compress.gzip_decompress(comp.bytes())?
  print(encoding.hex_encode(back.bytes()))
  return Ok(())
}
";
    let out = build_and_run("m11c-bytes-input", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "deadbeef00ff\n");
}

// --- error policy: corrupt / truncated → Error.Invalid (exit 2) --------------------------------

/// The main boundary exits `tag + 1` on a propagated `Err`; `Error.Invalid` is tag 1 → exit 2.
/// Decompressing non-gzip bytes (no `0x1f 0x8b` magic) is `Error.Invalid`.
#[test]
fn gzip_decompress_non_gzip_is_invalid() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
import std.encoding
pub fn main() -> Result<(), Error> {
  junk := encoding.hex_decode(\"00112233445566778899\")?
  back := compress.gzip_decompress(junk.bytes())?
  print(back.len())
  return Ok(())
}
";
    let out = build_and_run("m11c-nongzip", prog);
    assert_eq!(out.status.code(), Some(2), "non-gzip input → Error.Invalid (exit 2); stderr: {}", String::from_utf8_lossy(&out.stderr));
}

/// A truncated gzip stream (a well-formed 10-byte gzip header with no DEFLATE body / trailer) →
/// `Error.Invalid`: zlib consumes the header, then needs more input that never arrives.
#[test]
fn gzip_decompress_truncated_is_invalid() {
    if !backend_available() {
        return;
    }
    // `1f8b 08 00 00000000 00 03` = magic, CM=deflate, FLG=0, MTIME=0, XFL=0, OS=unix — a complete
    // gzip header with nothing after it (the compressed body + CRC/size trailer are missing).
    let prog = "\
import std.compress
import std.encoding
pub fn main() -> Result<(), Error> {
  head := encoding.hex_decode(\"1f8b080000000000000003\")?
  back := compress.gzip_decompress(head.bytes())?
  print(back.len())
  return Ok(())
}
";
    let out = build_and_run("m11c-truncated", prog);
    assert_eq!(out.status.code(), Some(2), "truncated stream → Error.Invalid (exit 2); stderr: {}", String::from_utf8_lossy(&out.stderr));
}

// --- level bounds ------------------------------------------------------------------------------

/// The boundary levels 0 (no compression) and 9 (best) both round-trip.
#[test]
fn gzip_level_boundaries_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
pub fn main() -> Result<(), Error> {
  lo := compress.gzip_compress(\"boundary\", 0)?
  lo_back := compress.gzip_decompress(lo.bytes())?
  print(lo_back.len())
  hi := compress.gzip_compress(\"boundary\", 9)?
  hi_back := compress.gzip_decompress(hi.bytes())?
  print(hi_back.len())
  return Ok(())
}
";
    let out = build_and_run("m11c-levels", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "8\n8\n");
}

/// An out-of-range `level` is a programmer error (the total-or-abort policy, like `rand.range`): the
/// generated program **aborts** at runtime (`SIGABRT` via `panic_abort`), not a silent clamp / `Err`.
/// A signal-killed process has no normal exit code (`code() == None`) and does not `success()`.
#[test]
fn gzip_out_of_range_level_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.compress
pub fn main() -> Result<(), Error> {
  comp := compress.gzip_compress(\"x\", 99)?
  print(comp.len())
  return Ok(())
}
";
    let out = build_and_run("m11c-badlevel", prog);
    assert!(!out.status.success(), "an out-of-range level must abort the process");
    assert_eq!(out.status.code(), None, "abort is a signal death (SIGABRT), not a normal exit code");
    assert!(out.stdout.is_empty(), "nothing is printed before the abort, got: {:?}", String::from_utf8_lossy(&out.stdout));
}

// --- capability header + arity + purity --------------------------------------------------------

/// Every `compress.*` use requires `import std.compress` (the capability-header rule).
#[test]
fn compress_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  comp := compress.gzip_compress(\"x\", 6)?
  print(comp.len())
  return Ok(())
}
";
    assert!(check_errs("m11c-noimport", src), "compress.* without `import std.compress` must error");
}

/// A missing `import std.compress` names the capability in the diagnostic.
#[test]
fn missing_import_diagnostic_names_the_capability() {
    let diags = check_diagnostics(
        "m11c-diag",
        "pub fn main() -> Result<(), Error> {\n  comp := compress.gzip_decompress(\"x\")?\n  return Ok(())\n}\n",
    );
    assert!(diags.contains("import std.compress"), "diagnostic should name the capability: {diags}");
}

/// `gzip_compress` takes exactly 2 args (data, level); `gzip_decompress` exactly 1 (data). A wrong
/// arity is a compile error.
#[test]
fn compress_wrong_arity_rejected() {
    assert!(
        check_errs("m11c-comp-arity1", "import std.compress\npub fn main() -> Result<(), Error> {\n  c := compress.gzip_compress(\"x\")?\n  return Ok(())\n}\n"),
        "gzip_compress with 1 argument must error"
    );
    assert!(
        check_errs("m11c-decomp-arity2", "import std.compress\npub fn main() -> Result<(), Error> {\n  c := compress.gzip_decompress(\"x\", 6)?\n  return Ok(())\n}\n"),
        "gzip_decompress with 2 arguments must error"
    );
}

/// `level` must be exactly `i64` — a non-integer level is a type error.
#[test]
fn compress_non_integer_level_rejected() {
    let src = "\
import std.compress
pub fn main() -> Result<(), Error> {
  c := compress.gzip_compress(\"x\", \"six\")?
  return Ok(())
}
";
    assert!(check_errs("m11c-level-type", src), "a non-i64 level must be a type error");
}

/// `gzip_compress` is a C-engine (libz) call — Impure. A closure that compresses is never `Pure`, so
/// `par_map` (which requires a Pure closure) rejects it (the `encoding`-is-pure / `io`-is-impure line).
#[test]
fn compress_rejected_by_par_map() {
    let src = "\
import std.compress
fn f(x: i64) -> i64 {
  c := compress.gzip_compress(\"data\", 6) else { return x }
  return c.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11c-parmap", src), "an impure compress-using closure must be rejected by par_map");
}

/// Likewise `gzip_decompress` inside a `par_map` closure is rejected (impure).
#[test]
fn decompress_rejected_by_par_map() {
    let src = "\
import std.compress
fn f(x: i64) -> i64 {
  c := compress.gzip_decompress(\"data\") else { return x }
  return c.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11c-decomp-parmap", src), "an impure decompress-using closure must be rejected by par_map");
}
