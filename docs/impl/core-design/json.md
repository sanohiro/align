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
json.encode(x)   -> str                      // x: struct (nested structs recurse); str fields JSON-escaped
json.decode(s)   -> Result<T, Error>         // T from the binding/context: u: User := json.decode(s)?

// decode targets, all verified:
//   struct                 (flat OR with nested-struct / Option<T> / array<Struct> fields; field order free; unknown keys ignored)
//   array<i64> / array<f64>
//   array<Struct>          (AoS; str fields = zero-copy views into the input; nested-struct + Option fields recurse)
//   soa<Struct>            (direct columnar decode — no AoS intermediate, no transpose;
//                           inside arena {}; str columns borrow the input text; primitive/str columns only,
//                           NO nested columns — the owned-columns deferral stands)
//   enum (union)           (shape-directed: a JSON oneOf → a sum type; the variant is selected by the
//                           value's shape class — str/number/bool/object; O(1) first-byte dispatch;
//                           str payloads borrow the input; array-payload variants = J2)
```

**Union (sum-type) targets (JSON completeness J1b).** A JSON `oneOf` maps to a sum type
discriminated by the value's **shape class** — `Str` (`"`) / `Number` (digit/`-`) / `Bool` (`t`/`f`)
/ `Object` (`{`) — an O(1) dispatch on the first structural byte. **Compile-checked (the Align
move):** a union-decodable enum has every variant carry exactly one payload, each payload mapping to
one shape class, all classes **pairwise distinct** — `i64 | f64` (both Number) or two object payloads
are a compile error naming the clash; a tag-only or no-shape (`char`) payload is rejected too. `null`
is not a class (absence belongs to `Option`); a runtime value whose shape has no variant (e.g. an
array, or `null`) is a decode `Err`. Encode writes the live variant's payload **bare** (no wrapper
key), so `decode(encode(x))` round-trips by construction. Runtime: a `JsonUnion` descriptor (one
`JsonField` payload arm per variant + a shape-class→arm table + an arm→enum-tag table); decode
classifies the first byte, writes the payload via the shared `write_value`, and sets the tag; encode
reads the tag and emits the live arm via the shared `json_encode_value`. **v1 boundary:** payloads are
str / number / bool / object (an owned `array<Struct>` payload — the OpenAI multimodal `content`
union — needs enum owned-payload drop, J2). `json.encode` of a union needs a local binding (like
struct encode). **Union as a struct field** (`Message { content: Content }`) is J1b-2b.

**`array<Struct>` fields (REST-gateway runway, Slice C).** A struct field may be an owned
`array<Struct>` — the `messages: array<Message>` / `choices: array<Choice>` shape; the full OpenAI
request/response now round-trips. Decode: a descriptor kind 5 (`sub` = element schema) drives
`decode_struct_array_value`, which parses the JSON sub-array into an owned AoS (`parse_object` per
element, so nested/`Option` element fields recurse) and writes `{ptr,len}` to the field; the field
buffer is freed by the struct's `Drop`. Encode: a `StructArrayField` piece calls the runtime
descriptor-driven encoder (`json_encode_struct_array` → `json_encode_object`, **reusing the decode
descriptors** — symmetric, handles nested/Option/str/scalar). **Memory-safety:** on a decode `Err`
after an array field allocated, `drop_decoded_owned` frees the partial struct's AoS buffers (the
runtime dual of codegen `drop_struct_fields`). **v1 element restriction:** non-owned (scalar /
`str`-view / plain-data struct) — `array<string>` / `array<Move-struct>` rejected at declaration.
**Constraint:** a Move struct (owns an array) can't be a `Result`/`Option` Ok payload across a
function boundary — decode + use in-scope. Deferred: `array<scalar>` field decode, owned-element
arrays.

**`Option<T>` fields (REST-gateway runway, Slice B).** A struct field may be an `Option<T>` (payload
scalar / `str` / nested struct). **Null policy:** decode maps a missing key → `None`, JSON `null` →
`None`, a type mismatch → `Err`; a required (non-`Option`) field still `Err`s when missing. **Encode
omits a `None` field entirely** (never `"k":null`), so `decode(encode(x))` round-trips. Runtime: the
`JsonField` descriptor gains `opt_tag` (`-1` = required, else the `Option` tag byte offset); an
optional field is exempt from `all_required_seen`, and the shared `write_value` writes the payload at
the payload slot then sets the `Some` tag. Encode switches an `Option`-bearing object to a
trailing-comma layout with one `align_rt_builder_pop_comma` before `}` (a pure-required object keeps
the static layout). **v1 boundary:** an Option payload must be **non-owned** (`Option<string>` /
`Option<Move-struct>` rejected at declaration — no consumer, and owned-Option-drop-as-a-field is
deferred). One recorded follow-up: `Option<struct>` **encode** (decode supports it).

**Nested-struct fields (REST-gateway runway, Slice A).** A struct field may itself be a `Struct`;
`decode` recurses into the nested object and `encode` renders it back, so a nested record round-trips.
Runtime: the field descriptor carries kind 4 with a `JsonSubTable` pointer (the nested struct's own
descriptors + PHF + store size), and `parse_object` / `write_field_indexed` recurse — so BOTH the slow
path and the Mison speculative path handle nesting (a nested field is one record-level colon whose
value the record-splitter leaves at a deeper bracket depth). Nested `str` fields stay zero-copy views
into the input, so the whole value is region-tied to it recursively (`struct_has_str` recurses). Still
deferred here: `Option<T>` fields (Slice B), `array<T>` fields (Slice C), enum-payload targets.

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

## Designed but not implemented (the JSON-completeness design, settled 2026-07-18)

The full design lives in `open-questions.md` → "JSON completeness — DESIGN SETTLED" (the
implementation source of truth; spec text in draft §14 + §18.1). Remaining slices J1–J6:

- **Unions (J1–J2):** a JSON `oneOf` maps to a sum type discriminated by pairwise-distinct
  **shape classes** (Str/Number/Bool/Object/Array; compile-checked; O(1) first-byte dispatch;
  encode writes the live payload bare). Language prerequisite: enum `str` payloads (region
  tracking) then owned payloads (`array<Struct>`, tag-switched drop). **SHIPPED so far:** enum `str`
  payloads + region tracking (J1a); enum as a struct field (J1b-1); top-level union decode/encode
  over str/number/bool/object payloads (J1b-2a, documented above). **Remaining:** union as a struct
  field (J1b-2b) → enum owned `array<Struct>` payloads + the full `Content` union (J2).
- **Matrix fill (J3):** top-level scalar targets, `array<scalar>` fields, `Option<struct>`
  encode, supported-constructor compositions.
- **`json.doc` (J4):** the schema-unknown lazy view — arena-backed tape; navigation is total and
  Missing-propagating (`get`/`at` always return a doc; absence surfaces once as `None` from a leaf
  `as_*`); objects-as-data via ordered `key(i)`+`at(i)`; `elems()` materializes a level for
  pipelines (no map type, no serde-style value tree).
- **`json.scan` (J5):** streaming typed rows, binding-annotation-typed, pipeline source only.

Settled out (deleted from the catalog, not pending): `json.validate<T>` (decode-and-discard is
validation), `json.token` (doc + scan cover it; no consumer), `json.field_table<T>`
(compiler-internal). `json.decode<T>(...)` call syntax stays permanently out (no turbofish).

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
- P5 — **the decode target's field schema must feed the codegen cache key.** A decode target
  struct's field names/types feed only the codegen descriptor table, not the surrounding MIR — a
  field RENAME at the same slot (or a NESTED struct's field change) leaves every other MIR statement
  byte-identical, so without a schema fingerprint the unit's `impl_hash` would be unchanged and the
  warm cache would serve a STALE object still decoding the OLD key (reproduced end-to-end; the
  #514/#517 stale-cache class). The `JsonDecode*` MIR rvalues bake a recursive `json_schema_sig`
  (names + types + `layout(C)`/`align`, nested expanded) that is printed into the MIR — pinned by
  `cache_codegen.rs` gate 2b. Any new schema-carrying decode surface must do the same.

## Test anchors

`m5.rs` (decode matrix: struct/arrays/str-fields/order/unknown-keys/malformed/range #295 #311;
encode escaping; duplicate-key #306; **nested** decode+encode round-trip
`json_decode_encode_nested_struct_roundtrip` + Mison-path `json_decode_nested_struct_array_mison`),
`soa.rs:317` (json→soa filtered aggregate), `cache_codegen.rs` gate 2b (schema-fingerprint cache
invalidation, flat + nested), runtime `json_decode_nested_struct_single` / `..._array_mison`
(descriptor-level slow + Mison recursion), examples `json.align`, `json_decode.align`,
`json_nested.align`, `soa_json_str.align`; benches `bench/json_decode`, `bench/json_soa` (+ their
READMEs for the measured model).
