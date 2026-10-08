# Bulk discovery during CSV text normalization

Status: **IMPLEMENTED 2026-10-08 — native/source owners and local ARM64 measurements qualified.**

Plan150 accelerates quoted field discovery, but selected escaped text still
walks every ordinary byte again when computing decoded length and emitting
chunks. Reuse the existing portable quote search for those two internal walks.
The public CSV contract and its allocation strategy remain unchanged.

| Boundary | Invariant and owner |
| --- | --- |
| Decoded length | Subtract one byte per validated doubled quote from the raw extent. Preserve None for an unmatched quote, including the last byte. A scalar oracle crosses short byte products and long runs around the 16-byte search boundary. |
| Chunk emission | Preserve every callback byte slice and its order, including empty slices before adjacent pairs and at EOF; stop before emitting the run preceding a malformed quote. The same independent oracle owns full callback sequences, hash, equality and guarded exact copies. |
| Memory and native output | Searches borrow only the Cell extent, allocate nothing and keep existing copy destinations, decoded extents, UTF-8 admission, descriptor validation, error precedence and arena layout. Native selected-text owners cover NUL, Unicode, CR/LF, sparse/dense escapes, exact output and existing allocation/conversion probes. No ownership, Drop, FFI, IR or ABI change. |
| Source consumers | Existing whole/per-unit pkg_csv owners retain application execution and language lifetime rules. No compiler, interface or cache-format change. |
| Measurement | Compare ordinary release archives using a selected-text variant of the existing 31-case CSV probe; verify all decoded bytes outside timing and returned lengths/edge bytes during timing. Retain paired ABBA samples and short, dense, clean and malformed controls. Adopt only with a measured long sparse-text benefit; no universal latency, I/O, RSS or x86 claim. |

The author-side invariant-to-diff pass and one fresh inspection-only review
cover this representation-preserving refinement. Native and source owners,
then the final SHA-bound preflight, qualify the implementation.

## Qualification

`Cell::decoded_len` subtracts one byte per admitted doubled pair, and
`Cell::chunks` discovers each ordinary run with `next_quote`. The shared search
still uses its short scalar prefix and portable bulk tail. The scalar product
and long-range owners pin exact callback sequences, malformed suffix refusal,
decoded lengths, hash/equality and copy sentinels. Selected Unicode/NUL/CRLF
text occupies the original arena tail with one allocation. All 15 native CSV
and 11 active pkg_csv owners pass on macOS/Linux ARM64. All four probe wrappers
pass the existing timeout, leader-exit and signal lifecycle products.

The selected-text probe retains all paired ABBA samples and short/clean/dense/
malformed controls in `bench/csv_normalization`. The author-side extraction pass
maps the unchanged CSV byte, order, extent and allocation obligations to these
owners; no compiler or public-contract change requires specification mirrors.
