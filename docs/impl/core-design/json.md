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
//                           value's shape class — str/number/bool/object/array; O(1) first-byte dispatch;
//                           str payloads borrow the input; an owned array<Struct> variant is J2b)
```

**Union (sum-type) targets (JSON completeness J1b).** A JSON `oneOf` maps to a sum type
discriminated by the value's **shape class** — `Str` (`"`) / `Number` (digit/`-`) / `Bool` (`t`/`f`)
/ `Object` (`{`) / `Array` (`[`) — an O(1) dispatch on the first structural byte. **Compile-checked
(the Align move):** a union-decodable enum has every variant carry exactly one payload, each payload
mapping to one shape class, all classes **pairwise distinct** — `i64 | f64` (both Number), two object
payloads, or two array payloads are a compile error naming the clash; a tag-only or no-shape (`char`)
payload is rejected too. `null` is not a class (absence belongs to `Option`); a runtime value whose
shape no variant claims (e.g. an array in a union with no array variant, or `null`) is a decode `Err`.
Encode writes the live variant's payload **bare** (no wrapper key), so `decode(encode(x))` round-trips
by construction. Runtime: a `JsonUnion` descriptor (one `JsonField` payload arm per variant + a
shape-class→arm table + an arm→enum-tag table); decode classifies the first byte, writes the payload
via the shared `write_value`, and sets the tag; encode reads the tag and emits the live arm via the
shared `json_encode_value`. **Owned `array<Struct>` payload (J2b, SHIPPED — the OpenAI multimodal
`content: str | array<Part>` union):** a `[` dispatches to the Array-class arm (descriptor kind 5, the
element struct's sub-schema), decoding into an owned AoS the enum's tag-switched `Drop` frees; encode
writes it as a bare JSON array. The full `Content { Text(str), Parts(array<Part>) }` round-trips. The
element struct must be non-owned (Slice-C rule; `array<string>` / `array<Move-struct>` deferred), and
an `array<scalar>` union payload has no descriptor arm yet (J3). `json.encode` of a top-level union
needs a local binding (like struct encode). **Union as a struct field (J1b-2b / J3a, SHIPPED):** a struct field may be a union
(`Message { content: Content }`) — a descriptor **kind 6** whose `sub` is the `JsonUnion` (reused for
both decode and encode); `field_width`/`write_value` (all decode paths — slow + Mison speculative +
fallback) and `json_encode_value` grow a kind-6 arm, so a union field composes with nested structs,
`Option` fields (trailing-comma layout), and `array<Struct>` fields. **J3a** extends this to a **Move**
union field — the full multimodal `content: str | array<Part>` (`Content { Text(str), Parts(array<Part>) }`)
composes into `Message`, decoding/encoding both shapes and round-tripping byte-identically. A Move-enum
field makes the enclosing struct **Move**: `struct_is_move`/`ty_owns_buffer_rec` became enum-aware (a
`Ty::Enum` arm consulting `enum_is_move`, threaded through every Move-ness caller in lockstep), and
`drop_struct_fields`'s `Ty::Enum` arm frees the live variant via the tag-switched `drop_enum`; the
runtime `drop_decoded_owned` grew a **kind-6** arm (`→ drop_decoded_union`) to free the union's owned
payload on the decode error path. `match m.content { … }` moves the owned payload out and zeroes the
field (`NullStructField` became type-aware — the whole `{tag,payloads}` aggregate), so the struct's
`Drop` frees null there (single-free). The union's variants are expanded into the enclosing struct's
`json_union_schema_sig` so a variant change invalidates the decode/encode cache. **Boundary:** because a
Move struct cannot be a `Result`/`Option` Ok payload across a function boundary (Slice-C constraint), a
`Message` decode target binds with `?`; and `Chat { messages: array<Message> }` where `Message` is Move
is an `array<Move-struct>` field, rejected until J3b's owned-element deep free (a non-Move-`Message`
`array<Message>` — a union with only str/scalar/object variants — still round-trips).

**`array<Struct>` fields (REST-gateway runway, Slice C).** A struct field may be an owned
`array<Struct>` — the `messages: array<Message>` / `choices: array<Choice>` shape; the full OpenAI
request/response now round-trips. Decode: a descriptor kind 5 (`sub` = element schema) drives
`decode_struct_array_value`, which parses the JSON sub-array into an owned AoS (`parse_object` per
element, so nested/`Option` element fields recurse) and writes `{ptr,len}` to the field; the field
buffer is freed by the struct's `Drop`. Encode: a `StructArrayField` piece calls the runtime
descriptor-driven encoder (`json_encode_struct_array` → `json_encode_object`, **reusing the decode
descriptors** — symmetric, handles nested/Option/str/scalar). **Memory-safety:** on a decode `Err`
after an array field allocated, `drop_decoded_owned` frees the partial struct's AoS buffers (the
runtime dual of codegen `drop_struct_fields`). **`array<Move-struct>` elements (J3b, SHIPPED):** the
element may now itself be **Move** — the `Chat { messages: array<Message> }` shape, each `Message`
owning a Move-enum `content` field. Drop is a **deep** free: a shared codegen `deep_free_struct_array`
helper loops the `len` elements, recursively `drop_struct_fields` each (freeing its `string`/owned-array/
Move-enum field), then frees the AoS — called from both the struct-field drop AND a standalone
`array<Struct>` local's `Stmt::Drop`. The runtime error path mirrors it: `drop_decoded_owned`'s kind-5
arm deep-frees each element (gated by `sub_owns_buffers`), and `decode_struct_array_value` frees the
elements already materialized in `buf[0..count]` on a mid-array parse failure. **With J3b the OpenAI
chat gateway closes end-to-end** (`Chat` round-trips byte-identically). **Still rejected:**
`array<string>` (a bare-`string`-element array field — its per-element string free is a separate slice,
caught at 0b-2). **Constraint:** a Move struct (owns an array/Move-enum) can't be a `Result`/`Option`
Ok payload across a function boundary — decode + use in-scope; `json.encode` of a bare
`array<Move-struct>` and pipelines over such a field stay restricted (decode→encode passthrough works).
Deferred: `array<scalar>` field decode.

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
  over str/number/bool/object payloads (J1b-2a); union as a struct field (J1b-2b); enum owned
  `array<Struct>` payloads + tag-switched drop (J2a); the union Array shape-class arm (J2b); the
  multimodal union as a **Move-enum struct field** (`Message { content: Content }`, J3a) — all
  documented above. Plus `array<Move-struct>` struct fields — the owned-element deep free (J3b) —
  which closes `Chat { messages: array<Message> }`. **The OpenAI chat gateway now closes end-to-end.**
- **Matrix fill (J3):** top-level scalar/bool decode targets, `array<scalar>` fields,
  `Option<struct>` encode, supported-constructor compositions.
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
