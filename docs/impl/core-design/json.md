This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, Move/effect classification, error policy, pitfalls, test
anchors). Authored by the main loop (Fable).

# core — json

> 🌐 **English** · [Japanese](./ja/json.md)

## Overview

JSON across typed and schema-unknown boundaries (draft §14). The surface has five operations:
typed `encode` / `encode_bounded` / `decode`, lazy schema-unknown `doc`, and streaming typed-row `scan`. Target and row
types are carried by **inference, never a written type argument** (settled: Align has no
expression-position type-argument syntax / no turbofish). Requires `import core.json` (the
capability-header rule applies to core.json exactly like std modules).

## Escaped strings (Request 7)

Typed JSON accepts the RFC 8259 string grammar on every string token: declared and undeclared
keys, declared and undeclared values, nested values, union payloads, and `json.doc`. The accepted
escape set is `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, and `\uXXXX`; `\uXXXX` requires
valid surrogate pairing and produces UTF-8 semantic bytes. Raw C0 bytes, malformed/truncated
escapes, lone or reversed surrogates, and invalid UTF-8 return `Error.Code(1)` (or `Err` from
`json.doc`). A valid escaped `\u0000` is an embedded NUL; native-boundary validators retain
responsibility for rejecting it where required.

Clean strings remain zero-copy views into the input. A selected escaped string is materialized
exactly once in the caller's enclosing arena; its bytes are arena-owned and the enclosing decoded
value is region-bounded by both input and arena. Outside an arena, a selected escaped string is a
decode error, while clean strings retain the existing input-view behavior. Ignored escaped strings
and keys are validated and discarded without proportional scratch allocation. `json.scan` has no
arena operand, so an escaped declared string remains rejected there; `json.doc` already owns an
arena and materializes escaped `as_str`/`key` results there.

The runtime ABI passes the nullable arena handle as the final argument of the three materializing
typed entrypoints; `null` means clean-view-only mode. The descriptor layouts (`JsonField`,
`JsonSubTable`, and `JsonUnion`) do not change:

| Entry point | Final ABI argument | Escape behavior |
| --- | --- | --- |
| `align_rt_json_decode` | `arena: *mut Arena` | record and nested fields materialize in `arena` |
| `align_rt_json_decode_struct_array` | `arena: *mut Arena` | every AoS row shares the caller arena |
| `align_rt_json_decode_union` | `arena: *mut Arena` | string and object arms share the caller arena |
| `align_rt_json_decode_soa` | existing `arena: *mut Arena` | unchanged ABI; escaped columns materialize in the arena |
| `align_rt_json_scan_next` | none | escaped declared strings reject; no hidden allocation |

The arena allocation is exact-size, bump-only, and never individually freed. A semantic failure may
leave unreachable bytes until arena exit, but publishes no partial result and preserves the first
parser error. No hidden arena, process-global decoder state, descriptor field, persisted format, or
second JSON representation is introduced.

The canonical design fixture is
`bench/json_escape/fixtures/canonical.json`, SHA-256
`57fab88300c5522cd49dae7bafe7f90c29e077148cbd50ab6079e70446186321` including its final LF. It
contains an escaped declared key, short and Unicode escapes, a surrogate pair, an escaped ignored
key/value, a clean value, and a scalar field. The implementation must preserve the semantic record
and exact error precedence for this fixture and its malformed mutations.

### Request 7 implementation closure matrix

| Transition | Owner | Required regression |
| --- | --- | --- |
| String grammar, UTF-8, short escapes, Unicode/surrogates, and deterministic error order | shared runtime string-token decoder and `json.doc` parser | `align_runtime::tests::json_escape_string_grammar_matrix`, `align_runtime::tests::json_doc_top_level_scalar_and_escapes` |
| Clean view versus one arena materialization, outside-arena rejection, embedded NUL | `JsonParser`, arena writer, `align_rt_json_decode` | `align_runtime::tests::json_escape_record_lifecycle`, `m5::json_escape_typed_decode_materialization_and_region`, `m5::json_scalar_array_str_element_materializes_and_is_region_bound` |
| Escaped semantic keys, duplicate detection, ignored-key/value validation | semantic key matcher, `parse_object`, structural prevalidation | `align_runtime::tests::json_escape_aos_path_equivalence`, `align_runtime::tests::json_escape_nonmaterializing_paths`, `align_runtime::tests::json_escape_string_grammar_matrix` |
| Slow record and nested-record success/failure, return and Drop | `parse_object`, `write_value`, decoded-owner cleanup from Request 15 | `m5::json_decode_encode_nested_struct_roundtrip`, `m5::json_option_move_struct_later_failure_cleans`, `align_runtime::tests::json_array_field_error_path_frees_buffer` |
| AoS slow/speculative/fallback equivalence and row cleanup | `json_speculate`, `json_fallback`, `align_rt_json_decode_struct_array` | `align_runtime::tests::json_escape_aos_path_equivalence`, `m5::json_decode_struct_array_malformed_errors`, `align_runtime::tests::json_array_field_error_path_frees_buffer` |
| SoA direct fill and arena cleanup | `align_rt_json_decode_soa`, `SoaDst` | `align_runtime::tests::json_escape_soa_path_equivalence` |
| Union and scanner materialization boundaries | `decode_union_value`, `align_rt_json_scan_next` | `align_runtime::tests::json_escape_nonmaterializing_paths`, `m5::json_union_decode_by_shape_class`, `m5::json_scan_malformed_row_errors` |
| HIR/MIR arena operand, region meet, descriptor/cache identity, and ABI | sema storage/region analysis, MIR Rvalues, LLVM runtime registry | `m5::json_escape_typed_decode_materialization_and_region`, `cache_codegen::gate2b_json_decode_field_rename_invalidates`, `align_codegen_llvm::runtime_abi_extern_type_matrix_is_exact_for_every_row_and_ordinal` |
| Canonical fixture identity | checked-in fixture and runtime oracle | `align_runtime::tests::json_escape_record_lifecycle` includes the fixture bytes and semantic-output oracle; SHA-256 is recorded above |

The author-side matrix-to-diff pass must point every applicable row to implementation and a focused
owner test before the implementation PR is opened. The benchmark-evidence document remains the
separate trusted measurement boundary; it does not define this language or runtime contract.

## Direct owned records (Request 9)

Request 9 extends the existing inferred operations with one closed, flat owned-record graph. The
implementation ships. A direct record selects the owned route when it has
at least one direct `string`, `Option<string>`, or `array<string>` field. Once selected, every other
field must be a required signed/unsigned 8/16/32/64-bit integer or `bool`. A `str`, `array<str>`,
float, char, nested record/array/enum, other `Option`, explicit `layout(C)`, or `align(N)` makes the
complete owned graph reject before descriptor construction or runtime allocation. Records without
an owned text leaf keep the existing JSON route unchanged.

The three operations share one accepted graph:

```text
json.decode(input: str) -> Result<T, Error>
json.encode(value: T) -> str
json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>
```

Owned decode is an ownership-directed materializer. Every text field is a free-standing owner;
`array<string>` owns its spine and every element; `Option<string>` owns only `Some`. The result has
no input or arena provenance and remains free-standing when decoded inside `arena {}`. The owned
target types and `json.decode` make allocation explicit. Borrowed JSON keeps Request 7's input/arena
behavior, and a mixed owned/borrowed graph is rejected rather than silently cloned.

Recoverable parse, duplicate, shape, integer-range, missing-field, and trailing-input failures drop
every initialized direct owner exactly once before returning `Error.Code(1)`. Cleanup visits fields
in declaration order, initialized text-array elements in ascending index order, then the spine.
Overflow and allocator failure retain the runtime-wide terminal-abort policy. `u64` accepts and
encodes the complete `0..=u64::MAX` range: typed integer holes select the dedicated unsigned
builder ABI without a signed intermediate. Missing and `null` map to
`None`; `None` is omitted, while `Some("")` is distinct.

The checked compiler-private `OwnedJsonDescV1` is structural and target-local. It fixes field names
and declaration order, integer width/sign, natural-layout algorithm and offsets, optional tag and
payload offsets, allocation/drop tags, and the `array<string>` element Drop-plan version. It is
never serialized naked: a per-unit interface binds it to the canonical LLVM target triple, object
format, and exact relevant ABI cells in `OwnedJsonInterfaceEnvelopeV1` before including it in cache
identity. Interface format 7 owns the new descriptor list and rejects format 6 before parsing it.
The envelope rejects a target/ABI mismatch before trusting descriptor offsets and is not
a public artifact or reflection surface. Existing AoS, SoA, union, fixed-array, scalar-array, `json.doc`, and
recursively-Copy `json.scan` routes are unchanged.

The exact public ledger, descriptor bytes, error precedence, implementation closure matrix, and
golden vectors are authoritative in [`../24-owned-json-plan.md`](../24-owned-json-plan.md).

## Recursive owned records (Request 13, design accepted)

Request 13 replaces the flat owned implementation boundary with one acyclic, view-free graph while
keeping the same three inferred operations. A transitive owned `string` selects the route. The
complete graph admits fixed-width signed/unsigned integers, bool, owned `string`, nonempty
natural-layout records, `Option<T>` payloads, and dynamic arrays whose elements are an integer, bool,
string, or accepted record. An option payload cannot itself be an option because missing and `null`
are one absence state. Record/option/dynamic-array constructor depth is at most 128. Arrays of options/arrays, borrowed
text, floats, char, enums, fixed arrays, explicit layout/alignment, and every other constructor
reject before descriptor construction or allocation. A root with no transitive owned string keeps
the existing route and is not narrowed.

Decode makes every reachable owner free-standing, including inside `arena {}`. Recursive failure
cleanup visits live fields in declaration order, option payloads only after `Some`, array elements
in ascending order, then array spines. Encode and bounded encode borrow the same root and use one
descriptor-driven declaration-order writer; bounded success is byte-identical and retains Request
12's inclusive limit behavior.

`OwnedJsonGraphDescV2` and `OwnedJsonInterfaceEnvelopeV2` replace the flat V1 records for every
owned root. Interface format 8 replaces format 7 atomically; there is no compatibility decoder or
parallel runtime route. A103 decode and A80 encode keep their ABIs. The accepted grammar, exact V2
bytes, validation order, C6 fixture scope, and implementation matrix are authoritative in
[`../25-recursive-owned-json-plan.md`](../25-recursive-owned-json-plan.md). Implementation is
pending; the preceding Request 9 section remains the current shipped compiler behavior.

## Signatures (verified unless marked pending)

```text
json.encode(x)   -> str                      // x: struct (nested structs recurse); str fields JSON-escaped
json.encode_bounded(x, max_bytes: i64) -> Result<string, Error>
json.decode(s)   -> Result<T, Error>         // T from the binding/context: u: User := json.decode(s)?

// decode targets, all verified:
//   i64 / f64 / bool       (a BARE scalar — parses the whole input as one JSON number/bool; Copy → Static/returnable; T1b)
//   struct                 (flat OR with nested-struct / Option<T> / array<Struct> / array<scalar> fields; field order free; unknown keys ignored)
//   array<i64> / array<f64>
//   array<Struct>          (AoS; clean str fields = zero-copy views into the input; escaped selected fields use the caller arena; nested-struct + Option fields recurse)
//   soa<Struct>            (direct columnar decode — no AoS intermediate, no transpose;
//                           inside arena {}; clean str columns borrow the input text; escaped selected columns use the caller arena; primitive/str columns only,
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
field makes the enclosing struct **Move**: the canonical recursive `DropPlan` sees the enum payload
and `struct_is_move`/`enum_is_move` derive from it in lockstep, and
`drop_struct_fields`'s `Ty::Enum` arm frees the live variant via the tag-switched `drop_enum`; the
runtime `drop_decoded_owned` grew a **kind-6** arm (`→ drop_decoded_union`) to free the union's owned
payload on the decode error path. `match m.content { … }` moves the owned payload out and zeroes the
field (`NullStructField` became type-aware — the whole `{tag,payloads}` aggregate), so the struct's
`Drop` frees null there (single-free). The union's variants are part of the structural MIR type
tables, so a variant change invalidates the decode/encode cache. Finite non-recursive Move structs
and unions use the shipped recursive tagged `DropPlan`, so raw `Result`/`Option` payloads may be
bound, passed, returned, and transferred through ordinary control-flow ownership paths. The
following J3b slice supplies owned-element deep free, so
`Chat { messages: array<Message> }` also round-trips when `Message` is Move.

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
chat gateway closes end-to-end** (`Chat` round-trips byte-identically). The borrowed/nested/AoS
routes still reject a bare-`string`-element array field. Request 10 makes that field valid for
ordinary owned record construction by reusing the standalone deep Drop; Request 9 admits it only
through the closed direct-owned flat-record JSON route.
`json.encode` of a bare `array<Move-struct>` and pipelines over such a field stay restricted
(decode→encode passthrough works).

**`array<scalar>` fields (JSON completeness T1b + `array<str>`, align-llm Request 3).** A struct field
may be an owned `array<i64>` / `array<f64>` / `array<bool>` (the align-LLM data shapes — embeddings,
token ids) **or `array<str>`** (argv lists, `stop`/`tags`, tool-name lists). A JSON descriptor
**kind 7**: the field's own `{ptr,len}` slot is width 16 (low byte); the ELEMENT scalar's kind (0=int /
1=bool / 2=float / **3=str**, bits 20-23), width (bits **24-28** — 5 bits, because a `str` element's
`{ptr,len}` is width 16, which does not fit the original 4) and sign (bit 16) pack into the tag's upper
bits, so one tag carries both. Decode: `decode_scalar_array_value` parses the JSON array into an owned
buffer via the shared per-scalar `write_value`, so the same range / sign / float-width checks a scalar
*field* gets apply per element. **A clean `str` element (kind 3, width 16) is written as a zero-copy
`{ptr,len}` VIEW into the input** (the top-level `str`-field rule); selected escaped elements
materialize exactly once in the enclosing arena. The owned spine therefore borrows the input or
arena, and the whole decoded struct is input/arena-region-bound. `.clone()` copies past those
sources; without an arena an escaped element returns `Err`, so
the decoded record remains input/arena-region-bound. Encode: a
`ScalarArrayField` template piece → `json_encode_scalar_array` loops the buffer emitting `[e0,e1,…]`
(a `str` element renders quoted + `write_json_str_body`-escaped). Drop: the owned SPINE flat-frees
(scalar / `str`-view elements own nothing — the views are freed by no one, they borrow the input) —
`drop_struct_fields`'s `DynArray` arm on success, `drop_decoded_owned` kind-7 (element-agnostic
flat-free) on the decode error path (`sub_owns_buffers` has kind 7 so a scalar/str-array field inside an
`array<Move-struct>` element is cleaned up). Composes with J3b (`Table { rows: array<Row>, meta: array<i64> }`
where `Row { vals: array<f64> }`). The structural MIR fingerprint includes the element type, so an
`array<i64>`→`array<f64>` change invalidates the cache. **Still deferred:** `array<char>` (no JSON
form); a **top-level** `array<str> := json.decode`
(a struct FIELD rides the enclosing struct's input-region binding, but a top-level array result would
have to carry that region itself — the scalar top-level array is deliberately `Static`/returnable, so
`array<str>` at top level is a separate region-carrying slice). v1 limits: `.sum()`/pipelines over an
owned scalar-array field and `json.encode` of a bare `array<scalar>` stay restricted (decode + `.len()`
+ encode-as-field work).

**`Option<T>` fields (REST-gateway runway, Slice B).** A struct field may be an `Option<T>` (payload
scalar / `str` / nested struct). **Null policy:** decode maps a missing key → `None`, JSON `null` →
`None`, a type mismatch → `Err`; a required (non-`Option`) field still `Err`s when missing. **Encode
omits a `None` field entirely** (never `"k":null`), so `decode(encode(x))` round-trips. Runtime: the
`JsonField` descriptor gains `opt_tag` (`-1` = required, else the `Option` tag byte offset); an
optional field is exempt from `all_required_seen`, and the shared `write_value` writes the payload at
the payload slot then sets the `Some` tag. Encode switches an `Option`-bearing object to a
trailing-comma layout with one `align_rt_builder_pop_comma` before `}` (a pure-required object keeps
the static layout). **JSON ownership boundary:** L1a permits owned `Option<T>` fields in ordinary
language structs, but each JSON path may still have a narrower descriptor contract. The current
compiler's Decode schema admits an `Option<Move-struct>` shape; ordinary decode constructs it and
ordinary encode plus scope Drop preserve that admitted shape. A known partial-error cleanup defect
remains a separate ownership request: if a later required sibling fails after a `Some` payload has
been decoded, the decoded optional owner must still be released. `Option<string>` remains outside
the current JSON Decode schema. These ordinary JSON details do not weaken the scanner rule below:
a row whose reachable graph needs `Drop` is rejected by `json.scan`. **`Option<struct>` encode
(T1b, SHIPPED):** `Some` renders the nested object via the runtime descriptor-driven encoder (a new
`OptionStructField` template piece → `align_rt_json_encode_object`, a single struct by its descriptor
table), `None` omits the field (the same trailing-comma + `PopComma` scheme); it composes recursively
(a payload with a nested plain struct + a nested `Option<str>` omits its own `None`s). The payload
struct is validated encodable (`decode_struct_fields_ok`), including the currently admitted
`Option<Move-struct>` shape. The scanner-only Copy restriction does not narrow ordinary JSON
decode, encode, or scope Drop.
The structural MIR fingerprint includes the payload struct definition, so an `Option<struct>` payload
field change invalidates both decode and encode objects. JSON MIR nodes carry only target ids; no
manually threaded schema string exists.

**Nested-struct fields (REST-gateway runway, Slice A).** A struct field may itself be a `Struct`;
`decode` recurses into the nested object and `encode` renders it back, so a nested record round-trips.
Runtime: the field descriptor carries kind 4 with a `JsonSubTable` pointer (the nested struct's own
descriptors + PHF + store size), and `parse_object` / `write_field_indexed` recurse — so BOTH the slow
path and the Mison speculative path handle nesting (a nested field is one record-level colon whose
value the record-splitter leaves at a deeper bracket depth). Nested `str` fields stay zero-copy views
into the input, so the whole value is region-tied to it recursively (`struct_has_str` recurses).
The later Option, array-field, and union slices described above compose with this recursive path.

## Type & ownership classification

- `encode` builds through the string builder; result is an arena-regioned `str`.
- `encode_bounded` borrows the same accepted value graph and uses the same ordered encode pieces,
  but returns one individually owned `string` on exact-fit-or-smaller success. Its inclusive
  `max_bytes` ceiling applies to emitted UTF-8 bytes before growth; negative or exceeded limits are
  `Error.Invalid`, with no partial result. The shipped operation adds no shape by itself. The
  accepted Request 13 implementation replaces the flat owned parts with one V2 descriptor-driven
  root writer shared by both encode operations.
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
#295; `u64` fields accept the full `u64` range through one write dispatcher, #311). A declared
field must appear exactly once: a duplicate declared key is an `Err` on both the strict and
speculative paths, including at a position the learned pattern considered unqueried. Undeclared
keys are skipped.

`encode_bounded` is the fallible resource-boundary sibling of `encode`: a negative limit or the
first emitted byte beyond the inclusive ceiling is `Err(Error.Invalid)`. An allocator failure keeps
the language-wide terminal-abort policy. Successful bytes are byte-identical to `encode`, including
declaration-order keys, numeric spelling, escaping, omitted `None`, arrays, and unions; “canonical”
does not mean RFC 8785 sorting. The authoritative contract and closure matrix are in
`../17-library-boundary-prerequisites.md` §7.7.

## Regions

`region_of(clean decoded str view) = region_of(input)`; an escaped selected string is bounded by
the input and enclosing arena; `region_of(soa columns) = enclosing arena`; owned arrays escape
freely only when all their elements are owned. Escaping a decoded view past its input or arena is
caught at the escape point (clone out to keep).

## Completeness status and remaining boundaries

The full design lives in `open-questions.md` → "JSON completeness — DESIGN SETTLED" (the
implementation record; spec text in draft §14 + §18.1). The ledger below records what shipped and
the few boundaries that remain:

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
- **Matrix fill (J3/T1b): COMPLETE.** ~~top-level scalar/bool decode targets~~ (SHIPPED),
  ~~`array<scalar>` fields~~ (SHIPPED), ~~`Option<struct>` encode~~ (SHIPPED). `array<Option<T>>` is
  **DEFERRED** — an owned array of a composite element is un-representable in the non-recursive
  `Scalar`/`PrimScalar` type system (a language-type gap needing a dedicated composite-element array
  type, not a JSON matrix-fill; low value). See open-questions "T1b".
- **`json.doc` (J4):** the schema-unknown lazy view — arena-backed tape; navigation is total and
  Missing-propagating (`get`/`at` always return a doc; absence surfaces once as `None` from a leaf
  `as_*`); objects-as-data via ordered `key(i)`+`at(i)`; `elems()` materializes a level for
  pipelines (no map type, no serde-style value tree). **Slice 1 SHIPPED:** the `json.doc` type +
  `json.doc(s)?` parse (arena-backed tape, `Result<json.doc, Error>`) + `kind()` (→ builtin
  `json.kind` sum type) + `get`/`at` navigation + the four leaf accessors `as_str`/`as_i64`/`as_f64`/
  `as_bool` (→ `Option`; `as_str` is a zero-copy input view, escaped strings unescape into the arena).
  A number's **form** selects the accessor (`42.0` / `1e3` are integer-valued but non-integer form →
  `as_i64` `None`, `as_f64` `Some`), matching simdjson's on-demand model. `get` on a **duplicate** key
  returns the **first** occurrence (lazy-view semantics; deliberately distinct from typed `decode`,
  which rejects a duplicate declared key). **Slice 2 SHIPPED:** `d.len()` (member/element count, 0 on a
  non-container) + `d.key(i) -> Option<str>` (the i-th object key in document order — objects-as-ordered
  data). Together with `at(i)`, these drive iteration over a doc array by recursion (no `loop` needed).
  The types `json.doc` and `json.kind` are now **nameable** (a `fn f(d: json.doc)` helper / a
  `k: json.kind` binding resolve directly — the two builtin `core.json` type names). **Slice 3 SHIPPED —
  J4 COMPLETE:** `d.elems() -> slice<json.doc>` materializes one level (each Array element, or each
  Object member **value** — keys via `key(i)`) as an arena-backed `slice<json.doc>` **once** (O(n), then
  O(1) indexing — vs `at(i)`'s O(i) re-walk). It reuses the existing `slice` machinery:
  `slice<json.doc>` = `Ty::Slice(Scalar::JsonDoc)` (already representable — no new array type), so
  `.len()` and `xs[i] -> json.doc` (region-bound to the slice, a Copy 16-byte handle → no double-free)
  work out of the box, and `slice<json.doc>` is nameable as a param type, so `fn f(xs: slice<json.doc>)`
  walks a level by recursion. The slice buffer is bump-allocated in the enclosing arena (needs
  `arena {}`), region-tied to min(input, arena). Full `.map`/`.where` **pipeline fusion** over a
  `slice<json.doc>` (closures taking `json.doc`) is the natural next step but not required — index + len
  + recursion cover level iteration today.
  **Known systemic leniency (not a J4 regression — shared with `decode`'s scanner):** raw C0 control
  bytes inside strings and leading zeros in numbers (`007`) are currently accepted; making the shared
  `find_quote_or_escape` / `number_span` strict (RFC 8259 §7/§6) is a follow-up that must land for
  `decode` and `doc` together (fixing only one would make `json.doc(s)` and `json.decode(s)` disagree
  on the same malformed `s`).
- **`json.scan` (J5):** streaming typed rows, binding-annotation-typed, pipeline source only.
  **Slice 1 SHIPPED:** the `json.scanner<Row>` type (a Copy `{ptr,len}` input view, region-tracked —
  it borrows the input, never materializes an `array<Row>`) + `json.scan(view)` (row type from the
  binding annotation `rows: json.scanner<Row> := json.scan(view)`, exactly like `decode`; no arena
  needed — the row decodes into a per-step stack slot and its `str` fields borrow the input) + the
  streaming fused reducers `.sum()` / `.count()` → **`Result<T, Error>`** (a malformed row surfaces
  once as `Err`; unwrap with `?`). Stages: `.field` projection, `.where(.field)`, `.where(pred)`,
  `.map(f)` — the full stage set, driven per row by [`lower_json_scan_reduce`], not
  `lower_array_reduce`'s counted loop. One scanner handles **both** a top-level JSON array and NDJSON
  (the runtime `align_rt_json_scan_next` treats a leading `[`, inter-value `,`, and whitespace/newlines
  as separators, `]`/EOF as terminators; reuses the struct decode descriptor per row). A materializing
  terminal (`.to_array()` / `.sort()` / `group_by`) over a stream is rejected in sema (a clean
  diagnostic, not mis-lowered). **Slice 2 SHIPPED:** the rest of the streaming reducer family over a
  scanner — `.reduce(init, f)` / `.any(p)` / `.all(p)` / `.min()` / `.max()` — each `Result<T, Error>`,
  sharing `lower_json_scan_reduce`'s guarded per-row fold. So the complete streaming reducer set is
  `sum` / `count` / `reduce` / `any` / `all` / `min` / `max`; only materializing terminals stay out (by
  design — they would defeat streaming).

  **J5 safety boundary — Request 6 design gate (implementation pending).** A scanner reuses one
  row slot for every input value and has no per-row arena or `Drop` transition. Therefore the row
  schema must be recursively **Copy**: `json.scan` accepts a row only when the canonical recursive
  `DropPlan` reports no `Drop` for the complete reachable struct, option, and union graph. This is a
  scanner-only rule; the same declaration remains a valid ordinary Align type and remains eligible
  for every ordinary JSON path whose own schema contract admits it. Among rows that pass the existing
  JSON schema whitelist, the rule rejects direct or transitive `array<T>`, `array<Struct>`, or an
  owning option/union payload, rather than maintaining an ad-hoc list of array forms. A field shape
  that is not JSON-decode-eligible retains the existing schema diagnostic; the Copy diagnostic below
  is only the deterministic ownership error for a schema-admitted Move row.

  The source semantic check runs after the existing JSON decode-schema whitelist and before input-type
  checking, MIR construction, descriptor construction, or runtime calls. The universal expression
  `Span` check remains first after non-expression envelope fields. For a whole-program check, the
  active `align_mir::hir_program_is_valid` pre-lowering gate then rechecks the complete scanner
  envelope and applies the same pure row predicate to the checked HIR. `JsonScan` is the one
  explicit ordering exception to the general stored-field-before-`Span` rule. Its deterministic
  active-HIR order is: (1) validate `Expr.span`, (2) require `Expr.ty == Ty::JsonScanner(struct_id)`,
  (3) require the expression's `struct_id` to resolve to an existing row definition, (4) require
  `input.ty == Ty::Str`, (5) run the Decode-direction schema check, and (6) run the canonical
  recursive Copy check. The stored `struct_id` is already a typed `u32`; its semantic row lookup is
  step 3, not a separate raw-representation state. The gate rejects a failed envelope step before
  asking the descriptor walker to inspect the row graph. Thus a malformed span beats a wrong stored
  type, unknown row id, wrong input type, schema error, or Copy error. For
  imported or per-unit consumers, interface/import reconstruction first materializes the checked HIR;
  the active gate then applies this same explicit scanner order and row predicate to that reconstructed HIR
  before MIR lowering, descriptor construction, or runtime calls. The gate never reconstructs source
  spelling, and the dormant `align_sema::checked_hir_body_facts_are_valid` body replay is not a
  substitute. A schema-admitted rejected row reports the exact source-level diagnostic:

  The active-envelope precedence owner is
  `hir_program_json_scan_envelope_precedence_matrix`, which calls the crate-private reason-valued
  seam `align_mir::validate_hir::json_scan_validation_reason(&hir::Program) ->
  Result<(), JsonScanValidationReason>`. Production `hir_program_is_valid(&hir::Program) -> bool`
  remains the unchanged boolean caller and returns `reason.is_ok()`. The reason enum is an
  implementation-owned test seam, not a new user-facing diagnostic. Its paired-invalid cases are normative:
  malformed `Span` + wrong stored type reports `Span`; malformed `Span` + unknown row id reports
  `Span`; malformed `Span` + non-`str` input reports `Span`; malformed `Span` + schema-invalid row
  reports `Span`; malformed `Span` + Move/Copy failure reports `Span`; with a valid `Span`, wrong
  stored type precedes unknown row id, which precedes non-`str` input, which precedes schema, which
  precedes Copy. The reason variants are `InvalidSpan`, `StoredType`, `UnknownRow`, `InputType`,
  `Schema`, and `Copy`, in that order after the enclosing variant has matched.

  ```text
  `json.scan` row type '<row-type-source-spelling>' must be Copy; Move rows need per-row Drop before the scanner can reuse its row slot
  ```

  `<row-type-source-spelling>` is the declared public spelling, including a module qualifier and
  concrete generic arguments; internal `$`-mangled names and monomorph interner names never appear.
  `check_json_scan` must receive this spelling from the producer-owned source-type annotation or
  source-type formatter before the AST spelling is erased into `Ty`. The formatter resolves the
  local/imported path and concrete generic arguments through the same module/type-resolution tables
  that produced the expected row type; it must not use `ty_name`, `StructDef::name`,
  `StructDef::source_name`, or any internal mangled/interner spelling. The producer-owned spelling
  is part of the diagnostic contract, not runtime reflection or a cache/artifact read.
  Accepted rows retain the existing scanner handle, input region, framing, terminal `Result`, HIR,
  MIR, codegen, runtime entrypoint, and cache identity. The change is intentionally a compile-time
  rejection of an unsafe existing surface, not a per-row cleanup implementation.

  The implementation gate is the following contract ledger:

  | Surface | Contract |
  | --- | --- |
  | Public entrypoint | `rows: json.scanner<Row> := json.scan(view)`; the row type comes from the expected scanner annotation, not a written call-site type argument. The scanner is a pipeline source only. |
  | Input and result | `view` is the existing `str` input (or the existing explicit borrow from `string`); its region bounds the scanner. Five accepted HIR terminal variants (`ArraySum`, `ArrayCount`, `ArrayReduce`, `ArrayAnyAll`, and `ArrayMinMax`) expose all seven public methods (`sum`, `count`, `reduce`, `any`, `all`, `min`, and `max`); each supported terminal returns the existing `Result<T, Error>` scalar result and preserves malformed-row and exhaustion behavior. |
  | Compiler/runtime owner | `align_sema::Checker::check_json_scan` owns source validation and source spelling. `align_sema::Checker::check_generic_call` additionally owns the new expected-return propagation enabling rule described below; its numeric finalization keeps the existing `IntVar -> i64` and `FloatVar -> f64` defaults. For imported/per-unit consumers, interface/import reconstruction first materializes checked HIR; all four MIR lowerers call the private `align_mir::hir_program_is_valid(&hir::Program) -> bool`, whose active Request 6 exception calls the reason-valued `validate_hir::json_scan_validation_reason` in the explicit `Span`, type, row-id, input, schema, Copy order above and consumes `.is_ok()` before MIR/runtime construction. This is distinct from dormant `align_sema::checked_hir_body_facts_are_valid`; only pure row-predicate helpers may be shared. The existing MIR `JsonScan` lowering, LLVM emission, and `align_rt_json_scan_next` own accepted execution. The gate adds no runtime owner. |
  | Row eligibility | `json.scan` accepts only a recursively non-owning row whose canonical `DropPlan` is valid and needs no drop. |
  | Validation order | Source: capability import, arity, scanner annotation/inference, existing JSON schema, recursive Copy check, then input `str` typing and region checks. Active HIR replay uses the explicit `JsonScan` exception: `Expr.span`, `Expr.ty == Ty::JsonScanner(struct_id)`, existing row id, `input.ty == Ty::Str`, Decode schema, recursive Copy check; reject before any descriptor or MIR consumer. The reason-valued seam makes the winner testable while production lowering still consumes a boolean. |
  | Ownership | rejected rows construct no scanner, descriptor, row slot, allocation, or runtime side effect; accepted rows retain the existing borrowed input and Copy row slot. |
  | Diagnostic identity | use the producer-owned public local/imported/generic source spelling; never expose internal names or reconstruct spelling from HIR mangling. |
  | ABI and persistence | N/A: no source syntax, HIR/MIR node, descriptor, runtime ABI, wire format, or cache identity changes for accepted programs. |
  | Runtime cleanup | N/A for accepted rows because the complete row graph has no `Drop`; existing scanner input and scalar-accumulator cleanup remains authoritative. |
  | Compatibility prerequisite | The implementation PR is gated on this design and must retain the existing JSON schema and scanner terminal contracts. Request 6 align-llm adoption is a later consumer gate after the implementation release is pinned. |
  | Acceptance and benchmark | The owner tests, `scripts/compare-json-scan-identity.sh` cross-compiler identity probe, and the `json_scan_copy_row_no_owned_alloc` allocation probe below close the contract; performance is N/A and no benchmark improvement is claimed. |
  | Source-of-truth map | This English design, `docs/impl/core-design/ja/json.md`, `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, `docs/open-questions.md`, `docs/impl/17-library-boundary-prerequisites.md`, `docs/impl/19-hir-validation-ledger.md`, and the align-llm Request 6 register must agree. |
  | Concurrent scanners | N/A to the compile-time gate; independent accepted scanners retain their existing independent handles and slots. |
  | Performance | N/A: no performance claim and no production MIR, codegen, or runtime change. |

  **Generic inference boundary.** Request 6 covers only an ordinary generic call whose scanner row
  is concrete before call checking. The checker must seed a concrete expected return before checking
  any argument: it structurally matches the declared return against the expected type, binding
  generic slots while validating every concrete return leaf, then substitutes every bound parameter
  into the corresponding declared argument type. A concrete return-leaf mismatch is owned by the
  seed and stops argument checking. Each
  argument is checked with that substituted expected type in source order, so a nested
  `json.scan(view)` receives its concrete `json.scanner<Row>` context before its own source check.
  The expected-return seed is itself an inference boundary: if structural matching emits an error,
  the checker returns the existing error sentinel before checking any argument. The argument's
  actual type is then reconciled with the original declared parameter: when the substituted
  expected type is fully bound, that argument check owns the concrete mismatch and the inference
  pass binds only still-unbound parameters (it must not report the same conflict again in reverse
  order). Partial-substitution classification is over the callee's own inference slots, not raw
  `Ty::Param` numbers: a position is wholly unresolved when all of its callee slots are unbound,
  fully bound when all are bound, or partially substituted when both states occur. A bound slot may
  carry an enclosing generic function's symbolic parameter; that is valid generic forwarding and is
  not a partial state. Request 6 deliberately rejects a partially substituted composite such as
  `Result<T, U>` after only `T` was seeded; `Ty::Param` is not a wildcard for the ordinary expression
  checker, and accepting that state would either lose the constructor's expected context or allow a
  callee inference slot to escape into HIR. The source checker reports the exact deterministic
  diagnostic
  `generic argument {ordinal} of '<function>' has a partially inferred type; annotate the argument or use a bare generic parameter`
  before checking that argument. Arguments stop at the first new error in source order; the generic
  call returns the existing error sentinel and never publishes a partial call or scanner. Scanner
  spelling is producer-owned checker state: an annotated parameter wins first, then a spelling
  carried by an inferred scanner slot, then the active outer expected spelling. The slot spelling
  travels through inferred scanner-typed locals, annotated generic-call return results (including a
  producer with no scanner argument), transparent generic-call results, parameters, and
  lambda captures, so aliases and generic wrappers cannot erase the exact diagnostic identity. The
  checker derives it only across producer-owned local/block/borrow/call expression boundaries; it
  never enters HIR. After all arguments, the checker finalizes every bound
  parameter, rejects any unresolved bare parameter with the existing
  `cannot infer type parameter '<name>' of '<function>'; annotate the call's context`, constructs
  the concrete instantiation, and reruns the existing schema and Copy checks. A wrapper call and a
  multi-argument call use the same rule: expected context propagates through each bare return/argument
  boundary, and the first source-order conflict wins.

  The inference-state contract is explicit. Missing scanner context on a direct `json.scan` retains
  `cannot infer the scan row type; annotate the binding, e.g. \`rows: json.scanner<Row> := json.scan(d)\``;
  an unresolved bare parameter is a slot with no expected or argument binding and uses the generic
  inference diagnostic above; numeric `IntVar` and `FloatVar` slots are not ambiguous and retain the
  current finalizer's deterministic defaults to `i64` and `f64`; a conflicting slot reports the
  first source-order existing type mismatch. There is no new ambiguous diagnostic. The unresolved
  `json.scanner<Row<T>>` type argument is not an inference state that Request 6 adds. It remains a
  separate Align prerequisite and retains the exact resolver diagnostic
  `instantiating a generic struct with a type parameter ('Row<…>' inside a generic function) is not supported yet`.
  `m5::json_scan_generic_return_context_wrapper_matrix` owns wrapper propagation, including
  forwarding through distinct outer generic slot numbers, and
  `m5::json_scan_generic_return_context_argument_order_matrix` owns two-or-more argument source
  order, exact first-conflict publication, and the no-cascade rule.
  `m5::json_scan_generic_return_context_expected_conflict_no_cascade` owns the expected-return seed
  conflict boundary, `m5::json_scan_generic_return_context_expected_concrete_conflict_no_cascade`
  owns concrete return-leaf validation before arguments, and `m5::json_scan_generic_argument_source_spelling`
  owns annotated and inferred local aliases, annotated or inferred generic-call result propagation,
  and lambda capture spelling. The matrix also owns the exact Copy diagnostic through a bare
  generic wrapper, the partial-composite rejection above, and asserts
  that every failed state produces no `ExprKind::JsonScan` HIR node; the corresponding driver/cache
  owner snapshots the complete cache-owned tree (manifest, index, and CAS blobs) and asserts that no
  `PerUnitArtifact`, cache manifest, or cache blob is published.

  The generic inference closure has two additional explicit owners: concrete return-leaf mismatch
  is rejected by the expected-return seed before any scanner argument is checked, and an annotated
  scanner return spelling is retained through a generic call even when the call has no scanner
  argument. These are owned by `m5::json_scan_generic_return_context_expected_concrete_conflict_no_cascade`
  and the return-producer cases in `m5::json_scan_generic_argument_source_spelling`.

  **Ownership closure matrix (implementation gate).** The following cells are closed before the
  implementation PR starts; `N/A` is a consequence of the recursively Copy precondition, not an
  omitted decision.

  | Cell | Intended owner | Exact regression or benchmark |
  | --- | --- | --- |
  | Type formation, row validation, and scanner construction | `align_sema::Checker::check_json_scan`; no scanner node is produced until schema and Copy checks pass | `m5::json_scan_copy_row_terminal_matrix`, `m5::json_scan_rejects_owned_row_fields` |
  | Move-in, move-out, source nulling, replacement, and returned row ownership | N/A for an accepted row: `DropPlan` proves no Move field; the rejected path returns before construction | `m5::json_scan_copy_row_error_matrix`, `json_scan_copy_row_no_owned_alloc` |
  | `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early terminal return, and malformed input | Existing scanner MIR/runtime control flow; no new ownership edge is admitted by this gate | `m5::json_scan_copy_row_terminal_matrix`, `m5::json_scan_copy_row_error_matrix` |
  | Direct, nested, optional, union, and invalid/cyclic schema graph | Canonical recursive `DropPlan`/JSON schema producer tables; fail closed on missing or invalid graph nodes. The active pre-lowering gate must apply the same pure predicate after interface/import reconstruction. Its scanner envelope checks also fail closed on a mismatched expression type/id or non-`str` input. | `m5::json_scan_rejects_transitive_owned_row_fields`, `m5::json_scan_row_schema_matrix`, `hir_body_validator_json_scan_copy_row`, `hir_program_json_scan_copy_row`, `hir_program_json_scan_envelope_mismatch` |
  | Generic monomorphization, return-context inference, and imported source spelling | Request 6 covers ordinary generic function calls whose scanner row is already concrete before call checking, such as `identity<T>(value: T) -> T` called under an expected `json.scanner<Owned>` context. This is a new sema enabling rule owned by `align_sema::Checker::check_generic_call`: before any argument is checked, it seeds the expected return into the bare substitution, stops immediately if that seed emits an error, substitutes bound parameters into each declared argument type, and checks arguments in source order. A parameter position is classified using the callee's own inference slots: all unbound is wholly unresolved, all bound is fully substituted (including a valid enclosing generic parameter), and a mixture is rejected before its argument is checked with the exact diagnostic recorded above. The concrete instantiation reruns the existing Decode schema and canonical `DropPlan` checks. A concrete substituted argument owns its own mismatch; the inference pass only binds unbound parameters, and a later argument is not checked after the first new error. Producer-owned scanner spelling is carried alongside an inferred scanner slot through annotated or inferred locals, transparent generic-call results, parameters, and lambda captures; annotated parameter spelling wins over slot spelling, which wins over the active outer spelling. The checker may derive that spelling only by walking producer-owned local/block/borrow/call boundaries, and the spelling remains checker-only and never enters HIR. The existing finalizer still defaults numeric `IntVar`/`FloatVar` to `i64`/`f64`; only an unbound bare parameter is unresolved, and a conflicting candidate uses the first existing type-mismatch diagnostic. Wrapper propagation, expected-return conflict precedence, source spelling, inferred aliases/call results, and two-or-more argument source order are separate fixtures. It does not add `json.scanner<Row<T>>` with an unresolved row parameter: the current resolver's exact “generic type parameter inside a generic type argument is not supported yet” diagnostic remains the explicit deferred Align prerequisite. Missing scanner context uses the existing scan-inference diagnostic. A failed state produces no `ExprKind::JsonScan` HIR node and publishes no artifact. | `m5::json_scan_generic_row_ownership`, `m5::json_scan_generic_return_context_ownership`, `m5::json_scan_generic_return_context_wrapper_matrix`, `m5::json_scan_generic_return_context_argument_order_matrix`, `m5::json_scan_generic_return_context_expected_conflict_no_cascade`, `m5::json_scan_generic_argument_source_spelling`, `m5::json_scan_generic_return_context_numeric_default`, `m5::json_scan_generic_return_context_inference_matrix`, `m5::json_scan_generic_return_context_partial_composite_rejection`, `modules::json_scan_imported_row_ownership`, `modules::json_scan_imported_generic_return_context_ownership` |
  | Whole-program, per-unit, cold/hot cache, schema edit/revert | Existing structural MIR/cache identity remains owner; rejected rows publish no artifact. Per-unit fixtures must cover interface reconstruction, accepted Copy rows, rejected Move rows, and every failed generic inference state. Rejection snapshots cover every cache-owned file under `cas`, `actions`, and `index`, not only action manifests. | `cache_codegen::json_scan_row_schema_rejection`, `cache_codegen::json_scan_per_unit_interface_row_ownership`, `cache_codegen::json_scan_generic_return_context_no_publication`, accepted Copy-row MIR/raw-LLVM identity comparison |
  | Interface serialization and persisted/wire identity | Interface/import reconstruction is an input to checked HIR for imported/per-unit consumers; the active gate validates the reconstructed scanner envelope and row graph before MIR/runtime construction, while accepted source identity remains unchanged | `cargo test -p align_interface --test summary`, `modules::json_scan_imported_row_ownership`, `cache_codegen::json_scan_per_unit_interface_row_ownership` |
  | Runtime ownership provenance and allocation parity | Existing scanner input/accumulator owners; Copy rows allocate no owned row field. The exact composite fixture is `Leaf { score: i64, name: str }`, `CopyContent { Text(str), Count(i64), Flag(bool), Object(Leaf) }`, and `CopyRow { maybe_i64: Option<i64>, maybe_f64: Option<f64>, maybe_bool: Option<bool>, maybe_text: Option<str>, maybe_leaf: Option<Leaf>, leaf: Leaf, content: CopyContent, label: str }`. Its nonempty stream includes Some values for every optional field including `maybe_leaf`, explicit `null`, omitted optional fields, all `Text`/`Count`/`Flag`/`Object` arms, nested `Leaf`, and borrowed `label`; a second stream has a valid first row followed by malformed input. The LLVM allocation oracle requires `align_rt_json_scan_next` and no calls to `align_rt_alloc` or `align_rt_arena_alloc`. | `json_scan_copy_row_no_owned_alloc`, `json_scan_copy_row_copy_composites_no_owned_alloc`, `m5::json_scan_copy_composite_runtime_matrix` |
  | Exhaustion, empty input, malformed first/later row, and `Result`/`?` cleanup | Existing scanner input and accumulator cleanup; row-slot cleanup is N/A by construction. Non-scalar Copy option/union rows exercise nonempty and later-malformed streams. | `m5::json_scan_copy_row_error_matrix`, `m5::json_scan_copy_row_terminal_matrix`, `m5::json_scan_copy_composite_runtime_matrix` |
  | Concurrent independent scanners | N/A to the new check; existing independent handles, immutable descriptors, and row slots remain separate | two accepted scanner terminals in one program plus the existing nested-scanner rejection |
  | Performance | N/A: no production performance claim | N/A; record the reason in the implementation PR |

  The design acceptance matrix must cover direct and transitive owned fields, nested and optional
  structs (including `Option<Leaf>` Some/null/omitted), every JSON scalar width, borrowed `str`, Copy
  options and unions (including an object-payload arm), local/imported types, concrete-row generic
  calls with resolved, numeric-defaulted, unresolved-bare, conflicting, expected-seed-conflicting,
  forwarding, source-spelling, and partially substituted return-context inference,
  wrapper propagation and multi-argument source order, explicit deferral of unresolved row-type
  generic arguments, semantic rejection before MIR, each active scanner-envelope precedence pair,
  whole-program and per-unit
  interface reconstruction, cold/hot/cache-edit/revert behavior, malformed and exhausted streams,
  and ordinary `json.decode` compatibility. The valid-`Span` active-envelope matrix must exercise
  every pairwise precedence winner among `StoredType`, `UnknownRow`, `InputType`, `Schema`, and
  `Copy`, in addition to the malformed-`Span` pairs. The focused owner tests are
  `m5::json_scan_copy_row_terminal_matrix`, `m5::json_scan_rejects_owned_row_fields`,
  `m5::json_scan_rejects_transitive_owned_row_fields`, `m5::json_scan_generic_row_ownership`,
  `m5::json_scan_generic_return_context_ownership`, `m5::json_scan_generic_return_context_wrapper_matrix`,
  `m5::json_scan_generic_return_context_argument_order_matrix`,
  `m5::json_scan_generic_return_context_expected_conflict_no_cascade`,
  `m5::json_scan_generic_argument_source_spelling`, `m5::json_scan_generic_return_context_numeric_default`,
  `m5::json_scan_generic_return_context_inference_matrix`,
  `m5::json_scan_generic_return_context_partial_composite_rejection`,
  `m5::json_scan_copy_composite_runtime_matrix`,
  `m5::json_scan_rejects_owned_composite_rows`, `hir_program_json_scan_envelope_mismatch`,
  `hir_program_json_scan_envelope_precedence_matrix`,
  `modules::json_scan_imported_row_ownership`, `modules::json_scan_imported_generic_return_context_ownership`,
  `cache_codegen::json_scan_row_schema_rejection`,
  `cache_codegen::json_scan_per_unit_interface_row_ownership`,
  `cache_codegen::json_scan_generic_return_context_no_publication`, and
  `json_scan_cross_compiler_identity`; the feature-gated runtime allocation
  probes are `json_scan_copy_row_no_owned_alloc` and
  `json_scan_copy_row_copy_composites_no_owned_alloc`. The named cross-compiler probe is
  `scripts/compare-json-scan-identity.sh`, which runs the checked-in Rust owner
  `crates/align_driver/tests/json_scan_identity.rs::json_scan_cross_compiler_identity` against the
  fixture `crates/align_driver/tests/fixtures/json_scan_copy_identity.align`. Its two explicit
  inputs are baseline Align commit `576e57307fe4ef34e74566f5e389a2f0e2a04acd` and the exact
  implementation-head SHA recorded in the implementation PR and `HANDOFF.md`. In two clean release
  worktrees it runs `cargo test --release --locked --target x86_64-unknown-linux-gnu -p align_driver
  --test json_scan_identity -- --exact json_scan_cross_compiler_identity` with
  `rustc 1.96.1`, `llvm-config-22 22.1.8`, `cc`, `LC_ALL=C`, `ALIGNC_CACHE=off`, and no custom
  `RUSTFLAGS`; the test writes exact files in an explicit per-worktree output directory. The owner
  compares, with `cmp` and no normalization, canonical serialized interface bytes
  (`align_interface::serialize`), complete structural codegen-input MIR
  (`align_mir::print::codegen_input_to_string`), raw LLVM, object bytes from
  `emit_object_file` with `BuildTarget::Baseline` and `Profile::Release`, and the cache-key inputs
  (`InterfaceSummary.interface_hash` and the actual `CodegenKey` fields: `cache_format_version`,
  `compiler_build_id`, `frontend_schema`, `located`, `impl_hash`, `dep_interface_hashes`, `exports`,
  `target_triple`, `object_format`, `resolved_cpu`, `resolved_features`, `profile_name`, `pipeline`,
  `codegen_opt`, `reloc_model`, `code_model`, `llvm_version`, `rt_lto`, `rt_lto_digest`, `pgo_mode`,
  and `unit`). `interface_hash` is not a `CodegenKey` field; the interface artifact and the codegen
  action key are compared separately. `compiler_build_id` is intentionally expected to differ
  between the baseline and implementation-head compilers, so the full cache-key digest is also
  expected to differ and no cache object may be shared across those compiler builds. The owner
  compares every other listed `CodegenKey` field for exact equality and fails if any additional
  difference appears. The implementation-side
  `cache_codegen::json_scan_copy_row_codegen_key_identity_owner` test separately exercises the
  production `CodegenKey::non_compiler_build_digest()` and `CodegenKey::first_diff()` classifier
  with a compiler-build variant, recording `FirstDiff::CompilerBuildId` rather than merely echoing
  an expected label. The cross-worktree shell owner remains baseline-compatible by comparing the
  explicit serialized fields and the production full/slot digests exposed by the identity test; it
  does not copy newer cache APIs into the historical compiler checkout. That expected build-id
  difference is not treated as a scanner identity failure. The existing
  `cache_codegen::json_scan_row_schema_rejection` and
  `cache_codegen::json_scan_per_unit_interface_row_ownership` separately own cold/hot, schema
  edit/revert, cache-hit/miss, and no-publication behavior. The required Linux object comparison
  fails the gate if unavailable; the align-llm Request 6 adoption fixture owns the later pin change.

  The existing compiler currently admits some owning rows; this paragraph is the reviewed target
  contract, not a claim that the implementation has shipped. The implementation PR must land only
  after this design gate and must keep ordinary decode, encode, and scope Drop for the currently
  admitted `Option<Move-struct>` JSON shape explicit; cleanup after a later required sibling decode
  error remains a separate ownership request.

## Decoded-owner transition closure (Request 15)

Request 15 closes the already-admitted decoded-owner transitions without widening the JSON schema,
changing source syntax, adding a runtime ABI, or changing error precedence. A live
`Option<Move-struct>` payload is released after any later recoverable object failure and its tag and
payload are nulled. Indexed AoS speculation is transactional with respect to owned fields: a failed
speculation cleans every partially written destination before fallback writes it. Top-level AoS staging
cleans every completed row and the current partial row on element, delimiter, EOF, or trailing-input
failure. A completed single-record decode cleans its owned fields when the final trailing-input check
rejects. Cleanup is exact-once and idempotent; successful construction, generated `Drop`, move-out,
replacement, and the existing SoA/scanner non-owning contracts remain unchanged.

The implementation closure matrix is:

| Transition | Owner | Regression |
| --- | --- | --- |
| Optional-owner formation and admitted success | `align_sema` JSON schema and existing recursive `DropPlan` | `m5::json_option_move_struct_payload_remains_admitted` |
| Optional payload after later sibling/type/duplicate/malformed/trailing failure | `align_rt_json_decode`, `parse_object`, and `drop_decoded_owned` | `json_decoded_optional_owner_failure_matrix` |
| Indexed speculation partial write → fallback success/failure | `json_speculate`, `json_fallback`, `write_field_indexed`, and the AoS destination | `json_decoded_owner_speculation_transition_matrix` |
| Top-level AoS completed/current rows on malformed element, delimiter, EOF, or trailing input | `align_rt_json_decode_struct_array` staging ledger and recursive cleanup | `json_decoded_owner_aos_slow_failure_matrix` |
| Nested record, Move union, field-array, and scalar-array compatibility | `parse_object`, `drop_decoded_union`, and existing descriptor-kind cleanup | `json_nested_move_struct_array_failure_no_double_free`, `json_array_of_move_struct_sibling_failure_deep_frees_every_element`, `json_union_array_arm_trailing_garbage_frees_buffer`, and `json_scalar_array_field_sibling_failure_frees_buffer` |
| Success move, replacement, return, branch/loop exit, and generated `Drop` | existing MIR/codegen ownership paths; no new runtime ABI | `m5::json_option_move_struct_payload_remains_admitted`, `m5::json_option_move_struct_later_failure_cleans`, and existing Move/Drop control-flow owners |
| Whole/per-unit/interface/cache and concurrent calls | existing structural fingerprints, unchanged descriptors, and per-call runtime state | existing JSON cache/interface owners plus `json_decoded_owner_same_process_pair_matrix` |

The implementation must map every applicable row to the final diff and a regression witness. The
runtime tests that read process-global allocation counters acquire `ALLOC_COUNT_LOCK` before fixture
construction and hold it through cleanup and final assertions. No new process-global state, CLI input,
persisted field, benchmark claim, or scanner ownership is introduced by this repair.

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
  struct's field names/types feed the codegen descriptor table rather than its surrounding statement
  sequence. The per-unit key therefore fingerprints the complete structural MIR Program, including
  struct/enum tables, `layout(C)`, and alignment. `cache_codegen.rs` gates 2/2b pin flat, nested, and
  type-table-only changes. JSON MIR nodes carry target ids rather than copied schema strings. New
  schema-carrying surfaces must place every backend input in the structural Program; do not add
  cache-only strings to the human MIR printer.

## Test anchors

`m5.rs` (decode matrix: struct/arrays/str-fields/order/unknown-keys/malformed/range #295 #311;
encode escaping; duplicate-key #306; **nested** decode+encode round-trip
`json_decode_encode_nested_struct_roundtrip` + Mison-path `json_decode_nested_struct_array_mison`),
`soa.rs:317` (json→soa filtered aggregate), `cache_codegen.rs` gates 2/2b (structural codegen-input
cache invalidation, flat + nested), runtime `json_decode_nested_struct_single` / `..._array_mison`
(descriptor-level slow + Mison recursion), examples `json.align`, `json_decode.align`,
`json_nested.align`, `soa_json_str.align`; benches `bench/json_decode`, `bench/json_soa` (+ their
READMEs for the measured model).
