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
| `json.decode(input: str) -> Result<T, Error>` where `T` is the closed direct-owned record | One positional `str`; no default or ambient input. Expected-result inference chooses `T`. | `Ok(T)` or existing `Error.Code(1)`. Compile-time capability, arity, inference, graph, and layout failures remain diagnostics. Runtime UTF-8, string grammar, object syntax, duplicate, shape, range, missing-field, and trailing-input failures are recoverable. Capacity overflow and allocation failure retain the runtime-wide terminal-abort policy. | Every `string` is an independent free-standing owner. `array<string>` owns one dynamic spine and every element string. `Option<string>` owns only `Some`. The result is `Static`, has no input or arena dependency, and remains free-standing even when the call appears inside `arena {}`. The owned target makes this allocation choice visible. | `align_sema` route and graph validation; MIR result/out-slot construction; LLVM descriptors and recursive Drop; runtime parse, allocation, cleanup, and integer writers. | `OwnedJsonDescV1` below is structural, target-local, and part of interface/cache identity. Existing HIR/MIR record definitions remain the semantic source. | Request 7 grammar and Request 15 cleanup are merged. Focused owners are the implementation matrix in §8. |
| `json.encode(value: T) -> str` | One positional bound direct-record value. The source is borrowed for the call and is neither moved nor mutated. | Infallible after compile-time graph/descriptor validation. | Uses the existing template builder. Inside an arena the returned view is arena-backed. Outside an arena it has the existing hidden scoped string owner and cannot escape the frame. Persisting it requires explicit `.clone()`. Source owners remain live and unchanged. | Shared checked encode plan, MIR template pieces, LLVM field access, runtime canonical string/array writers. | The same `OwnedJsonDescV1` graph and target-local offsets as decode. Field order is declaration order. | Exact canonical vectors and source-nonmutation owners in §8. |
| `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>` | The same direct-record value plus exact `i64` inclusive limit. No default. | Existing bounded semantics: negative or exceeded limit is `Err(Error.Invalid)`; success is `Ok(string)`; allocation failure is terminal. | Success owns one free-standing `string`. The source is borrowed and unchanged. No unbounded-first pass or discarded partial public value exists. | The same checked encode plan as `json.encode`; existing bounded builder/finalizer owns limit behavior. | Identical graph/descriptor identity and canonical bytes to `json.encode`. | Existing Request 12 bounded owners plus owned-field byte-parity and limit-boundary rows in §8. |

The two encode operations must share one accepted graph and ordered plan. A
field accepted by one and rejected by the other is a compiler bug.

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

Validation precedence is deterministic:

1. import/capability, arity, and expected-type inference;
2. direct-owned route selection, closed graph, natural layout, recursive
   `DropPlan`, allocation mode, interface identity, and input type;
3. complete input UTF-8 and JSON string-token grammar validation;
4. object parsing in input order: syntax, declared-key duplicate, value shape,
   integer range, and array-element failures;
5. missing required fields at object close;
6. non-whitespace trailing input;
7. successful result publication and Move transfer.

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

`OwnedJsonDescV1` is a compiler-private canonical byte record. It is serialized
in per-unit interface data and included in the implementation/cache fingerprint.
Whole-program compilation constructs the identical record. It is not a public
file format or runtime-reflection API.

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
cover version, layout mode/algorithm, count/length truncation and overflow,
name grammar/duplicate/order, type payload, signedness, offset, optional sentinel,
size/alignment, allocation/drop agreement, array element/drop-plan pair, and
trailing bytes.

The descriptor is structural over the complete accepted direct graph. Field
rename/reorder/type/width/signedness/layout/allocation/drop changes invalidate
identity; edit/revert restores it. It contains no pointer, local type id, source
position, or declaration hash. Target triple precedes descriptor validation, so
cross-target interface reuse rejects before codegen. The exact required baseline
is `x86_64-unknown-linux-gnu` on Ubuntu 24.04 with Rust 1.96 and LLVM 22. The
release-target acceptance environments are `aarch64-unknown-linux-gnu` on Ubuntu
24.04-arm and `aarch64-apple-darwin` on macOS 15 with the same Rust/LLVM majors.
No 32-bit contract is added.

The runtime keeps the existing C ABI:

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
Existing encode builder ABIs remain unchanged; owned `string` is read as bytes,
and `array<string>` uses the existing scalar-array writer shape with owned
elements. Runtime descriptor kinds are generated only from a validated
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

## 8. Implementation closure matrix

| Cell | Implementation owner | Required owner |
| --- | --- | --- |
| formation, inference, selected/non-selected route, mixed/nested/layout rejection | `align_sema` direct-owned classifier beside existing JSON shape checks | `m5_owned_json::formation_and_target_routing` plus generic substitutions |
| construction of direct string, optional string, and text array; empty/NUL/UTF-8/escape states | MIR owned decode result + runtime owned writers | `decode_owned_text_states_detach_from_input` |
| allocation inside/outside arena, input drop, move-out, return | sema region/allocation mode + MIR cleanup bit | `owned_decode_is_free_standing_inside_arena` and `owned_decode_outlives_input` |
| move-in/out, raw `Result`, parameter/return/reassignment, source nulling | canonical recursive Move/Drop paths | `owned_json_result_transfer_matrix` |
| `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early return | sema flow and MIR cleanup CFG | `owned_json_control_flow_cleanup_matrix` |
| replacement and ordinary Drop | drop-old lowering + canonical record `DropPlan` | `owned_json_replacement_and_drop_order` |
| malformed/duplicate/type/range/missing/trailing failure after each live-owner prefix | runtime parser and direct decoded-owner cleanup | `owned_json_recoverable_failure_prefix_matrix` |
| array growth/current element/completed elements/spine cleanup | runtime text-array staging | `owned_json_text_array_transition_matrix` |
| overflow versus OOM terminal policy | checked runtime arithmetic and allocator failpoint | separate decode-growth, encode-growth, and allocation child owners |
| canonical bytes, `u64::MAX`, optional states, source nonmutation | shared encode plan and runtime writers | `owned_json_canonical_vectors` |
| bounded exact-limit/rejected-next and unbounded byte parity | existing bounded builder with owned plan | `owned_json_bounded_parity` |
| `OwnedJsonDescV1` malformed record, target-local offsets, edit/revert | interface codec, layout cache, codegen validation | descriptor golden/malformed matrix and target mismatch owner |
| whole/per-unit/monomorph/cache parity | interface import and structural cache identity | generic, per-unit, cold/edit/revert owners |
| existing borrowed/AoS/SoA/union/scanner/fixed-array routes | unchanged route-specific gates | parameterized compatibility owner including `array<bool>` top-level decode |
| same-process and process concurrency | per-call state; immutable descriptor globals | full 153-pair operation-variant matrix and two-process owner |

The author-side matrix-to-diff pass must point every row to implementation and
a regression that would fail on the pre-change compiler. Reused tests count only
when the changed defect would make them fail. A review finding triggers a sweep
of the complete root-cause class before the one fix commit.

## 9. Documentation and lifecycle

This design commit updates `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, `docs/impl/08-memory-model-v2.md`, the English/Japanese
JSON designs, the runtime ABI ledger, `docs/open-questions.md`, and `HANDOFF.md`.
The implementation must update this plan's status and those sources only where
the shipped contract or capability state changes. It also adds the Align-owned
syntax/golden fixtures.

After Align merges the implementation, update the sibling request register with
the merged PR, exact ownership surface and limits, leave that edit uncommitted,
and run exactly `cargo build --release --workspace`. align-llm adoption and its
final `make ci` remain sibling-repository work.
