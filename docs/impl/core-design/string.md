This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — str / string / builder / template

> 🌐 **English** · [Japanese](./ja/string.md)

## Overview

Text (draft §12–§13): a borrowed view type, an owned buffer type, a builder for assembly, and one
template form. Byte-oriented UTF-8 throughout; the searching methods ride the memchr-class SIMD
scan layer (#310). The load-bearing policy: **every string allocation has a visible home** — an
arena, an owner, or a builder; allocation inside pipeline lambdas is a compile error.

## Signatures and settled surface

```text
"lit"                      -> str        // single-line only; \n \t \" escapes; UTF-8
'A' / 'あ'                 -> char       // one Unicode scalar
s.len()                    -> i64        // BYTE length ("あ".len() == 3)
s.contains(n) / s.starts_with(n) / s.ends_with(n)      -> bool
s.eq_ignore_ascii_case(t)  -> bool       // ASCII fold only, not Unicode
s.find(n) / s.rfind(n)     -> Option<i64>   // byte index of first/last occurrence
s.trim() / s.trim_start() / s.trim_end()    -> str   // ASCII-whitespace; zero-copy sub-view
s[a..b]                    -> str        // range view; region-tied; NO s[i] byte indexing
s.clone()                  -> string     // deep copy; the arena-escape hatch
a + b                      -> compile error; builder is the one concatenation path

b := builder()  /  builder(cap)
b.write(s: str|string)  /  b.write_int(i: i64)
b.to_string()              -> string     // the finisher (there is no finish()/build())

template "…{expr}…"        -> str        // holes: int, float, str, bool, char; full expressions
```

Receivers auto-borrow: every method above takes `str` or `string` (an owned `string` is viewed,
not consumed). `hash64`/`hash128` also accept these views ([hash.md](hash.md)).

## Type & ownership classification

- `str` — Copy view `{ptr, len}`, region = the pointed-at data (literals are region-0/static).
- `string` — owned Move heap buffer; drop frees; reassign-drops-old; auto-borrows to `str`.
- `builder` — an owned accumulator; `to_string` finishes it. Adjacent writes are fused by the
  MIR peephole (`fuse_builder_writes`, `"lit" + int + "lit"` → one runtime call) — new write
  shapes should extend the batcher, not bypass it.
- `template` results are arena-regioned `str` inside an arena. The current arena-free lowering leaks
  its payload for process lifetime; this is a confirmed ownership gap, not a performance contract
  ([audit 13](../13-string-array-allocation-short-input-audit.md#33-confirmed-p0p1--arena-free-template-and-jsonencode-leak-forever)).

## Effects

Pure (no I/O). The *allocation-visibility* rules are enforced structurally, not via effects:
`str + str` is a settled hard error everywhere; `template` inside a pipeline lambda without its own
arena is a hard error ("would silently leak" — `lambda.rs`). The current checker still accepts
`str + str`; audit 13 records that implementation/settlement drift as P0.

## Errors & aborts

No `Result` in this area. `s[a..b]` out of bounds aborts. Non-UTF-8 *input* is a `std` boundary
concern (`fs.read_file` → `Error.Invalid`); core string ops assume the invariant and stay
byte-oriented. Range lowering now enforces the promised O(1) UTF-8-scalar-boundary abort at both
endpoints (audit 13 §3.1; fixed 2026-07-13).

## Regions

`region_of(trim*/s[a..b]) = region_of(s)` — sub-views inherit. `clone` → owned, region-free. A
`string` struct field read borrows as a
Frame-regioned `str` (owned-structs work).

## Spec'd but not implemented

- **`split`** and **`find_any`** (§18.1 catalog) — no dispatch arms. `split` is the big one:
  its return shape (`array<str>` of views — a Move array of regioned views) needs the
  Move-element collection work; do not ship it as an owned-copies compromise ("ideal form, or
  defer"). Today: `find`/`rfind` + `s[a..b]` compose the manual split.
- No `s[i]` byte access — deliberate so far (byte access invites indexing bugs on UTF-8); if a
  use case demands it, decide `u8`-returning semantics in `open-questions.md` first.
- The §13/§18.1 template variants (`html`, `raw`, json-template) — only plain `template "…"`
  exists. The escaping-variant design (context-aware autoescape) is unsettled.

## Pitfalls

- P1 — every search/compare is **byte-oriented**; document char-vs-byte in anything user-facing
  (find returns a *byte* index — valid input to `s[a..b]`, not a char count).
- P2 — `str + str` is a settled hard error, not a pipeline-lambda-only rule. Use builder; do not
  weaken it to a lint or revive the stale arena-concat implementation.
- P3 — `builder.to_string` is the only finisher; adding `finish()` aliases violates One-way.
- P4 — `eq_ignore_ascii_case` is ASCII-only by name and by design; a Unicode case-fold is a
  different (locale-infested) feature — reject as out of scope, per non-goals.

## Test anchors

`m5.rs` (methods incl. find/rfind pairs, trim family, builder incl. fuse,
template, escapes, UTF-8 byte lengths, print type coverage); `lambda.rs:271/280/287/294`
(template allocation rejection + arena-in-lambda allowance); `hash.rs` (view acceptance);
`fuzz_fmt.rs` (formatter round-trips string-heavy sources); examples `strings.align`,
`template.align`. The remaining concat-acceptance tests are stale until audit-13 P0 is fixed. SIMD
scan pin: #310 differential oracle.
