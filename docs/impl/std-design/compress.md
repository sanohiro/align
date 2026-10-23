This directory records the detailed `std` module designs that supplement the roadmap.
These specifications define the contract for each module's implementation.

# std.compress — implementation design (M11)

> 🌐 **English** · [Japanese](./ja/compress.md)

> **Status:** complete in M11. gzip, zlib, raw DEFLATE and zstd compression/decompression are shipped.

## Overview

gzip, zlib, raw DEFLATE and zstd compression/decompression (draft §18.2).
Runtime wrappers call `libz`/`libzstd`; Align owns the output buffer and the native
engine fills it.

## Signatures

```text
compress.gzip_compress(data: bytes, level: i64) -> Result<buffer, Error>    // owned output
compress.gzip_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
compress.zstd_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zstd_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
compress.zlib_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.zlib_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
compress.deflate_raw_compress(data: bytes, level: i64) -> Result<buffer, Error>
compress.deflate_raw_decompress(data: bytes, max_bytes: i64 = 1073741824) -> Result<buffer, Error>
```

Each decoder accepts an optional trailing `max_bytes` with default 1073741824.
The type is exactly i64 and the range is `0..=1073741824`. Explicit operands evaluate
once in source order; input remains live through limit evaluation. Invalid limits
abort after operand evaluation, before input inspection or native initialization.
Zero accepts complete empty-output streams, not empty encoded input. Excess output
returns `Error.Invalid`. Every output allocation request and payload-buffer span stays
within the limit; the separate one-byte stack probe detects excess without growing
output. Output grows on demand; the first request is min(64 KiB, max_bytes). Allocator rounding,
transient old/new realloc overlap, native workspace, buffer headers and process RSS
are outside this payload-storage bound. [Plan226 R156](../226-deflate-framing.md#caller-selected-output-limits-llm-r156)
owns this extension.

## Type & ownership classification

Byte→byte transformation. Input `bytes` (borrowed view → its data ptr crosses FFI, length passed
separately, per draft §15 FFI rules). Output owned `buffer` (Align allocates via the buffer
machinery; the C engine writes into it). No new Move type — reuses `buffer` (#346).

## Effect classification

**Impure** — an `extern "C"` call is inferred impure (draft §15: any extern-calling fn is
non-Pure), so compress can never be a `par_map` callee. Fine for its I/O-shaped use.

## Error policy

C-engine error codes (Z_DATA_ERROR, ZSTD error codes) → `Error.Invalid` (corrupt/truncated input)
or `Error.Code` (map the engine's category). Native resource failures and fallible output
allocation failures preserve `Error.Code`; final buffer-header OOM retains the runtime's terminal
OOM policy.

## Complete-input decompression

All decoders borrow the binary input only for the call and publish one independent owned buffer
only after consuming all input successfully. No partial output escapes a later failure.

- gzip accepts one or more complete RFC 1952 members and concatenates their outputs in order,
  including empty members.
- zstd accepts one or more current RFC 8878 ordinary or skippable frames. At every frame boundary,
  the little-endian magic must be `0xFD2FB528` or `0x184D2A50..=0x184D2A5F`; pre-1.0 legacy formats
  are rejected regardless of native build flags. Ordinary outputs are concatenated in order;
  skippable payloads contribute no output. Skippable-only input succeeds with an empty buffer.
- Empty input, incomplete/corrupt members or frames (including later checksums),
  and trailing bytes (including gzip zero padding) return `Error.Invalid`.
- The aggregate decoded payload has the inclusive caller limit across all members/frames (default 1 GiB).
  Exceeding it returns `Error.Invalid`. For gzip/zstd at exactly the cap, further valid empty/skippable frames
  remain valid. This bounds the returned payload, not native workspace or process RSS.

Reuse one native stream and one growing output vector per call. At the cap, a one-byte stack
probe permits metadata progress without growing the returned storage; any produced byte means
`Error.Invalid`. A native engine error takes precedence over the probe result. Every failure
releases native state and partial output. Compression still produces one member/frame. No
streaming source API, dictionary support, or partial-decode API is added.
[Plan 124](../124-complete-decompression.md) owns the exact contract and closure.

Zlib and raw DEFLATE use `libz` with window bits 15 and -15 respectively.
Each decompressor requires exactly one complete stream and all input consumed;
a second stream or any trailing byte is `Error.Invalid`. Raw final-byte padding
bits remain part of that byte; raw DEFLATE has no checksum or identifying magic.
Zlib rejects bad Adler-32 and preset dictionaries. Both keep the inclusive caller
output cap, atomic owned output, Impure effect and existing error mapping.
Compression emits one stream, even for empty input. Levels are `i64` 0..=9;
out-of-range levels abort before native initialization, as for gzip. The source
arguments evaluate in order. No streaming API, dictionary API
or PNG policy is added. [Plan226](../226-deflate-framing.md) owns this completion.

## Runtime and dependencies

Runtime wrappers own native codec state, checked output growth and cleanup, and
return an owned buffer on success. Programs using compression link `-lz`/`-lzstd`;
both libraries are required build dependencies.

## Original M11 slices (completed)

1. gzip (libz) — compress+decompress+size-cap.
2. zstd (libzstd) — same shape.

## Pitfalls

- **P1 (FFI memory safety — the align-self-review Gate 2 core)**: i64→usize via `try_from` (no
  `as usize`), `checked_mul` on buffer sizing, null-guard before `from_raw_parts`, the
  output-growth loop must not overflow.
- **P2 (decompress bomb)**: enforce the caller's aggregate output cap, defaulting to 1 GiB; excess output returns `Error.Invalid`.
- **P3 (external lib dependency)**: libz/libzstd must be linked; the driver's link step needs
  -lz/-lzstd. If absent, linking a compression program fails.
- **P4 (view → FFI ptr)**: `bytes` input lowers to its data ptr only (draft §15); length passed
  separately. A view is not a valid FFI return type (C ptr → raw). Output must be an owned
  buffer, not a view.

## Test checklist

- round-trip gzip/zlib/raw DEFLATE/zstd over empty / small / 1MB random / highly-compressible data →
  `decompress(compress(x)) == x`
- concatenated members/frames, empty members, and all sixteen skippable frame tags
- corrupt/truncated later input, wrong format, legacy zstd and trailing bytes → `Error.Invalid`
- inclusive aggregate cap and exact-cap empty suffixes; errors publish no partial buffer
- decompress bomb (crafted small→huge) → capped `Error.Invalid` (P2)
- level bounds
- buffer owned + Drop-freed
- import-required
- Tests require libz/libzstd; these are part of the supported build environment.
