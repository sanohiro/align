# Owned declared JSON plan

Status: **ACCEPTED DESIGN; IMPLEMENTATION PENDING**.

This document is the plan of record for align-llm Request 9. It extends the
existing declared-record JSON path with one closed, directly owned text graph.
It does not define the later recursive C6 graph from Request 13.

The implementation starts only from a commit containing this reviewed design.
The public source form remains the existing inferred `core.json` surface. The
change is type-directed acceptance and ownership, not a second API.

## 1. Capability boundary

The accepted source operations are:

```text
json.decode(input: str) -> Result<T, Error>
json.encode(value: T) -> str
json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>
```

`T` is a direct declared record selected from the expected type for decode and
from the value type for encode. There is no written type argument, named/default
argument, option object, environment switch, or `Owned*` marker.

The owned route is selected when a direct record contains at least one of:

```text
string
Option<string>
array<string>
```

Once selected, the complete record grammar is closed:

```text
required signed or unsigned integer of width 8, 16, 32, or 64
required bool
required string
direct Option<string>
direct array<string>
```

No other field is accepted. In particular, `str`, `array<str>`, float, char,
nested records, record arrays, enums, `Option` of anything except `string`,
nested `Option`, and nested arrays are rejected before descriptor construction
or runtime allocation. `layout(C)` and `align(N)` records are also rejected by
this route. A record with no owned text leaf keeps the existing JSON route and
is not narrowed by these rules.

The declaration and positional call are separate:

```align
OwnedTask {
  id: string
  priority: i64
  attempts: u16
  limit: u64
  enabled: bool
  argv: array<string>
  note: Option<string>
}
```

```align
import core.json

fn decode_task(input: str) -> Result<OwnedTask, Error> {
  return json.decode(input)
}
```

The ordinary recursive Move carrier owns retained and transformed raw results;
the JSON route adds no alternate result mechanism:

```align
fn retain(input: str) -> Result<OwnedTask, Error> {
  return json.decode(input)
}

fn pass_raw(result: Result<OwnedTask, Error>) -> Result<OwnedTask, Error> {
  return result
}

fn store_raw(input: str) -> Result<OwnedTask, Error> {
  raw: Result<OwnedTask, Error> := json.decode(input)
  return raw
}

fn to_error(value: Error) -> Error {
  return value
}

fn use_mapped(input: str) -> Result<OwnedTask, Error> {
  raw: Result<OwnedTask, Error> := json.decode(input)
  mapped: Result<OwnedTask, Error> := raw.map_err(to_error)
  task: OwnedTask := mapped?
  return Ok(task)
}
```

The direct-record restriction is deliberate. It gives the first consumer an
independently owned result without silently widening AoS, SoA, union, scanner,
or nested-graph cleanup. Request 13 may later consume this ownership model for
one reviewed recursive graph.

## 2. Public-contract ledger

| Surface | Exact input and defaults | Result and errors | Ownership, lifetime, allocation | Compiler/runtime owner | Artifact and cache identity | Prerequisite and acceptance owner |
| --- | --- | --- | --- | --- | --- | --- |
| `json.decode(input: str) -> Result<T, Error>` where `T` is the closed direct-owned record | One positional `str`; no default or ambient input. Expected-result inference chooses `T`. | `Ok(T)` or existing `Error.Code(1)`. Compile-time capability, arity, inference, graph, and layout failures remain diagnostics. Runtime UTF-8, string grammar, object syntax, duplicate, shape, range, missing-field, and trailing-input failures are recoverable. Capacity overflow and allocation failure retain the runtime-wide terminal-abort policy. | Every `string` is an independent free-standing owner. `array<string>` owns one dynamic spine and every element string. `Option<string>` owns only `Some`. The result is `Static`, has no input or arena dependency, and remains free-standing even when the call appears inside `arena {}`. The owned target makes this allocation choice visible. | `align_sema` route and graph validation; interface target-envelope validation; checked-HIR route/envelope validation; MIR result/out-slot construction; LLVM descriptors and recursive Drop; runtime parse, allocation, cleanup, and integer writers. | `OwnedJsonDescV1` below is structural and target-local. A serialized descriptor is carried only in the exact target-bound interface envelope below; both are part of interface/cache identity. Existing HIR/MIR record definitions remain the semantic source. | Request 7 grammar and Request 15 cleanup are merged. Focused owners are the implementation matrix in §8. |
| `json.encode(value: T) -> str` | One positional bound direct-record value. The source is borrowed for the call and is neither moved nor mutated. | Infallible after compile-time graph/descriptor validation. | Uses the existing template builder. Inside an arena the returned view is arena-backed. Outside an arena it has the existing hidden scoped string owner and cannot escape the frame. Persisting it requires explicit `.clone()`. Source owners remain live and unchanged. | Shared sema plan plus dedicated checked-HIR plan reconstruction; MIR template pieces; LLVM field access; runtime canonical string/array writers. | The same `OwnedJsonDescV1` graph and target-local offsets as decode. Field order is declaration order. | Exact canonical vectors and source-nonmutation owners in §8. |
| `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>` | The same direct-record value plus exact `i64` inclusive limit. No default. | Existing bounded semantics: negative or exceeded limit is `Err(Error.Invalid)`; success is `Ok(string)`; allocation failure is terminal. | Success owns one free-standing `string`. The source is borrowed and unchanged. No unbounded-first pass or discarded partial public value exists. | The same sema and checked-HIR plan as `json.encode`; existing bounded builder/finalizer owns limit behavior. | Identical graph/descriptor identity and canonical bytes to `json.encode`. | Existing Request 12 bounded owners plus owned-field byte-parity and limit-boundary rows in §8. |

The two encode operations must share one accepted graph and ordered plan. A
field accepted by one and rejected by the other is a compiler bug.

`align_sema` owns the shared semantic classifier and `TypeLayoutCache` values.
The driver supplies the canonical target triple/object format to
`align_interface`, which owns the exported-record list, envelope codec, hash,
and import validation without depending on LLVM codegen. `align_codegen_llvm`
independently compares the accepted offsets and ABI cells with the current
`TargetData` before emitting a runtime table. Private/current-unit records and
consumer monomorphs use the same semantic encoder through their checked-HIR/MIR
identity path. No layer reconstructs target identity from descriptor offsets.
Every integer template piece retains its exact `Ty::Int` width and signedness;
LLVM selects the signed or unsigned runtime decimal writer from that type. There
is no signed `i64` reinterpretation of an unsigned source.

No performance threshold is part of this capability. Whole/per-unit allocation
parity is a correctness measurement, not a benchmark promise.

## 3. Allocation and region rule

The ordinary allocation default remains unchanged: an ordinary owned producer
inside `arena {}` uses that arena unless its public contract selects another
mode. A fully owned JSON target is an ownership-directed materializer: selecting
`string`/`Option<string>`/`array<string>` says that the decoded record must be
independent of the input. The complete result is therefore free-standing in and
out of an arena.

This is one general boundary, not a field exception:

```text
borrowed JSON graph  -> input views; escaped selected text may also borrow arena
fully owned JSON graph -> free-standing owners; no input or arena provenance
mixed graph          -> compile-time rejection
```

The decoded aggregate carries one set cleanup bit on success. Every owned child
uses the same free-standing mode. Move, raw `Result` binding, parameter passing,
return, `match`, `else`, `?`, `map_err`, branch joins, loop joins, and replacement
use the existing recursive Move carrier. Moving clears the complete source;
replacement drops the old value before installing the new one.

Decode never allocates an arena fallback or silently converts `str` to `string`.
The target declaration contains owned types, and `json.decode` is the explicit
materializing operation.

## 4. Wire and error contract

Request 7 remains the only JSON string grammar source. Owned text accepts the
same RFC 8259 short escapes and valid UTF-16 surrogate pairs, rejects raw C0,
malformed escapes, invalid surrogate sequences, and invalid UTF-8, and decodes
`\u0000` to an embedded NUL byte. Unknown keys are ignored only after their
complete values pass that grammar. Repeated unknown keys remain ignored.

Declared fields appear in canonical declaration order on encode. Integer fields
retain their declared width and signedness. Decode rejects an out-of-range value
recoverably; `u64` accepts `0..=u64::MAX`, and encode never passes unsigned
values through `i64`. Bool accepts only `true` and `false`. Missing and `null`
map to `None` only for `Option<string>`; `None` is omitted on encode, while
`Some("")` emits an explicit empty string. Required fields reject missing and
`null`; an `array<string>` element rejects `null`.

Validation precedence is deterministic. Route selection first confirms a direct
record and scans its resolved fields only to decide whether at least one direct
`string`, `Option<string>`, or `array<string>` leaf exists. With no such leaf the
existing route owns all later validation. Once an owned leaf selects this route,
the owned classifier uses this exact order:

1. import/capability, arity, and expected-type inference;
2. direct-record identity, then `layout(C)`, then `align(N)` rejection;
3. fields in source declaration order. For one field, resolved-type completeness
   precedes the outer constructor; integer width/sign or exact `string` payload
   follows that constructor. `Option` accepts only `string`; dynamic array accepts
   only `string`; a direct `str` or `array<str>` reports the mixed-borrowed error;
   every other constructor reports the unsupported-owned-field error. The first
   failing field is the only graph diagnostic;
4. canonical natural layout, recursive `DropPlan`, free-standing allocation mode,
   target-bound interface identity, and input type in that order;
5. complete input UTF-8 and JSON string-token grammar validation;
6. object parsing in input order: syntax, declared-key duplicate, value shape,
   integer range, and array-element failures;
7. missing required fields at object close;
8. non-whitespace trailing input;
9. successful result publication and Move transfer.

Encode has a separate deterministic compile-time order:

1. import/capability and arity;
2. source-local/place formation and resolved target class;
3. the same owned-leaf selector, attribute order, source-ordinal field order,
   natural layout, `DropPlan`, allocation mode, and target-bound descriptor
   identity used by decode;
4. exact canonical part plan in source ordinal order;
5. for `encode_bounded` only, `max_bytes` expression checking followed by the
   exact-`i64` requirement.

An explicit layout error wins over every field error; otherwise the earliest
invalid source field wins, and that graph error wins over a simultaneously
non-`i64` limit. A parameterized multi-invalid owner permutes attribute, field
ordinal, field-shape, target-envelope, input, part, and limit failures through
sema, interface import, checked-HIR replay, and all four lowerers and requires
this same first cause. After checked HIR, runtime evaluation reads the
already-bound source parts in canonical plan order, then evaluates `max_bytes`,
then initializes the builder. A negative limit makes that builder sticky-invalid
with zero payload allocation; exact-limit success and first-byte-over-limit
failure follow the existing bounded writer order. Source reads may precede the negative-limit
decision, but no source is moved or mutated and no output owner is published.

A recoverable failure preserves the first error and publishes no partial value.
It drops live direct fields in source declaration order. A live
`array<string>` drops initialized elements in ascending index order and then
its spine. An optional payload is dropped at its field position. Cleanup does
not allocate and is idempotent after source nulling. Terminal overflow/OOM has
no cleanup-after-abort promise.

The normative semantic-to-byte and byte-to-semantic vector is:

```text
input UTF-8 bytes:
{"id":"task-1","priority":-7,"attempts":3,"limit":18446744073709551615,"enabled":true,"argv":["","quote:\" slash:\/ backslash:\\ controls:\b\f\n\r\t","nul:\u0000","emoji:\ud83d\ude00"],"note":"\u20ac"}

decoded semantics:
id = task-1
priority = -7
attempts = 3
limit = 18446744073709551615
enabled = true
argv = empty string; quote/slash/backslash/five control bytes; nul:<NUL>; emoji:<U+1F600>
note = Some(<U+20AC>)

canonical output UTF-8 bytes:
{"id":"task-1","priority":-7,"attempts":3,"limit":18446744073709551615,"enabled":true,"argv":["","quote:\" slash:/ backslash:\\ controls:\b\f\n\r\t","nul:\u0000","emoji:😀"],"note":"€"}
```

The output is compared before any CLI newline. Optional states have three
independent vectors:

```text
omitted input/output:
{"id":"task-1","priority":-7,"attempts":3,"limit":18446744073709551615,"enabled":true,"argv":[]}

null input:
{"id":"task-1","priority":-7,"attempts":3,"limit":18446744073709551615,"enabled":true,"argv":[],"note":null}
null canonical output:
{"id":"task-1","priority":-7,"attempts":3,"limit":18446744073709551615,"enabled":true,"argv":[]}

Some(empty) input/output:
{"id":"task-1","priority":-7,"attempts":3,"limit":18446744073709551615,"enabled":true,"argv":[],"note":""}
```

Implementation checks in both directions: decode each input to the stated
semantic value, encode the semantic value to the exact output, decode the
output again, and require semantic stability. The Align-owned checked-in
fixture copies these bytes; the external request register is never a build
input.

## 5. Checked descriptor and ABI

`OwnedJsonDescV1` is a compiler-private canonical byte record. It is target-local
and is never serialized naked. Per-unit interface data carries it in the exact
target-bound `OwnedJsonInterfaceEnvelopeV1` below, and both records participate
in interface/cache identity. Whole-program compilation constructs the identical
inner descriptor for its current target. Neither record is a public file format
or runtime-reflection API.

```text
descriptor := u8 schema_version (= 1)
              u8 layout_mode (= 0, natural only)
              u8 layout_algorithm (= 1, descending alignment; stable declaration ties)
              u32 field_count_le (non-zero)
              field[field_count]

field := u32 name_len_le
         u8[name_len] name_utf8
         u8 type_tag
         type_payload
         u32 physical_payload_offset_le
         u32 optional_tag_offset_le  // 0xffffffff for required
         u32 layout_size_le
         u32 layout_align_le
         u8 allocation_mode          // 0 Copy, 1 free-standing owner
         u8 drop_tag

type 0x01 integer := u8 bits, u8 signedness  // 0 signed, 1 unsigned
type 0x03 bool    := empty
type 0x10 string  := empty
type 0x11 Option<string> := empty
type 0x12 array<string> := u8 element_tag (= 0x10), u8 drop_plan_version (= 1)

drop 0x00 Copy
drop 0x01 string
drop 0x02 Option<string>
drop 0x03 array<string>
```

Type tag `0x02` is reserved and rejected by v1. Names are non-empty ASCII Align
identifiers, unique, and serialized in source declaration order. Integer widths
are 8/16/32/64. Every tag, payload, allocation
mode, and drop tag must agree. Field offsets are record-base-relative target ABI
offsets. For `Option<string>`, both the payload and tag offsets add their
target-local offsets to the field base. Size/alignment and the natural layout
algorithm must equal the compiler's canonical `TypeLayoutCache`/
`logical_to_physical` result. Counts, lengths, and offsets use checked arithmetic.
Trailing bytes reject.

The implementation bumps `align_interface::FORMAT_VERSION` from `6` to exactly
`7` before adding this field. A v6 artifact therefore fails as
`UnknownVersion(6)` before any field count, envelope, hash, or descriptor byte is
read; `7` is included in the interface surface hash and the frontend K3 schema
key. No compatibility decoder or v6/v7 alternate path is added.

Format 7 adds `owned_json_descriptors` after the exported struct
definitions and before exported enums. It is a `u32` little-endian count followed
by this record, sorted by exported local struct name:

```text
owned_json_interface_entry := u32 type_name_len_le
                              u8[type_name_len] type_name_ascii
                              u32 envelope_len_le
                              u8[envelope_len] envelope

envelope := u8 envelope_version (= 1)
            u32 target_triple_len_le
            u8[target_triple_len] llvm_canonical_target_triple_ascii
            u8 object_format        // 0 ELF, 1 Mach-O
            u8 endian               // 0 little; no other v1 value
            u8 pointer_size         // 8
            u8 pointer_align        // 8
            u8 string_size          // 16
            u8 string_align         // 8
            u8 array_header_size    // 16
            u8 array_header_align   // 8
            u8 option_string_size   // 24
            u8 option_string_align  // 8
            u8 option_tag_offset    // 0
            u8 option_payload_offset // 8
            u64 abi_hash_lo_le
            u64 abi_hash_hi_le
            u32 descriptor_len_le
            u8[descriptor_len] OwnedJsonDescV1
```

The canonical target triple is exactly `align_codegen_llvm::default_triple()`;
it is nonempty ASCII with no NUL and is not a Rust distribution target alias.
The ABI bytes deliberately name every target-dependent layout fact used by the
v1 graph. The three supported release targets must match this 64-bit
little-endian ABI tuple; a future target with any different value cannot produce
or import a v1 envelope. `abi_hash` is existing `Hash128::of` over the envelope
bytes from `envelope_version` through `option_payload_offset`, in exact encoded
order. It is a local non-cryptographic consistency identity, not an authenticity
claim. The enclosing interface hash covers the entry name, complete envelope,
hash, and inner descriptor.

The list contains exactly one entry for each non-generic exported struct that
matches the direct-owned grammar, in the same name order as the corresponding
interface struct table. Generic definitions remain semantic interface records;
each accepted concrete monomorph constructs its target-local envelope in the
consumer and folds it into that consumer's structural MIR implementation hash.
Private/current-unit concrete descriptors likewise live in checked HIR/MIR and
the implementation hash, never the public interface hash. This preserves the
interface/implementation split: a private body edit cannot invalidate consumers.
Duplicate, missing, extra, generic, unknown-name, or out-of-order entries reject.
Identity is explicit: inner descriptor and ABI envelope bytes are structural;
the interface entry's `type_name` association is nominal within `summary.unit`.
The enclosing interface hash therefore changes on either nominal rename or any
reachable accepted-graph/envelope change. A consumer monomorph's descriptor is
structural over the complete resolved direct graph; the existing MIR type record
retains its ordinary nominal identity separately.

For the required x86_64 Linux baseline, LLVM's canonical triple is
`x86_64-pc-linux-gnu`. The exact 36-byte ABI prefix and its following 16-byte
little-endian hash are:

```text
abi-prefix 01 13 00 00 00 78 38 36 5f 36 34 2d 70 63 2d 6c 69 6e 75 78 2d 67 6e 75 00 00 08 08 10 08 10 08 18 08 00 08
abi-hash   d4 df f2 a5 8e c8 21 27 2a f3 26 2f 96 1a eb a5
desc-len   d6 00 00 00
```

Appending the 214 descriptor bytes below yields an exact 270-byte envelope.
The independent envelope golden owner encodes these semantic ABI cells to those
bytes and hash, decodes them back, and then consumes the inner golden; it does
not copy the production encoder or hash-byte assembly.

For the `OwnedTask` declaration above on the required x86_64 Linux baseline,
the exact 214-byte descriptor is grouped by header then source field:

```text
header   01 00 01 07 00 00 00
id       02 00 00 00 69 64 10 00 00 00 00 ff ff ff ff 10 00 00 00 08 00 00 00 01 01
priority 08 00 00 00 70 72 69 6f 72 69 74 79 01 40 00 10 00 00 00 ff ff ff ff 08 00 00 00 08 00 00 00 00 00
attempts 08 00 00 00 61 74 74 65 6d 70 74 73 01 10 01 48 00 00 00 ff ff ff ff 02 00 00 00 02 00 00 00 00 00
limit    05 00 00 00 6c 69 6d 69 74 01 40 01 18 00 00 00 ff ff ff ff 08 00 00 00 08 00 00 00 00 00
enabled  07 00 00 00 65 6e 61 62 6c 65 64 03 4a 00 00 00 ff ff ff ff 01 00 00 00 01 00 00 00 00 00
argv     04 00 00 00 61 72 67 76 12 10 01 20 00 00 00 ff ff ff ff 10 00 00 00 08 00 00 00 01 03
note     04 00 00 00 6e 6f 74 65 11 38 00 00 00 30 00 00 00 18 00 00 00 08 00 00 00 01 02
```

The independent descriptor golden owner must construct this byte sequence from
the semantic declaration and decode the same bytes back to the semantic fields,
including `attempts` at record offset 72, `enabled` at 74, and the nonzero
`note` field's record-relative payload/tag offsets 56/48. One-field mutations
cover version, layout mode/algorithm, zero count, count/length truncation and
overflow, name grammar/duplicate/order, unknown/reserved type tag, type payload,
signedness, offset, optional sentinel, size/alignment, allocation/drop agreement,
array element/drop-plan pair, and trailing bytes.

Interface-envelope and descriptor validation has one deterministic failure
order:

1. entry count availability, then each sorted entry's name length/bound,
   identifier grammar, known non-generic exported struct, uniqueness, source
   order, envelope length, and exact envelope bound;
2. envelope version, target-triple length/bound, nonempty canonical ASCII triple,
   object-format tag, endian tag, then pointer/string/array/option size, alignment,
   and option-offset cells in encoded order;
3. stored ABI hash equality, current compiler target-triple equality, current
   object format, and current target ABI tuple in that order. These checks finish
   before descriptor length or any descriptor offset is trusted;
4. descriptor length availability/bound and exact envelope end;
5. seven-byte descriptor header availability, then schema version, layout mode, layout
   algorithm, nonzero field count, and equality to the expected direct-record
   field count in that order;
6. each source-ordinal field in order: name-length availability and checked
   remaining-byte bound, nonempty ASCII identifier grammar, uniqueness and
   equality to the expected field name, accepted type tag, exact tag-specific
   payload, physical payload offset, optional tag offset/sentinel, ABI size,
   ABI alignment, allocation mode, and drop tag;
7. exact descriptor end, then exact list cardinality, rejecting any trailing byte
   or missing/extra accepted exported record.

The first failing check is the only reported cause. An imported interface with
malformed bytes, a target/ABI mismatch, or a semantic mismatch is rejected as an
interface decode diagnostic before cache lookup, checked-HIR lowering, or
codegen; it is never silently rebuilt. The persistent frontend key's existing
target triple remains a redundant cache partition, not the authenticity evidence
for an envelope. A well-formed descriptor whose identity differs at an object
cache lookup is an ordinary cache miss and is rebuilt from current checked HIR.
Whole-program compilation constructs the descriptor from validated semantic
types, while handcrafted malformed HIR fails the checked-HIR gate before the
descriptor producer. These outcomes are identical for whole-program and
per-unit compilation.

The descriptor is structural over the complete accepted direct graph. Field
rename/reorder/type/width/signedness/layout/allocation/drop changes invalidate
identity; edit/revert restores it. It contains no pointer, local type id, source
position, or declaration hash. The envelope, rather than ambient cache state,
binds the target triple and every relevant ABI cell before descriptor validation,
so cross-target interface reuse rejects before codegen. The exact required baseline
is `x86_64-unknown-linux-gnu` on Ubuntu 24.04 with Rust 1.96 and LLVM 22. The
release-target acceptance environments are `aarch64-unknown-linux-gnu` on Ubuntu
24.04-arm and `aarch64-apple-darwin` on macOS 15 with the same Rust/LLVM majors.
No 32-bit contract is added.

The decode runtime keeps the existing C ABI:

```text
align_rt_json_decode(
  input: *const u8, input_len: i64,
  fields: *const JsonField, n_fields: i64,
  out: *mut u8, out_size: i64,
  phf: *const i32, phf_len: i64, phf_seed: i64,
  arena: *mut Arena
) -> i32
```

Owned direct decode passes a null arena and uses new owned descriptor kinds in
the existing `JsonField` table. Those kinds allocate free-standing strings and
deep-owned text arrays. Borrowed decode keeps the Request 7 arena behavior.
Encode adds exactly one keyed runtime ABI,
`align_rt_builder_write_uint(builder: *mut Builder, value: u64) -> ()`, reusing
the LLVM A66 shape `void @SYM(ptr, i64)`. Its wrapper checks the existing sticky
limit and calls the existing internal `builder_push_u64`; allocation, failure,
and builder ownership do not change. Signed integers keep
`align_rt_builder_write_int`; every unsigned width is zero-extended to `i64`
bits and calls the new unsigned writer. Owned `string` is read as bytes, and
`array<string>` uses the existing scalar-array writer shape with owned elements.
Runtime descriptor kinds are generated only from a validated
`OwnedJsonDescV1`; malformed interface records reject before LLVM/runtime use.

The `JsonField.tag` kind-byte extension is exact:

| `OwnedJsonDescV1` field | Runtime kind | Width byte | `opt_tag` |
| --- | --- | --- | --- |
| `0x10` `string` | `8` | `16` | `-1` |
| `0x11` `Option<string>` | `8` | `16` | record-relative option tag offset |
| `0x12` `array<string>` | `9` | `16` | `-1` |

Kinds `0..=7` retain their existing meanings. For the two new kinds the packed
integer signedness bit is zero and `sub` is null. The table's `offset` is the
record-relative payload offset already validated by `OwnedJsonDescV1`; no runtime
host-layout reconstruction is permitted.

Kinds `8` and `9` are legal only on the direct-owned A103 decode path and its
internal failure cleanup. Encode never passes them to A80. `OwnedJsonString` and
`OwnedJsonOptionStringField` use the existing JSON-string writer A73;
`OwnedJsonStringArray` uses A74 with the existing read-only `str` element tag
`(3 << 8) | 16`, since owned `string` and borrowed `str` share the same
`{ptr,len}` read layout. This changes no encode runtime function type.

## 6. Routing and exclusions

The owned predicate is operation-specific. It must not replace the existing
JSON whitelist.

| Entry class | Rule |
| --- | --- |
| direct record decode/encode/bounded encode | Select owned route only when an owned text leaf exists and the complete graph is accepted. |
| direct record with no owned text | Existing borrowed route, unchanged. |
| top-level scalar/`array<scalar>` decode | Existing scalar route, unchanged. `array<bool>` receives a missing direct owner test. |
| top-level or field `array<Struct>` | Existing AoS route, unchanged. Request 9 owned-text elements reject before owned descriptor construction. |
| fixed `StructArray` encode/bounded encode | Existing unrolled route, unchanged. |
| union decode/encode/bounded encode | Existing shape-directed route, unchanged. |
| `soa<Struct>` | Existing arena-column route, unchanged. |
| `json.scan` | Existing recursively Copy row gate, unchanged; owned rows reject before construction. |
| `json.doc` | Existing lazy arena view, unchanged. |

The complete same-process operation-variant set is:

```text
{OD, OE, OEB, BD, SD, AD, BE, BEB, FE, FEB, UD, UE, UEB, DOC, SCAN, AOS, SOA}
```

`O`/`B`/`F`/`U` mean owned record, borrowed record, fixed struct array, and union;
`D`/`E`/`EB` mean decode, encode, and bounded encode. `SD` and `AD` are scalar and
scalar-array decode; the remaining names are `json.doc`, `json.scan`, AoS decode,
and SoA decode. The full unordered product including diagonals has 153 pairs. All
are supported concurrently, neither serialized nor rejected before side effects.
Every encode target therefore exercises unbounded/unbounded,
unbounded/bounded, and bounded/bounded overlap as well as cross-target pairs.
Parser, destination, temporary owners, and builders are per call. Immutable
descriptors may be shared. No global lock, mutable codec state, environment
input, or overlap rejection is added.

## 7. Implementation boundary

This capability crosses type formation, HIR/MIR, LLVM layout/ABI, runtime parse
and allocation, recursive Drop, interfaces, and caches. Splitting its strict
producer-to-consumer chain would leave a dormant descriptor or a reachable
owner without its cleanup consumer and would duplicate the same proof across
branches. One implementation PR is therefore expected to exceed roughly 1,000
hand-written lines. The larger boundary lowers integration risk because graph
admission, construction, cleanup, ABI, and owner tests land atomically. The
design PR remains independently useful because it freezes the public and safety
strategy before code.

No implementation may begin by widening a general JSON helper. First add the
owned route classifier and checked plan, then thread that exact plan through
construction and cleanup. Existing routes remain executable throughout local
development.

### 7.1 Checked-HIR and MIR boundary

The owned route uses dedicated HIR discriminants so malformed or imported HIR
cannot reinterpret an existing borrowed, AoS, union, or scanner node:

```text
JsonOwnedDecode { struct_id, input }
JsonOwnedEncode { base, parts }
JsonOwnedEncodeBounded { base, parts, max_bytes }
```

Its exact new template parts are `OwnedJsonString { access }`,
`OwnedJsonOptionStringField { name, access }`, and
`OwnedJsonStringArray { access }`. Static object syntax remains in `Text` parts;
Copy fields retain their existing exact `Hole` parts. `IntHole` preserves the
exact signed/unsigned `Ty::Int`: codegen sign-extends signed widths into
`BuilderWriteInt` and zero-extends every unsigned width into the new
`BuilderWriteUint`; `u64` therefore never reinterprets its high bit as a sign.
Existing `JsonDecode`,
`Template`, `JsonEncodeBounded`, `JsonDecodeStructArray`, union, SoA, and
`JsonScan` discriminants retain their current predicates and reject the new
owned graph.

The active checked-HIR body gate independently reconstructs the closed direct
graph and `OwnedJsonDescV1` from `struct_id`/`base`, validates natural layout,
the recursive `DropPlan`, exact result type, every part name/access/type/order,
and the operation-specific allocation mode, then compares the reconstructed
plan to the stored parts. `JsonOwnedDecode` requires `input: str`, returns exact
`Result<Struct, Error>`, and records free-standing output even at nonzero arena
depth. The two encode nodes require one visible bound source local and borrow
every access; bounded encode additionally validates `max_bytes: i64` after all
source parts and returns exact `Result<string, Error>`.

Validated owned decode lowers to a distinct MIR `JsonOwnedDecode` rvalue so its
free-standing allocation and recursive cleanup cannot be confused with the
nullable-arena borrowed rvalue. Owned encode lowers to the existing builder
machinery only after its dedicated HIR plan has validated; its three new part
kinds remain distinct through MIR and LLVM writer selection. All whole-program,
located, per-unit, and located-per-unit entrypoints run the same active body
gate. Any malformed owned node or part returns the canonical empty MIR with the
body-validation pass identity before descriptor emission or runtime calls.
Adding these HIR nodes, part kinds, and the MIR rvalue also extends every
exhaustive visitor, replay/clone walk, source-shape/implementation-hash encoder,
child enumerator, validator, lowering dispatcher, and enum-sweep tripwire in the
same commit. No wildcard arm may make a new node silently skip an analysis pass.

## 8. Implementation closure matrix

| Cell | Implementation owner | Required owner |
| --- | --- | --- |
| formation, inference, selected/non-selected route, mixed/nested/layout rejection, and first-invalid precedence | `align_sema` direct-owned classifier beside existing JSON shape checks; explicit attributes before source-ordinal fields | `m5_owned_json::formation_and_target_routing` plus every integer width/sign, generic substitutions, and the attribute/field/type multi-invalid permutation owner |
| checked-HIR route, envelope, plan reconstruction, ownership mode, malformed-node refusal, and new-variant consumer sweep | dedicated `JsonOwned*`/owned-part validator in the active body gate; exhaustive HIR/MIR visitors and source-shape/hash encoders; existing route predicates unchanged | parameterized `validate_hir` mutation sweep over node, type, field, part, descriptor, allocation, whole/per-unit, all four lowering entrypoints, and `variant_sweep_tripwire` |
| construction of direct string, optional string, and text array; empty/NUL/UTF-8/escape states | MIR owned decode result + runtime owned writers | `decode_owned_text_states_detach_from_input` |
| allocation inside/outside arena, input drop, move-out, return | sema region/allocation mode + MIR cleanup bit | `owned_decode_is_free_standing_inside_arena` and `owned_decode_outlives_input` |
| move-in/out, raw `Result`, parameter/return/reassignment, source nulling | canonical recursive Move/Drop paths | `owned_json_result_transfer_matrix` |
| `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early return | sema flow and MIR cleanup CFG | `owned_json_control_flow_cleanup_matrix` |
| replacement and ordinary Drop | drop-old lowering + canonical record `DropPlan` | `owned_json_replacement_and_drop_order` |
| malformed/duplicate/type/range/missing/trailing failure after each live-owner prefix | runtime parser and direct decoded-owner cleanup | `owned_json_recoverable_failure_prefix_matrix` |
| array growth/current element/completed elements/spine cleanup | runtime text-array staging | `owned_json_text_array_transition_matrix` |
| overflow versus OOM terminal policy | checked runtime arithmetic and allocator failpoint | separate decode-growth, encode-growth, and allocation child owners |
| canonical bytes, every integer width/sign including `u64::MAX`, optional states, source nonmutation | typed `IntHole`; signed A66 writer and new unsigned A66 writer share the builder | `template_unsigned_decimal_boundaries` plus `owned_json_canonical_vectors`, with signed/unsigned boundary pairs through ordinary templates and unbounded/bounded encode |
| bounded exact-limit/rejected-next and unbounded byte parity | existing bounded builder with owned plan | `owned_json_bounded_parity` |
| target-local descriptor provenance, cross-target rejection, and interface/implementation split | `OwnedJsonInterfaceEnvelopeV1`, exported non-generic descriptor list, consumer-created monomorph envelopes, interface and MIR hashes | independent 270-byte envelope/hash round trip; every prefix/hash/list mutation; x86_64/aarch64 Linux and Apple mismatch; private-body non-invalidation |
| interface schema transition | `align_interface::FORMAT_VERSION = 7`; descriptor list exists only in format 7; frontend K3 consumes the same constant | exact v7 surface/hash golden, v6 `UnknownVersion(6)` before list parsing, cold/edit/revert and cross-process cache owners |
| `OwnedJsonDescV1` malformed record, target-local offsets, edit/revert | target envelope before interface descriptor codec, layout cache, codegen validation | descriptor golden/malformed matrix and target mismatch owner |
| whole/per-unit/monomorph/cache parity | interface import and structural cache identity | generic, per-unit, cold/edit/revert owners plus producer/consumer ABI-envelope equality |
| existing borrowed/AoS/SoA/union/scanner/fixed-array routes | unchanged route-specific gates | parameterized compatibility owner including `array<bool>` top-level decode |
| same-process and process concurrency | per-call state; immutable descriptor globals | full 153-pair operation-variant matrix and two-process owner |

The author-side matrix-to-diff pass must point every row to implementation and
a regression that would fail on the pre-change compiler. Reused tests count only
when the changed defect would make them fail. A review finding triggers a sweep
of the complete root-cause class before the one fix commit.

### Reopened axis: target provenance and deterministic invalid-graph order

The revised-diff review found that the original matrix treated a target-local
descriptor as if the ambient per-unit cache key authenticated its bytes, and
grouped invalid graph shapes without an intra-phase order. This reopens the
artifact/provenance and validation-order axes. The capability boundary now owns
the target envelope, interface-list placement, generic/private split, independent
envelope golden, and cross-target rejection as one interface-codec failure domain;
it also owns one shared source-ordinal classifier used by sema and checked-HIR
replay. Implementation may not split either producer from its validating
consumer, and may not rely on cache partitioning or duplicated field walks as
proof.

### Reopened axis: scalar signedness and interface schema transition

The post-redesign review found two producer/consumer cells still hidden inside
broader rows: `IntHole` preserved an unsigned type but codegen always selected
the signed decimal ABI, and the new interface list lacked its mandatory format
transition. This reopens the scalar-writer and artifact-version axes. The owned
encode boundary now includes the `BuilderWriteUint` registry key, runtime export,
LLVM selection, and signed/unsigned boundary owner in the same capability. The
interface boundary includes the format-7 constant, encoder, decoder, surface
hash, frontend K3 key, stale-v6 rejection, and codec/cache goldens. Neither may
land as a dormant producer without its consumer and mutation owner.

## 9. Documentation and lifecycle

This design commit updates `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, `docs/impl/08-memory-model-v2.md`, the cache/interface
plan, the English/Japanese JSON designs, the checked-HIR and runtime ABI ledgers,
the library-boundary plan, dependent interface-format ledgers and mirrors,
`docs/open-questions.md`, and `HANDOFF.md`. The implementation must update this
plan's status and those sources only where the shipped contract or capability
state changes. It also
adds the Align-owned syntax/golden fixtures.

After Align merges the implementation, update the sibling request register with
the merged PR, exact ownership surface and limits, leave that edit uncommitted,
and run exactly `cargo build --release --workspace`. align-llm adoption and its
final `make ci` remain sibling-repository work.
