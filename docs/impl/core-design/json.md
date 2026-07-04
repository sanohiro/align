This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — json

> 🌐 **English** · [Japanese](./ja/json.md)

## Overview

Typed records across the text boundary (draft §14). Two functions — encode and decode — with the
target type carried by **inference, never a written type argument** (settled: Align has no
expression-position type-argument syntax / no turbofish). Requires `import core.json` (the
capability-header rule applies to core.json exactly like std modules).

## Signatures (verified)

```text
json.encode(x)   -> str                      // x: flat struct; str fields JSON-escaped
json.decode(s)   -> Result<T, Error>         // T from the binding/context: u: User := json.decode(s)?

// decode targets, all verified:
//   flat struct            (field order free; unknown keys ignored)
//   array<i64> / array<f64>
//   array<Struct>          (AoS; str fields = zero-copy views into the input)
//   soa<Struct>            (direct columnar decode — no AoS intermediate, no transpose;
//                           inside arena {}; str columns borrow the input text)
```

## Type & ownership classification

- `encode` builds through the string builder; result is an arena-regioned `str`.
- `decode` into `array<T>`/`array<Struct>` produces an owned Move array (deep-dropped).
- `decode` into `soa<T>` allocates columns in the enclosing arena (`align_rt_json_decode_soa`,
  one count pass + one value-parse pass sharing the Mison speculation via `FieldDst`).
- Decoded `str` fields/columns are **views into the input `str`** — the input must outlive the
  decoded value; the region checker enforces it.

## Effects

Pure (parsing is computation; no I/O — pair it with `std.fs`/`std.io` for the bytes).

## Errors & aborts

Everything malformed is `Err(Error)` — never a panic, never a silently-wrong value: syntax
errors, missing fields, type mismatches, **out-of-range integers** (sign-carrying field tag,
#295; `u64` fields accept the full `u64` range through one write dispatcher, #311). Duplicate
keys on the speculative path resolve consistently (last-wins parity with the slow path, #306 —
zero new state, cost confined to records with undeclared colons).

## Regions

`region_of(decoded str view) = region_of(input)`; `region_of(soa columns) = enclosing arena`;
owned arrays escape freely. Escaping a decoded view past its input is caught at the escape
point (clone out to keep).

## Spec'd but not implemented

- `json.scan`, `json.token` (streaming/SAX tier), `json.validate<T>`, `json.field_table<T>`
  (§18.1 catalog) — no dispatch arms. The `<T>`-explicit pair is *also* blocked on the settled
  no-turbofish rule: §18.1 already records that they are "the residual schema-selector case …
  may fold into `decode`". Settle that in `open-questions.md` before implementing anything here.
- `json.decode<T>(...)` call syntax — permanently out (settled); the annotation-through-`?`
  form is the one way.
- Nested-struct / Option-field / enum-payload decode targets — not in the verified matrix;
  extending the target grammar is design work (field tables, null policy) before code.

## Pitfalls

- P1 — **the decode target grammar is a whitelist**, enforced in sema: adding a target type
  means sweeping the same speculation/fallback machinery (count pass, `FieldDst`, error tags) —
  partial support that panics on exotic shapes is the bug class #295 closed; don't reopen it.
- P2 — the speculative (Mison PHF) path and the slow path must stay **observably identical**
  (duplicate keys, escapes, number edges). Any parser change needs both paths re-fuzzed
  (`fuzz_differential`-style oracle or the m5 corpus).
- P3 — encode's escaping table lives in the builder path — new escapable field types must
  extend it, not inline ad-hoc escaping.
- P4 — the soa decode's performance contract (≈serde parity at 1M rows, `bench/json_soa`) is a
  regression tripwire: re-run the bench before landing parser changes.

## Test anchors

`m5.rs` (decode matrix: struct/arrays/str-fields/order/unknown-keys/malformed/range #295 #311;
encode escaping; duplicate-key #306), `soa.rs:317` (json→soa filtered aggregate), examples
`json.align`, `json_decode.align`, `soa_json_str.align`; benches `bench/json_decode`,
`bench/json_soa` (+ their READMEs for the measured model).
