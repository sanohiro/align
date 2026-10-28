# Conditional Option joins

Run `python3 bench/option_joins/run.py BASELINE_ALIGNC CANDIDATE_ALIGNC` on
one host with two existing compiler artifacts. The runner uses the shared
bounded native supervisor, private scratch, release/baseline codegen, and seven
alternating pairs. It emits LF CSV to stdout and preserves diagnostics on stderr.

The C harness checks all 81 pairs from nine signed values against independent
overflow intrinsics before timing 8,100,000 calls per sample. Inputs include
MIN/MAX, zero, mixed signs and success/overflow transitions. Unsigned checksum
arithmetic is defined modulo 2^64. Kinds 0/1 are checked add/multiply with a zero
fallback; kind 2 is a wrapping-add control without an Option.

This isolates generated scalar-result code, uses no runtime stubs and makes no
whole-application or global bounds-check claim. Native string-search and typed
view correctness remain owned by the driver regression targets in Plan252.
