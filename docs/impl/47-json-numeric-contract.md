# Standard JSON numbers and owned encoder results (R63)

Status: **Implemented; focused correctness owners and local performance acceptance
complete. Ordinary delivery gates and external consumer adoption are separate**.
Provider baseline: `d469adcb931848ba03d18dcff022d32d95c2fda9`.
The owner approved ordinary JSON support, performance preservation, structural
owned float leaves, and owned `Result<string, Error>` from both encoders.
This is the exact contract completing the JSON row of [plan 46](46-deferred-client-boundaries-plan.md).
It does not select or implement R65. The owner expressly reopened R63; no further
friction-count prerequisite applies, and the general friction rule is unchanged.

## 1. Authoritative public-contract ledger

These are builtin declarations in design notation, not generic user-function
implementations. Calls have positional arguments and no written type arguments.
All operations require `import core.json`. JSON computation is Pure; argument
side effects retain their inferred effect. No new option, ambient configuration,
CLI argument, callback, codec customization, or parallel execution is introduced.

| Surface | Inputs, defaults and order | Result/errors | Ownership and allocation | Owner and identity | Acceptance |
| --- | --- | --- | --- | --- | --- |
| `json.encode(value: T) -> Result<string, Error>` | One stable named-local source place in the existing encode root domain; borrow it, validate its complete schema and moved state. No default. | Ok owns the complete canonical UTF-8 JSON. Nonfinite selected float -> `Error.Invalid`. No partial success. | Call-scoped shared source borrow, including transitive views. Success is free-standing even inside arena, with one transferred grow buffer; no source/output alias or final copy. Failure frees builder storage exactly once. Allocation failure/size overflow retain terminal runtime policy. | Sema owns schema/borrow; HIR validates route; MIR owns error CFG/Drop; LLVM lowers checked operations; runtime writes. Interface 12 and V3 graph; compiler/runtime build identities invalidate old caches. | E1–E5, O1–O3, T1, P1 in §8. |
| `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>` | Same source; source/schema diagnostics precede limit checking. Second argument is exact i64 (untyped integer literal may bind); evaluate once in source order. No default. | Same Invalid for nonfinite; negative cap or first byte beyond inclusive cap -> Invalid. No output on failure. | Same ownership. Output buffer capacity never exceeds the nonnegative cap; no unbounded-first pass. Source remains reserved while evaluating the limit. | One HIR/MIR/native encoder shared with unbounded mode; limit is a call value, not schema identity. | E1–E5, O1–O3, T1, P1. |
| `json.decode(input: str) -> Result<T, Error>` | Existing inferred target domains. Existing owned-string-to-str input borrow. Existing lexical validation/traversal order, with the selected scalar conversion rules; no new full-input prepass. | A selected f32/f64 is rounded directly to its destination precision, ties-to-even. Nonfinite rounded result or wrong token kind -> existing `Error.Code(1)`. No partial target. | Existing target-specific ownership: owned graph independent of source/arena; borrowed views and SoA retain their existing regions. A float leaf is Copy and owns nothing. All initialized owning prefixes clean up on error. | Shared runtime typed conversion reaches strict/indexed/speculative/fallback/AoS/SoA/scanner paths. The integer parser remains exact and separate. | N1–N4, O1–O3, T1, P1. |
| `value.as_f64() -> Option<f64>` where `value: json.doc` | Existing `json.doc` value accessor; numeric token conversion uses the same finite f64 conversion. | Some(finite) or None for wrong kind/out-of-range. | Copy result; no materialized text, no new allocation. Raw document accepts grammar-valid large numbers for navigation/skipping. | Existing tape/number-span accessor; no new wire or type. | N2, N3. |
| Owned record grammar | Add f32/f64 to every existing scalar position of the selected recursive owned graph, including Option and dynamic array elements. Existing nonempty/natural/acyclic/depth-128 rules stay. | Unsupported graph is a compile-time error before native table construction/allocation. | Float Copy; existing string/record/Option/array Drop plans. No mixed borrowed-text graphs, new containers or custom codecs. | V3 descriptor/envelope, complete reachable structural graph identity, nominal exported-root association. | G1–G3, T1. |

Encoding accepts a direct record, fixed struct-array, or accepted shape-directed sum;
R87 [plan 59](59-json-array-root-composition-plan.md) also admits dynamic arrays with
the same element grammar as existing array fields. Nested arrays/records and union
payloads follow their existing declared schema. This work does not add bare
scalar encode, a JSON tree type, or a new ownership carrier. Decode
and document navigation retain their broader existing root domains. A JSON
conversion targets a declared representation; arbitrary precision is not added.

## 2. Numeric semantics and byte spelling

JSON lexical grammar is [RFC 8259 §6](https://www.rfc-editor.org/rfc/rfc8259#section-6), with JSON whitespace only at its existing
structural positions. No leading plus, leading-zero integer, missing fraction or
exponent digit, hex number, NaN or Infinity token is accepted. The existing leading-zero acceptance in `JsonParser::number_span` is repaired
across selected/ignored numeric tokens and document parsing; integer scanners
receive the same leading-zero guard without a float conversion or a second
full token scan. Unknown fields are still lexically validated and skipped; a grammar-valid unselected `1e999` is not
a typed conversion failure. String contents are never interpreted as numbers.

A selected decimal token denotes an exact rational number. Round once to the
nearest destination binary32/binary64 value, halfway to an even significand.
Do not parse f32 via f64 and then cast. A nonfinite rounded value rejects;
underflow may round to subnormal or signed zero, preserving the token's sign.
Finite decimal input just above the maximum finite value may still round to
that finite value and succeed. This is a conversion range rule, not a claim
that the original exact decimal must equal a machine value. Integer conversion
retains its existing signed/unsigned range checks, without a float intermediary.
The host rounding mode must not change this result.

Encode accepts all finite bit patterns, including subnormal and signed zero.
Finite spelling preserves the existing float formatter: use the shortest decimal
significand that rounds to the same source-width bits; among equal-length choices
use the closest decimal value, ties to an even last decimal digit. Expand to
fixed-point decimal with no exponent, no leading plus, and no redundant leading
integer zeros. Include `.0` for integral values; zero is `0.0`, negative zero
`-0.0`. Do not first widen f32 to f64 for formatting. This is Align's existing
numeric spelling, not RFC 8785 canonicalization. Object keys stay in declaration
order, arrays in index order, unions emit their live payload, and None object
fields are omitted. String escaping is unchanged.

JSON-only float writers check finiteness before invoking the formatter. Ordinary
`print`, templates, arithmetic, casts and their IEEE behavior are unchanged.

## 3. Failure order and output ownership

JSON input/output is length-delimited UTF-8, never a C NUL-terminated string.
An embedded U+0000 in an encoded string is escaped by the existing JSON string
writer; raw NUL/C0 in JSON input is invalid, while escaped U+0000 remains valid
string content. Existing invalid-UTF-8/escape handling and cleanup are unchanged.
The codec has no I/O side effect; no partial text or target becomes public before
success. The only new wire validation is numeric grammar/conversion below.

Compile-time order: capability -> arity -> stable source place/liveness -> root
route and complete schema in its existing traversal order -> optional limit type
-> complete call borrow check -> expected result compatibility. Already-invalid
subtrees follow existing no-cascade rules and cannot mint checked encoder HIR.
The compiler reserves the source owner and transitive input views across all
eager arguments. A limit expression that consumes/replaces their backing storage
must reject, even if the old implementation happened to load pieces early.

At runtime, arguments are evaluated once before encoding. A negative limit makes
an initialized failed builder without payload allocation; no encoding traversal
occurs. Otherwise emit in declaration/array order. The first nonfinite leaf or
first over-limit emitted byte sticks as Invalid. No later write allocates or
resumes the encoder. Zero cap rejects every admitted root. Exact fit succeeds;
all cap counts are UTF-8 bytes, including escaped syntax. Both failure causes
have the same public Error, so there is no field-path error allocation or second
prevalidation pass. Earlier successful writes remain private until finalization.

The builder header is a nonescaping stack slot; its grow buffer is heap storage.
Success transfers ptr/length to exactly one String owner and disarms builder
cleanup. Failure frees payload, writes null/0 to the admitted output slot and
returns status Invalid. MIR constructs ordinary Ok/Err CFG; only Ok loads/moves
the output into Result. A discarded Result, replacement, returned Result and
`?`/`else`/`map_err` all use the existing recursive cleanup. Success has no input
or arena provenance, including when the source is borrowed or arena-backed.

## 4. Owned graph V3 and exact interface transition

Implementation must follow the byte contract and golden vectors below; no
compatibility shim or second flat/recursive route is retained.

```text
Record  := nonempty natural-layout record with Field+, no cycle
Field   := Value
Value   := Int | Float | Bool | string | Record | Option<Payload> | array<Element>
Payload := Int | Float | Bool | string | Record | array<Element>
Element := Int | Float | Bool | string | Record
Int     := i8 | i16 | i32 | i64 | u8 | u16 | u32 | u64
Float   := f32 | f64
```

The route is selected only by at least one transitive owned string. No owning
text -> existing route. Existing mixed `str`, nested Option, array-of-Option,
nested-array, resource, explicit-layout, cycle and depth exclusions remain.
Float leaves do not increment constructor depth or create a Drop obligation.

### 4.1 Canonical bytes

All integers are unsigned little-endian at the stated width. This is a private
compiler format, not a user JSON schema. Record numbering is root-first DFS in
source-field order, reusing the first ordinal for a completed DAG node; active
revisits reject cycles. Names are absent from structural record identity except
field names, which are JSON keys. Every record has natural layout: descending
field alignment, stable declaration-order ties, aligned offsets and final size.

```text
descriptor := u8 version=3, u8 layout_mode=0, u8 layout_algorithm=1,
              u32 record_count>0, u32 root_ordinal=0, record[record_count]
record := u32 size, u32 align, u8 owns, u8 drop, u32 field_count>0, field[field_count]
field := u32 name_len, u8[name_len] ASCII_identifier, type_node, u32 offset
type_node := u8 tag, u32 size, u32 align, u8 owns, u8 drop, payload

0x01 Int:    payload u8 bits in {8,16,32,64}, u8 unsigned in {0,1}; owns/drop=0/0
0x02 Float:  payload u8 bits in {32,64}; size=align=bits/8; owns/drop=0/0
0x03 Bool:   empty payload; size=align=1; owns/drop=0/0
0x10 String: empty payload; size=16, align=8; owns/drop=1/1
0x20 Record: payload u32 record_ordinal; cells exactly match referenced record
0x21 Option: payload u32 tag_offset=0, u32 payload_offset, type_node payload
0x22 Array:  payload u8 element_drop_version=1, type_node element

drop 0=Copy, 1=String, 2=owning Record, 3=owning Option, 4=owning Array
```

Int size/alignment = bits/8. An Option payload is aligned after its one-byte tag;
size is aligned payload-end and alignment is payload alignment. Its owns/drop
is 0/0 for Copy payload, 1/3 otherwise. Array header size/alignment = 16/8 and
owns/drop = 1/4 even for float elements. Record owns/drop = 0/0 if Copy, 1/2
otherwise. Record references must exactly match their target's size/alignment/
ownership/Drop. Depth counts records/Option/Array constructors exactly as plan 25
(root at 1, max 128). Node tags and nested payload rules are exhaustive.

Replace descriptor/envelope V2 with V3 and interface `FORMAT_VERSION` **11 -> 12**
in the implementation commit. All exported accepted roots, including graphs
without float, use V3. Do not retain V2 readers or version-dependent graph paths.

```text
entry := u32 root_name_len, u8[root_name_len] ASCII_identifier,
         u32 envelope_len, u8[envelope_len] envelope
envelope := u8 version=3, u32 triple_len, u8[triple_len] target_triple_ASCII,
            u8 object_format (0=ELF, 1=MachO),
            u8 endian=0, u8 pointer_size=8, u8 pointer_align=8,
            u8 bool_size=1, u8 bool_align=1,
            u8 string_size=16, u8 string_align=8,
            u8 array_size=16, u8 array_align=8,
            u8 option_tag_size=1, u8 option_tag_align=1,
            u8 f32_size=4, u8 f32_align=4, u8 f64_size=8, u8 f64_align=8,
            u64 abi_hash_lo, u64 abi_hash_hi,
            u32 descriptor_len, u8[descriptor_len] descriptor
```

ABI hash is the existing two-lane `Hash128::of` (wyhash seeds
`0x9e3779b97f4a7c15` and `0xc2b2ae3d27d4eb4f`) of the envelope prefix from version
through f64 alignment inclusive. This identifies target layout, not authenticity.
Target triple is the nonempty, NUL-free ASCII canonical LLVM target string used
by the compilation. ELF/MachO and all cells must match that target. No new target
support is implied: the driver retains its admitted 64-bit little-endian targets.
These vectors cover representative Linux x86_64/aarch64 and macOS arm64 triples. The golden triples below are fixed test inputs, not OS-release queries.

The graph list stays in its current interface position and is sorted by exported
local root name, one entry per accepted non-generic exported root. Root association
is nominal within its unit. Descriptor identity is structural over the complete
reachable definition graph, including float widths. Generic monomorphs/private
roots construct the same V3 graph in their consumer and fingerprint it in MIR.

Validation order is fail-closed: interface version -> list bounds/root names/
ordering -> envelope bounds/version/triple/format -> all ABI cells in encoded
order -> ABI hash and actual target match -> descriptor bounds/version/header ->
record/field/node cells in encoded order -> exact end -> reference bounds,
DFS/DAG/cycle/depth, semantic layout/Drop equality -> exact root-list cardinality.
Every count/offset/length calculation is checked before indexing or allocating.
Unknown tag/width, wrong float alignment/owns/drop, stale version, partial nested
node, duplicate names, trailing bytes or omitted root rejects. A malformed
interface cannot silently fall back to re-deriving its graph. Cache reads use the
existing rejection/miss policy before trusting cached artifacts; no stale object
may bypass validation.

The enclosing interface hash and frontend/per-unit schema inputs consume version
12. Keep key/container format versions unchanged where their byte layout is
unchanged. Codegen identity includes the complete new MIR graph/operation,
compiler build identity and matching runtime artifact. Private-body edit/hit/
restoration and reachable f32/f64 field changes get separate cache owners.

## 5. Compiler representation and native ABI

### 5.1 One checked encoder

Replace HIR `JsonOwnedEncode`, `JsonEncodeBounded` and
`JsonOwnedEncodeBounded` with:

```text
JsonEncode { base: LocalId, plan: JsonEncodePlan, max_bytes: Option<Box<Expr>> }
JsonEncodePlan = Pieces(Vec<TemplatePart>) | Owned(OwnedJsonGraphPlanV3)
```

Unbounded is None, bounded is Some(exact i64 expression). Both produce exact
`Result<String, builtin Error>`. Ordinary `Template` remains only the surface
template operation; `json.encode` never lowers through its infallible lifetime
path. `JsonOwnedDecode` keeps its discriminator with a V3 stored graph. Rename
V2 graph plan/record/field types to V3 atomically; no compatibility aliases.

Checked HIR independently reconstructs the source root domain, plan and limit
mode. Pieces must exactly match the canonical source plan, not merely form some
JSON-looking syntax; Owned must equal the reconstructed V3 graph. Each plan
borrows the same base; no field may introduce unrelated roots. Validate result,
source liveness, exact Error identity, limit type and all eager operands before
lowering. The operation is Pure (joined with argument effects), output storage is freshly owned/Static, and source
borrow retention lasts only through the call. Visit limit once and all plan
expressions through existing iterative traversal; do not use wildcard region or
move-analysis defaults for this discriminator.

Replace MIR `JsonEncodeBounded` with:

```text
JsonEncode { pieces: Vec<TemplatePiece>, max_bytes: Option<Operand>, out: Slot }
```

It yields i32 status; `out` is an exact String slot. None lowers to native
mode=0/max_bytes=0 (no cap); Some to mode=1 and the already-evaluated i64 cap.
Unbounded mode must retain the existing terminal buffer-size overflow policy;
using i64::MAX as a bounded sentinel would incorrectly turn it into Invalid.
Owned plans lower to one `OwnedJsonObject` piece with V3; existing nonowned
plans retain their validated pieces. MIR constructs Ok/Err, source ownership and
output nulling. LLVM only lowers checked pieces, loop/branch operations and native
calls. Remove the old hidden encoder-result String owner and arena operand.

Every emission entrypoint rejects malformed MIR: non-i32 result, non-String out,
wrong limit type, invalid V3/field/union/array/float shape, wrong operand types,
raw unescaped string holes or Char holes in a JSON encoder. Source schema/plan
validation remains at HIR; LLVM's structural checker must not claim to reconstruct
an absent original source root. Ordinary Template HIR/MIR rejects JSON-only
pieces (JSON escaping, optional JSON fields, descriptor objects/arrays/unions
and comma removal); those pieces are legal only inside JsonEncode. Text and
ordinary primitive holes remain valid template pieces. This prevents forged
infallible JSON transport.
Float byte emission selects JSON-only writers in direct/optional pieces and
runtime descriptor/union/array writers; no path calls the ordinary float writer
while claiming successful JSON.

The output slot is dedicated dead/uninitialized String scratch, never an already
live String owner. MIR producer/liveness validation enforces that precondition;
initializing an output cannot overwrite and leak an existing owner.

#### JSON piece-sequence validation

All MIR emission entrypoints also run one iterative, backend-independent JSON
piece-sequence validator before producing artifacts. Per-piece type safety alone
is insufficient: the baseline `bounded_json_piece_is_type_safe` accepts arbitrary
Static/PopComma, including a mismatched closing bracket. Do not carry that
acceptance into JsonEncode. LLVM may call the same pure MIR structural predicate;
it does not duplicate JSON semantics in emission.

Treat adjacent Static runs as one logical byte stream without requiring a new
concatenated buffer. Maintain an explicit container/token stack and accept exactly
one complete JSON value, with no trailing token, unmatched bracket, malformed
key/escape/number/keyword or missing/doubled separator. Ordinary value holes
(int, finite float, bool, quoted-and-escaped JsonStrHole, and validated descriptor
objects/arrays/unions) are atomic complete value tokens, legal only at a value
boundary. A hole cannot continue a static string/key/number/keyword. Static
member names and OptionField/OptionStructField names must be valid source ASCII
identifiers, unique in their object, including every potentially present optional
field. Validate descriptor-produced internal keys through the graph predicate.
A well-formed renamed key is detected by HIR's canonical source-plan comparison;
MIR does not claim to recover an absent source declaration.

OptionField/OptionStructField are legal only at an object member boundary. Model
both outcomes: None emits nothing; Some emits a complete named member and a
trailing comma. Required fields in that object use the same trailing-comma
protocol. Merge equivalent parser states after each optional piece; track the
possible empty-object/after-comma states, not 2^N complete output strings.
PopComma is permitted exactly once at the end of an optional-bearing object,
with no intervening emitted bytes before its matching `}`. Every possible prefix
must then end in the object's opening `{` or a removable literal `,`; whitespace
after a pending comma cannot make that comma removable. PopComma in an array,
inside a token, after a bare value, duplicated, or without the corresponding
optional-object protocol rejects. No optional presence combination may leave a
trailing comma, omit a needed separator or consume a parent object's separator.
Use the existing source-formed optional trailing-comma protocol; this adds no IR
variant, user syntax, runtime pass or output allocation.

T1 includes mutations of Static closing/opening delimiters, colon/comma/token
order, truncated/invalid key escapes, invalid/duplicate optional names, atomic
holes inserted inside a token, and missing/extra/misplaced PopComma. Run them
against every emitter, including an optional-object nested in a fixed root array.
Positive controls combine required fields with zero/one/multiple optional fields
in every presence combination and split Static runs across token boundaries.
The complete source schema remains HIR's responsibility; this MIR gate proves
well-formed output structure and the optional protocol for checked pieces.

### 5.2 Exact native rows

Reuse the existing 64-byte, 16-aligned stack builder header. Its private Rust
layout is constrained by a compile-time size/alignment assertion; it is not a
persisted format. Replace the two bounded-specific native symbols and keys;
add two JSON-only finite float writers. All rows are nounwind with no parameter,
return, purity or memory attributes.

| Key | Native symbol/signature | Registry shape | Semantics |
| --- | --- | --- | --- |
| `JsonBuilderInit` | `ptr align_rt_json_builder_init(ptr header, i32 mode, i64 max_bytes)` | new A127 | mode=0/max_bytes=0 is unbounded; mode=1 supplies the cap. Initialize a no-arena grow buffer with zero capacity. A negative bounded cap initializes sticky Invalid without payload allocation. Return header. |
| `JsonBuilderFinish` | `i32 align_rt_json_builder_finish(ptr header, ptr out_string)` | A128 | Consume valid initialized header; 0 transfers buffer, 2 (AL_INVALID) frees failed payload and publishes null/0. |
| `JsonBuilderWriteF32` | `void align_rt_json_builder_write_f32(ptr header, f32 value)` | A129 | On active builder, nonfinite marks sticky Invalid; finite uses source-width spelling. Already failed is a no-op. |
| `JsonBuilderWriteF64` | `void align_rt_json_builder_write_f64(ptr header, f64 value)` | A130 | Same at binary64. |

The old `BuilderInitBoundedStack`/`align_rt_builder_init_bounded_stack` and
`BuilderFinishBoundedStack`/`align_rt_builder_finish_bounded_stack` disappear.
Ordinary builders/templates retain their existing symbols and semantics.
The runtime constant `AL_INVALID` is exactly 2 on the assessed baseline; it maps
to the existing builtin `Error.Invalid`. Decoder conversion status remains 1.

A valid init destination is unique compiler-reserved 64-byte storage aligned to
16, without a live Builder. Finish and writers require that storage to contain
the initialized live Builder. Output is distinct writable 16-byte String scratch
aligned to 8, with no live String owner. It is a separate compiler stack allocation,
disjoint from both the grow buffer and source storage, not merely a different
address within either. Allocation provenance and absence of a
live destination owner are caller ABI preconditions; native code does not read
uninitialized storage to guess them. Init validates header null/extent/alignment,
then mode in {0,1}, then max_bytes=0
when mode=0. Invalid admission returns null without writes. Mode=1 accepts any
i64 as a cap; negative initializes sticky Invalid. Valid generated stack slots
and constant mode satisfy admission by construction; malformed IR rejects before
emission. Init returning null is never a successful builder handle.
Finish checks both numeric extents/alignment and disjointness before consuming
header or writing output. Invalid output admission returns AL_INVALID and leaves
both regions unchanged so the owner can retry/free; this exception is distinct
from an admitted encoder failure, which consumes the builder. The two float
writers require a live initialized header; they are not public raw-pointer APIs.

Existing bounded scratch limits become sticky encoder failure, shared by all
JSON writes. An admitted finish always initializes out to null/0 before success
transfer. Failed payload cleanup precedes publishing status. Subsequent generated
reads/writes cannot create side effects after sticky failure; descriptor array
loops stop on the failed flag. There is no field-path allocation or global state.
The new helpers share the existing formatter; no additional dependency or library
link capability is introduced.

Baseline inventory is 389 keyed / 407 base / 414 alloc-count / 411 par-map-probe /
418 maximum exports. Replacing two keys and adding four yields **391 / 409 / 416 /
413 / 420**. New A127 has ptr return and [Ptr, I32, I64] parameters, nounwind
only and no other attributes; it does not reuse A45's allocator noalias attributes.
A128/A129/A130 retain A19/A64/A63's physical signatures with nounwind. Source
inspection during implementation found those baseline shapes have no nounwind
attribute; distinct registry shapes preserve the reviewed attribute contract
without changing unrelated declarations or any public ABI signature. Declaration goldens, source/export sweep,
feature counts, default attrs and all registry type-matrix owners must agree.

Existing `JsonField`, `JsonSubTable`, `JsonUnion` layouts and decoder signatures
are unchanged. V3 float fields use kind=2, width=4/8, signed flag clear; optional
fields carry existing opt_tag; float arrays use kind=7, elem-kind=2,
elem-width=4/8, signed flag clear and 16-byte ptr/count header. HIR/interface and
LLVM validators independently enforce these combinations before pointer access.

## 6. Independent golden vectors

The implementation embeds these fixed vectors in its owner tests. Expected bits
were checked with exact decimal rationals and nearest/ties-even rounding,
independently of Rust's parser. Finite bytes were independently checked by
searching shortest decimal significands with §2's nearest/tie/fixed-point rules.
Descriptor bytes were independently decoded into the full declared graph/layout;
three envelope hashes were checked by independent Python wyhash and the existing
Rust hash primitive. A production round trip alone is not an independent oracle.
This is design evidence, not a test of future implementation code.

### 6.1 Numeric vectors

Hex is the target IEEE bit pattern, not host-endian memory. `reject` means typed
`Error.Code(1)` at that target width; only f64 reject rows imply doc accessor
None (an f32 overflow token can be finite f64). Finite byte columns contain the complete
number token without surrounding field syntax.

| Width | Decimal input | Expected bits | Encoded token |
| --- | --- | --- | --- |
| f32 | `0` | `00000000` | `0.0` |
| f32 | `-0` | `80000000` | `-0.0` |
| f32 | `0.3` | `3e99999a` | `0.3` |
| f32 | `1.000000059604644775390625` | `3f800000` | `1.0` |
| f32 | `1.0000000596046448` | `3f800001` | `1.0000001` |
| f32 | `16777217` | `4b800000` | `16777216.0` |
| f32 | `3.4028234663852886e38` | `7f7fffff` | `340282350000000000000000000000000000000.0` |
| f32 | `3.4028235e38` | `7f7fffff` | `340282350000000000000000000000000000000.0` |
| f32 | `3.4028236e38` | `7f800000` | reject |
| f32 | `1.1754943508222875e-38` | `00800000` | `0.000000000000000000000000000000000000011754944` |
| f32 | `1.401298464324817e-45` | `00000001` | `0.000000000000000000000000000000000000000000001` |
| f32 | `7e-46` | `00000000` | `0.0` |
| f32 | `8e-46` | `00000001` | `0.000000000000000000000000000000000000000000001` |
| f32 | `-1e-999` | `80000000` | `-0.0` |
| f32 | `1e999` | `7f800000` | reject |
| f64 | `0` | `0000000000000000` | `0.0` |
| f64 | `-0` | `8000000000000000` | `-0.0` |
| f64 | `0.3` | `3fd3333333333333` | `0.3` |
| f64 | `1.00000000000000011102230246251565404236316680908203125` | `3ff0000000000000` | `1.0` |
| f64 | `1.00000000000000011102230246251565404236316680908203126` | `3ff0000000000001` | `1.0000000000000002` |
| f64 | `9007199254740993` | `4340000000000000` | `9007199254740992.0` |
| f64 | `1.7976931348623157e308` | `7fefffffffffffff` | `179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0` |
| f64 | `1.7976931348623158e308` | `7fefffffffffffff` | `179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0` |
| f64 | `1.7976931348623159e308` | `7ff0000000000000` | reject |
| f64 | `2.2250738585072014e-308` | `0010000000000000` | `0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000022250738585072014` |
| f64 | `4.9406564584124654e-324` | `0000000000000001` | `0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005` |
| f64 | `2e-324` | `0000000000000000` | `0.0` |
| f64 | `3e-324` | `0000000000000001` | `0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005` |
| f64 | `-1e-999` | `8000000000000000` | `-0.0` |
| f64 | `1e999` | `7ff0000000000000` | reject |

Malformed token vectors: `+1`, `01`, `-01`, `1.`, `.1`, `1e`, `1e+`, `--1`,
`NaN`, `Infinity`, `-Infinity`, `0x1`. Wrong kinds for a required float: `null`,
`true`, `"0.3"`, `[]`, `{}`. `01`/`-01` reject in ignored fields and json.doc;
valid ignored `1e999` remains skippable. Optional missing/null are None; a
present malformed value is an error. Encoder negative controls include both
signs of infinity, quiet NaN and signaling NaN at both widths. Some(nonfinite)
rejects; None is omitted. Never substitute null, a string or zero for nonfinite.

### 6.2 Descriptor and envelope vectors

Source declarations (separate from call expressions):

```align
NumericLeaf { flag: bool, weight: f32, note: string }
NumericRoot {
  label: string
  direct: f64
  small: f32
  optional: Option<f64>
  samples: array<f32>
  child: NumericLeaf
}
```

Root ordinal 0 has size/alignment 88/8 and source-order offsets 0,16,80,24,40,56.
Leaf ordinal 1 is 24/8 with offsets 20,16,0. Both own/drop cells are 1/2.
Option<f64> is 16/8, Copy, tag/payload 0/8. Array<f32> is 16/8, owns/drop 1/4,
element-drop version 1. The exact 300-byte descriptor follows; whitespace is not
encoded.

```text
03000102000000000000005800000008000000010206000000050000006c6162
656c101000000008000000010100000000060000006469726563740208000000
080000000000401000000005000000736d616c6c020400000004000000000020
50000000080000006f7074696f6e616c21100000000800000000000000000008
000000020800000008000000000040180000000700000073616d706c65732210
0000000800000001040102040000000400000000002028000000050000006368
696c642018000000080000000102010000003800000018000000080000000102
0300000004000000666c61670301000000010000000000140000000600000077
656967687402040000000400000000002010000000040000006e6f7465101000
000008000000010100000000
```

Each exact envelope is `prefix || hash || 2c010000 || descriptor`, all operands
hex bytes. Hash contains two u64 lanes already in little-endian wire order.
The entry prepends u32 root-name length 11, ASCII `NumericRoot`, and u32 envelope
byte length. List count/order remain §4's contract.

| Target | Prefix bytes | Hash bytes |
| --- | --- | --- |
| `x86_64-pc-linux-gnu` | `03130000007838365f36342d70632d6c696e75782d676e7500000808010110081008010104040808` | `9cb2b1ae7d032143a2ea80fcee9f4d39` |
| `aarch64-unknown-linux-gnu` | `0319000000616172636836342d756e6b6e6f776e2d6c696e75782d676e7500000808010110081008010104040808` | `610dda274edf542a039411b801ab69a1` |
| `arm64-apple-darwin` | `031200000061726d36342d6170706c652d64617277696e01000808010110081008010104040808` | `0d9d000a2668715469522fc0941c697e` |

Negative vectors independently mutate every scalar/tag/width/owns/drop/layout/
offset and length/count; truncate each nested-node boundary; add duplicate fields/
roots, unreachable/out-of-order/active-cycle references, depth 129, trailing data
and V2. A forward reference at its next first-use DFS ordinal is valid.
Recompute envelope/container integrity where needed so integrity rejection does
not mask semantic validation. Assert the complete decoded semantic record,
not merely byte equality after a self-round-trip.

## 7. Capability and caller migration

Ship one coherent R63 capability: direct target-width parsing, finite-only
fallible encoding, owned float graph V3, interface 12 and caller migration.
Expect more than 1,000 handwritten changed lines: the result type crosses all
ownership analyses and emission routes, and graph producers have no useful
stable consumer without matching validators/runtime. One boundary proves error/
ownership transport once and avoids an intermediate infallible or stale-graph
route. Do not split by compiler directory. R65 does not precede or join it.

Implementation order inside this capability:

1. Embed fixed numeric/byte oracles in existing owner targets; implement shared
   target-width conversion and finite writers, preserving integer fast paths.
2. Add Float structurally to sema and independent HIR/interface/LLVM validators;
   switch graph/envelope to V3 and interface to 12 atomically.
3. Introduce the single HIR/MIR JsonEncode, Result CFG, native rows and ownership
   sweeps. Remove old variants/symbols and infallible JSON routing.
4. Migrate Align callers/assertions; run §8 checks, §9 measurements, self-review,
   one fresh code review and ordinary local PR gates.
5. Record provider delivery once in HANDOFF and the sibling register. At request
   batch completion run exactly `cargo build --release --workspace`; this is not
   a versioned release or consumer adoption.

An old `encoded := json.encode(row)` becomes `encoded := json.encode(row)?` in a
Result-returning function. A helper returning str becomes `Result<string, Error>`
and returns the encoder Result directly. Use match/else for handled failures.
Borrow the successful string for str consumers; remove a clone used solely to
make the former encoder view escape, retaining copies needed by other uses.
Do not invent a silent fallback. Bounded callers retain their signature.

Call examples using the separately declared NumericRoot:

```align
import core.json

fn encode_row(row: NumericRoot) -> Result<string, Error> = json.encode(row)
fn encode_small(row: NumericRoot) -> Result<string, Error> = json.encode_bounded(row, 4096)
```

Inspect all callers with `rg -n 'json\.encode\(' examples apps bench crates docs`,
including Align source embedded in Rust tests. Baseline executable examples are
`examples/json.align`, `examples/json_decode.align`, `examples/json_nested.align`;
`apps/web/pkg/web.align` has a stale comment example. Existing `m5`, owned/bounded,
module, cache and LLVM suites also assert the former result. Migrate these in the
implementation. The implementation migrates these executable examples and tests. The external align-llm implementation/adoption is consumer-owned; only its
request register is writable here, left uncommitted.

## 8. Implementation closure and acceptance owners

The matrix reuses parameterized existing owners where they detect the invariant.
Before code review every applicable row must point to implementation and passing
coverage. Numeric converter tests are in `json_number::tests`; native owners are
in `align_runtime::r63_tests`.

| ID / invariant and applicable cases | Implementation owner | Acceptance owner |
| --- | --- | --- |
| N1: exact bits/bytes, single f32 rounding, finite limits, signed zero/subnormal, host rounding independence | Runtime span/conversion helpers and finite writers | `align_runtime::r63_tests::r63_numeric_vectors`; halfway negative control fails under f64-then-cast; ordinary print/template nonfinite behavior unchanged |
| N2: invalid grammar/kinds/leading zero, ignored huge token, optional absence, exact integer range | JsonParser integer/number/skip, doc tokenization/accessor | `align_runtime::r63_tests::r63_numeric_grammar_and_doc`; reuse string-grammar owner and u64::MAX/both-neighbor range controls |
| N3: scalar/scalar-array, direct/nested/optional/union/owned fields, AoS strict/speculative/fallback, SoA, scanner at both widths | write_value and every decoder/accessor dispatch | Existing runtime `json` owners force learned/fallback patterns; `m5::r63_numeric_public_routes` uses only each route's admitted schemas |
| N4: late malformed/trailing/range error after strings/rows/arrays; no partial result | Owned staging/drop and AoS/scanner failure cleanup | Extend `m5_owned_json::owned_json_result_transfer_control_flow_replacement_and_drop_matrix`; `align_runtime::r63_tests::r63_decode_failure_allocation_parity` includes omitted-cleanup negative control |
| E1: every existing root and float leaf route, finite/nonfinite, None omission; same schemas for both APIs | Canonical sema plan, MIR pieces, runtime writers | `m5_json_bounded::r63_encoder_route_matrix`; unchanged excluded shapes remain negative controls |
| E2: negative/zero/exact/one-short cap; UTF-8/escapes/empty/Some/None; early/late nonfinite | Init/grow/sticky writes/finish | Extend `json_encode_bounded_limit_failures_are_invalid` and `json_encode_bounded_exact_fit_matches_json_encode`; `align_runtime::r63_tests::r63_builder_failure_matrix` proves sticky failure/no later growth; existing runtime `bounded_stack_builder` owners prove zero allocation for negative cap and capacity <= cap |
| E3: multi-invalid diagnostic order, non-i64 cap, result mismatch, moved source or backing views consumed/replaced by cap | Sema, generic substitution, borrow reservation | Extend `owned_json_formation_routing_and_multi_invalid_precedence_are_deterministic`, `owned_json_encoders_reject_moved_sources`; `m5_json_bounded::r63_limit_source_borrow_reservation` covers nested/array/text views |
| E4: negative cap skips encoding traversal; failure stops descriptor loops; deterministic successful bytes | MIR evaluation/branches and runtime failure state | `r63_builder_failure_matrix`, `r63_failed_encoder_stops_descriptor_traversal` and `r63_encoder_route_matrix`; the descriptor owner pins zero vs one million leaf visits |
| E5: native null/misaligned/overflowing/overlapping admission before writes or consumption | JSON builder native entrypoints | `align_runtime::r63_tests::r63_builder_native_admission`: sentinel preservation, invalid mode/nonzero unbounded payload, retry/free after rejected finish; no dereference of fabricated allocation provenance |
| O1: formation/construct/move-in/out/nulling/replace/Drop/return; input and arena may die before output | All HIR ownership/effect/region analyses, MIR Result CFG/String and graph Drop | Extend `m5_json_bounded::json_encode_bounded_owned_result_composes_with_existing_result_control_flow` for both APIs; `m5_owned_json::r63_encoded_result_ownership` |
| O2: if/match/else/?/map_err, branch/loop joins, early return, discard Ok/Err; generic/imported/whole/per-unit | Variant visitors, substitutions, interface reconstruction, MIR liveness | `m5_owned_json::r63_encoded_result_ownership`; extend bounded whole/per-unit owner; input borrow ends at call, output stays independent |
| O3: no final copy, exactly-once free on all outcomes, simultaneous calls/global-state independence | Builder transfer, recursive cleanup, allocation probes | Runtime `r63_builder_native_admission` pins grow-ptr == output-ptr; retain `stack_builder_header_finish_transfer_and_unfinished_drop`; allocation parity owners; reuse `all_json_operation_variants_overlap_in_the_full_153_pair_matrix` after API migration |
| G1: Float scalar/Option/array/record, with/without owning text; depth/cycle/exclusions | Producer and independent graph predicates | Extend `m5_owned_json` formation/generic/C6 owners; no widening of fixed arrays, array<Option>, mixed views or other containers |
| G2: all exact bytes/hash/target; independent semantic/byte oracles; malformed graph rejected before use | Interface codec, source derivation and target envelope | `align_interface::owned_json::tests::r63_graph_v3_golden_and_rejection`, three target vectors and full decoded graph equality |
| G3: version 12, V3 root lists, reachable float-width changes, generic/private/imported, cold/hit/edit/restore | Interface hash, MIR serialization/fingerprint, unit/codegen cache | `align_driver` `unit_cache` and `cache_codegen` targets: `r63_json_identity_matrix`; no stale artifact after version/target/width change |
| T1: forged result/base/plan/limit/type/effect/region/tables and Static/key/separator/optional/PopComma sequence; no infallible route; all lowerers/emitters | Variant-sweep tripwire, checked HIR, MIR traversal/hash/print, LLVM validation | MIR `owned_json_checked_hir_mutation_sweep_fails_closed_at_all_lowering_entrypoints` checks sema replay and every HIR lowerer; `hir_body_validator_pipeline_template_json_group` covers canonical pieces; `r63_sequence_delimiters_tokens_and_optional_protocol` validates sequences; LLVM `r63_json_sequence_is_checked_by_every_body_emitter` covers all body emitters and ordinary Template rejection |
| T2: exact ABI names/counts/shapes/attrs across feature combinations; ordinary templates unchanged | Native registry/declarations/exports | Existing `runtime_abi_extern_type_matrix_is_exact_for_every_row_and_ordinal`, declaration golden, source/export/feature inventory owners |
| P1: ordinary throughput, owned output transfer, bounded storage | Lexer/conversion/writers/buffer transfer | §9 local measurements; O3/E2 correctness checks independently prove allocation and capacity claims |

Focused owner commands:

```text
scripts/cargo.sh test -p align_runtime --lib r63_
scripts/cargo.sh test -p align_interface --lib owned_json
scripts/cargo.sh test -p align_mir --lib json
scripts/cargo.sh test -p align_codegen_llvm --lib r63_
scripts/cargo.sh test -p align_driver --test m5_owned_json --test m5_json_bounded
scripts/cargo.sh test -p align_driver --test m5 r63_
scripts/cargo.sh test -p align_driver --test unit_cache --test cache_codegen r63_
```

Also run affected existing ABI/transfer owners in their actual enclosing targets,
and sweep old JSON discriminants/keys. A filter matching zero tests is not proof.
Then use the final-candidate owner, bounded gate and Clippy via `scripts/pre-pr.sh`
without duplicating broad suites. Keep the 30-minute suite/15-minute binary budget.
This capability requires no new service dependency or CI job.

### Numeric implementation selection

The author-side host-rounding owner found that Rust `FromStr`'s native
multiply/divide fast path changes `0.3` bits under directed rounding. The shared
conversion therefore uses pinned `minimal-lexical = 0.2.1`, without default or
allocator features: validated borrowed digits feed its integer Eisel-Lemire path
and bounded stack-bigint fallback directly. The adapter never invokes its native
floating fast path, changes floating environment state, or allocates token text.
Long-significand/exponent and four rounding-mode owners supplement the independent
bits/bytes vectors. This selects the implementation of the existing N1 contract;
no public numeric or ownership rule changes.

## 9. Performance acceptance

Preserve performance; this design has not yet proved zero slowdown. Compare the
immutable baseline above with the final implementation on the same host/profile/
input/workload. Prepare artifacts in separate directories outside both checkouts.
Existing `bench/json_decode/run.sh prepare native` / `native` and
`bench/json_soa/run.sh prepare native` / `native` cover full/projection/AoS/SoA.
Set existing `ALIGN_BENCH_WORK_DIR`, then verify the prepare-produced
`ALIGN_BENCH_ARTIFACT_MANIFEST_SHA256` when running native. Preparation/build is
outside timed execution. Preserve raw outputs and artifact/input identities.

Add one local numeric benchmark beside these harnesses for f32/f64 direct decode,
skip-heavy decode and finite encode. Use 0.3, integral, signed zero, normal,
subnormal and large-exponent values with integer-only/text-heavy controls. Compare
the same values/bytes in borrowed records and owned string/optional/array records.
Failure measurements are separate: baseline invalid JSON is not a valid success
comparator. For owned output compare new success with old encode plus the explicit
copy needed for equivalent owned bytes; report raw old encode separately so copy
removal cannot hide writer regressions.

Warm both variants, alternate baseline/candidate on an idle host, retain at least
nine pairs, and report median, spread, allocations and peak buffer storage. Reuse
existing harness iteration counts and make new inner runs exceed timer noise.
Collect additional pairs if noise obscures the result. There is no automatic
percentage allowance. Investigate and repair reproducible regressions without
relaxing grammar, rounding/range or ownership. Measurements are local performance
acceptance, not a flaky CI timing gate.

### Local acceptance evidence

[Recorded measurements](../../bench/json_numeric/r63-results.md) retain all nine-pair
medians/spreads and artifact identities. Normal decode/AoS/SoA/projection controls
show no reproducible regression. Final native f32/f64 decode medians are
24.443/25.578 ns versus 26.676/26.328 ns; mixed finite encoding is 580.432 ns versus
596.615 ns raw or 609.093 ns including the baseline owned copy. Exact output bytes
agree. Direct `fmt::Write` removes the I/O adapter without changing Display digits,
ordinary template/print behavior, allocation or error policy.

Separate resource processes observe 384 peak requested live bytes for the growing
buffer in either implementation, versus 485 for baseline plus its explicit owned
copy. Success transfers the same pointer; receiving-owner cleanup returns to zero.
No throughput threshold is added to CI. The report explains the local Linux
benchmark-loader workaround and distinguishes earlier unchanged-decoder controls
from the final changed-formatter measurements.

## 10. Source agreement, review and handoff

The amendment is synchronized in draft, condensed spec, design notes, JSON Settled
entry, English/Japanese JSON library and guide, plans 24/25, bounded plan 17,
HIR ledger 19, ABI ledger 20 and cache plan 10. Earlier V1/V2 vectors/history remain
explicitly historical; this ledger supersedes their float exclusion and encoder
result/transport rules. The current API and historical rollout records are distinguished explicitly. Plan 46
retains R65's unclosed qualification; this document closes only its JSON row.

Design author-side closure: complete. The ledger-to-prose/matrix pass checked source
domains, effects, input/output lifetimes, all failure states, mode/limit products,
V3 byte/target/cache identity, native rows and all applicable owner axes. Exact
rational checks cover 30 numeric vectors; independent full semantic decoding and
Python/Rust hashes cover the descriptor and three envelopes. All four new/changed
Align example blocks parse with the baseline formatter; future API typechecking
belongs to implementation. Added local links, fenced blocks, whitespace and the
English/Japanese amendment pairs pass. That design checkpoint changed no production code; implementation evidence is recorded separately below.
Independent exact-contract review: complete, inspection-only, one P2 and no P1.
The reviewer checked the full ledger, vectors, source feasibility and modified
specifications/mirrors. The finding was that per-piece MIR type checks accepted
arbitrary Static/PopComma sequences and could publish malformed JSON. §5.1 now
requires one iterative symbolic sequence validator across every emitter, with
exact token/key/separator/optional-field rules; T1 owns the matching mutations
and presence combinations. The author verified the whole cause class, including
nested objects, split static tokens and comma-removal adjacency. Native fresh
header/output allocation preconditions were also clarified and accepted during
review. No API, graph format or runtime strategy changed in resolving the finding;
no repeat full review was needed. This is the design review record, separate from code review and performance evidence.

Both design entries are closed. The implementation model can follow §§7–9 without
choosing another numeric/error/ownership policy. A discovery changing public
contract or safety strategy reopens the affected matrix axis before code. Ordinary
local corrections follow the existing one-review/one-fix implementation cycle.

The implementation preserves the reviewed contract and synchronized documents.
The sibling request-register edit remains uncommitted. R65 stays under plan 46.

### Implementation author pass

The complete source-fact and ownership visitor sweep replaces the old three
encoder variants with one operation. Both Results use ordinary String transfer,
replacement, cleanup and return paths. Numeric field dispatch is shared across
scalar/array, strict/speculative/fallback/AoS/SoA/scanner and document access;
integer conversion remains separate. Interface graph producers and independent
validators include the same float leaf positions and V3/12 identity.

The author-side boundary pass added source/backing-view reservation across the
limit, integer-only destination-width conversion independent of host rounding,
full MIR symbolic sequence validation, float export identity reconstruction,
and parameterized native admission/failure/allocation owners. These are local
implementation corrections before the fresh code review, not review rounds.
Focused evidence passes: eight runtime numeric/native/allocation owners with
`alloc-count`, four bounded-builder owners, ten MIR JSON validators, five interface
owned-graph owners, the all-body-emitter LLVM mutation owner and existing ABI
matrix/golden owners. Driver coverage passes for whole/per-unit numeric routes,
source/backing-view reservation, generic owned-result escape/transfer/control
flow, both cache identity owners and migrated JSON/borrow/template/XML owners.
The native compiled signature/export audit passes at 409 base, 416 alloc-count,
413 par-map-probe, 409 task-group-probe and 420 combined symbols. The author-side
extracted-obligation/matrix-to-diff pass is complete. Fresh code review and final
repository gates remain ordinary delivery requirements; no clean-review claim is
made by these owner results.
