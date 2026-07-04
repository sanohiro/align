This directory holds Opus-implementable design specs for std modules beyond the roadmap's
prose. Authored by the main loop (Fable); these are the source of truth for implementing each
module.

# std.compress — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/compress.md)

## Overview

gzip, zstd (draft §18.2). The keystone library strategy: **own the memory wrappers, borrow the
mathematical engine** (draft §15) — wrap `libz`/`libzstd` via `extern "C" link("z"|"zstd")` rather
than reimplementing tuned DEFLATE/zstd. Align allocates the output (arena/buffer); the C engine
fills it.

## Signatures

```text
compress.gzip_compress(data: bytes, level: i64) -> Result<buffer, Error>    // owned output
compress.gzip_decompress(data: bytes) -> Result<buffer, Error>
compress.zstd_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zstd_decompress(data: bytes) -> Result<buffer, Error>
```

## Type & ownership classification

Pure byte→byte. Input `bytes` (borrowed view → its data ptr crosses FFI, length passed
separately, per draft §15 FFI rules). Output owned `buffer` (Align allocates via the buffer
machinery; the C engine writes into it). No new Move type — reuses `buffer` (#346).

## Effect classification

**Impure** — an `extern "C"` call is inferred impure (draft §15: any extern-calling fn is
non-Pure), so compress can never be a `par_map` callee. Fine for its I/O-shaped use.

## Error policy

C-engine error codes (Z_DATA_ERROR, ZSTD error codes) → `Error.Invalid` (corrupt/truncated input)
or `Error.Code` (map the engine's category). Decompress bomb guard: cap the output size (a
decompress-size limit param or a hard cap) — record as a v1 safety knob.

## New machinery required

The FFI `link()` path (exists, M8 #265-269) + safe unsafe wrappers that: allocate the output
buffer, call the C fn with (in_ptr, in_len, out_ptr, out_cap), handle the "output too small →
grow + retry" loop, return owned buffer. The build must link `-lz`/`-lzstd` (driver link step — a
new external dependency; document that libz/libzstd must be present. Consider making the module
opt-in / feature-gated if the lib is absent).

## Slice breakdown

1. gzip (libz) — compress+decompress+size-cap.
2. zstd (libzstd) — same shape.

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64→usize via `try_from` (no
  `as usize`), `checked_mul` on buffer sizing, null-guard before `from_raw_parts`, the
  output-grow-retry loop must not overflow. Highest risk — this is exactly the FFI/alloc bug
  class the review skill exists for.
- **P2 (decompress bomb)**: a tiny input can decompress to gigabytes. Cap output (param or hard
  limit) → `Error.Invalid` on exceed. Don't allocate unboundedly from attacker-controlled input.
- **P3 (external lib dependency)**: libz/libzstd must be linked; the driver's link step needs
  -lz/-lzstd. If absent, build fails — document the dependency; consider feature-gating so a build
  without the lib still works (compress simply unavailable).
- **P4 (view → FFI ptr)**: `bytes` input lowers to its data ptr only (draft §15); length passed
  separately. A view is not a valid FFI return type (C ptr → raw). Output must be an owned
  buffer, not a view.

## Test checklist

- round-trip gzip/zstd over empty / small / 1MB random / highly-compressible data →
  `decompress(compress(x)) == x`
- corrupt input → `Error.Invalid`
- decompress bomb (crafted small→huge) → capped `Error.Invalid` (P2)
- level bounds
- buffer owned + Drop-freed
- import-required
- (Tests require libz/libzstd present — gate the test on availability.)
