# core.codec — columnar batch wire format

> 🌐 **English** · [Japanese](./ja/codec.md)
>
> **Status:** implemented 2026-09-01.

## Authoritative public-contract ledger

This table is the authority for the first `core.codec` capability. Later prose and implementation
may make a field more explicit but must not widen it. The format carries data batches, not calls,
services, executable plans, or arbitrary user types.

| Public surface | Exact inputs, validation, and evaluation | Exact result, errors, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner and identity | Acceptance owner |
|---|---|---|---|---|---|
| `codec.kind { I64, F64, Bool, Str }` | One closed builtin tag-only sum. Source ordinals and wire tags are exactly `I64=0`, `F64=1`, `Bool=2`, and `Str=3`. There is no numeric conversion, custom kind, nullable modifier, extension metadata, alias, or ambient registry. | Copy and Pure. Inspection uses the existing exhaustive closed-enum `match`; `==` is not defined for enums. No source-formed operation errors or aborts. | The settled one-field enum aggregate `{ i32 tag }`; no borrow, allocation, or Drop. | `align_sema` owns the unique builtin enum behind `import core.codec`; HIR/MIR keep the ordinary enum aggregate. Interfaces serialize its nominal named definition. The source ordinal is also the exact descriptor `kind` byte. | Import/type/variant positives, exact ordinals, wrong module/type negatives, checked-HIR enum identity and closed-tag validation, whole/per-unit identity. |
| `codec.open(input: slice<u8>) -> Result<codec.batch, Error>` | Exactly one positional argument. `input` is borrowed and evaluated once. A successful input is the complete canonical v1 envelope with at most 1024 columns. Its address may have any byte alignment. Validation uses no owned/heap allocation, is bounded by the supplied length and fixed column cap, and follows the exact precedence below before constructing a result or retaining a fact. Name uniqueness uses fixed 4096-byte stack scratch and a ten-pass bottom-up merge sort, not a quadratic scan. It rejects every unknown tag/flag/version, overflow, over-limit count, noncanonical offset/order/padding, duplicate/invalid name, malformed Arrow-layout buffer, invalid UTF-8 string cell, trailing byte, and positive-length null. | Pure. Success returns one validated batch view. Every detectable malformed input returns `Err(Error.Invalid)` and performs no owned allocation or mutation; there is no partial batch. Compiler-private dangling pointers remain outside the safe ABI precondition. | `codec.batch` is a Copy opaque view into `input`, carries its storage generation, and has exactly `region_of(result) = region_of(input)`. The result allocates and owns nothing. Moving, replacing, dropping, or mutably borrowing the input owner while the batch or a derived view remains live is rejected. | Sema forms the region-bearing builtin result; checked HIR proves the exact `slice<u8>`, `Result<codec.batch, builtin Error>`, and input-region fact. MIR calls one keyed validator and constructs the fixed batch scalar only after status zero. Runtime owns byte validation but no retained allocation. The capability, format version, native row, and new canonical type leaves enter interface/implementation/cache and runtime-ABI fingerprints. | Independent semantic-to-byte and byte-to-semantic goldens; every validation-precedence class; truncation at every byte; overflow/allocation-bomb prefixes; 1024/1025 column and 4096-byte scratch boundaries; common-prefix distinct/duplicate name work; every base alignment 0..7; no-owned-allocation success/failure; input mutation/escape/control-flow carriers; whole/per-unit parity; malformed checked HIR and ABI. |
| `b.rows() -> i64`; `b.columns() -> i64` | `b` is a validated `codec.batch`, borrowed read-only. | Pure total Copy counts. They perform no revalidation. | No allocation, retained borrow, or mutation. | `rows` lowers to an alignment-1 little-endian i64 bit load at envelope byte 16 plus any target-required byte swap. `columns` lowers to an alignment-1 little-endian u32 load at byte 24, any target-required byte swap, and zero-extension to i64. Neither private count is stored in the `{ input_ptr, input_len }` batch scalar. | Empty/nonempty/max-admitted counts, repeated calls, every base alignment, imported and carrier-held batches, little-/big-endian lowering owners. |
| `b.name(i: i64) -> Option<str>`; `b.kind(i: i64) -> Option<codec.kind>`; `b.find(name: str|string) -> Option<i64>` | Receiver, then argument evaluate exactly once. Negative or out-of-range `i` returns `None`. `find` performs byte-exact, case-sensitive name comparison in ordinal order and returns the unique first match; absence returns `None`. An owned `string` is auto-borrowed. | Pure and total. `name` is a zero-copy UTF-8 view; `kind` and the ordinal are Copy. No error or abort. | `name` and its `Option` carry the batch/input region. `find` retains nothing. No allocation. | Name projection and `find` reread validated descriptor offsets/lengths with alignment-1 little-endian loads and target-required swaps; kind reads its one byte. They lower to optimizer-visible operations and an ordinal-order name loop without typed descriptor pointers. There is no additional native ABI row, source/artifact I/O, or reflection. | Cartesian negative/in-range/upper-bound ordinals; empty/non-ASCII/NUL names; exact-case miss; ordered lookup; repeated access; every base alignment; input-region escape/invalidation; no revalidation/allocation. |
| `b.i64s(i: i64) -> Option<codec.i64_column>`; `b.f64s(i: i64) -> Option<codec.f64_column>`; `b.bools(i: i64) -> Option<codec.bool_column>`; `b.strs(i: i64) -> Option<codec.str_column>` | Receiver then ordinal evaluate once. A negative/out-of-range ordinal or a different `codec.kind` returns `None`; the exact kind returns a view of all rows. No accessor revalidates bytes. All four views admit every input base alignment. | Pure and total. All projections are zero-copy. `f64` preserves every IEEE bit pattern, including NaNs and infinities. There is no coercion between column kinds and no alignment-dependent unavailable state. | Every returned view and `Option` is Copy and region-bound to the batch input and its generation. No allocation, ownership transfer, or Drop. | Four distinct checked HIR/MIR projections reread the kind and validated data/aux descriptor fields with byte or alignment-1 little-endian loads, then produce four opaque `{ ptr, len }`-class view scalars. LLVM forms no typed descriptor or element pointer before ordinal/kind success. | Four-by-four kind/accessor product; negative/out-of-range product; all base alignments; exact numeric/NaN bytes; region carriers/control joins/return and mutation rejection; no runtime call in element loops. |
| `codec.i64_column`: `c.len() -> i64`; `c.at(i: i64) -> Option<i64>` | `c` is borrowed. `at` evaluates the index once and returns `None` outside `0..len`; otherwise it reads the exact eight little-endian bytes for row `i`. | Pure, total, Copy, allocation-free. | The scalar result has no region; the column view remains input-region-bound. | Access lowers to a bounds comparison, an alignment-1 i64 bit load, and target-required byte swap. No typed pointer is formed before bounds success. | Empty, every base/element alignment, negative/upper-bound, signed extrema, optimized/unoptimized and little-/big-endian lowering owners. |
| `codec.f64_column`: `c.len() -> i64`; `c.at(i: i64) -> Option<f64>` | `c` is borrowed. `at` evaluates the index once and returns `None` outside `0..len`; otherwise it reads the exact eight little-endian bytes for row `i`. | Pure, total, Copy, allocation-free. Every IEEE bit pattern is retained exactly. | The scalar result has no region; the column view remains input-region-bound. | Access lowers to a bounds comparison, an alignment-1 i64 bit load, target-required byte swap, and f64 bitcast. No typed pointer is formed before bounds success. | Empty, every base/element alignment, negative/upper-bound, infinities and fixed NaN payloads, optimized/unoptimized and little-/big-endian lowering owners. |
| `codec.bool_column`: `c.len() -> i64`; `c.at(i: i64) -> Option<bool>` | `c` is borrowed. `at` evaluates the index once and returns `None` outside `0..len`; otherwise it reads Arrow LSB-first bit `i`. | Pure, total, Copy, allocation-free. | The scalar result has no region; the column view remains input-region-bound. | Access lowers to a bounds comparison, byte load, shift, and mask. No address is formed before bounds success. | Empty, every base alignment, byte boundary, bool tail, negative/upper-bound, and optimized/unoptimized owners. |
| `c.len() -> i64`; `c.at(i: i64) -> Option<str>` for `codec.str_column` | `c` is borrowed. `at` evaluates the index once, returns `None` outside `0..len`, and otherwise uses validated adjacent i32 offsets to return exactly that UTF-8 cell. Empty strings and embedded NUL/LF are ordinary data. | Pure, total, zero-copy, allocation-free. | The `str` and its `Option` carry the original batch/input region and generation. | Checked string-column operations lower to one bounds comparison, two alignment-1 little-endian i32 loads with target-required byte swaps, and a view construction. `open` has already proved monotonic offsets, bounds, and per-cell UTF-8. | Empty/non-ASCII/NUL/LF cells; repeated offsets; every base alignment; negative/upper-bound; return/carrier/control region closure; optimized/unoptimized and whole/per-unit parity. |
| `codec.encoder(rows: i64) -> Result<codec.encoder, Error>` | Exactly one positional argument, evaluated once. A negative row count returns `Err(Error.Invalid)` before allocation. A nonnegative row count creates an encoder with zero columns. No ambient schema, allocator setting, file, clock, or target input participates. | Pure. Success returns one initialized encoder. OOM follows the language-wide hard-abort policy. | The returned Move handle owns one encoder shell and all later copied column/name staging. It retains no argument. Drop releases every staged byte exactly once and produces no output. | One nominal `Ty::CodecEncoder`/MIR scalar and one keyed runtime constructor/drop pair. Interface identity uses the named builtin type and existing Move return rules. | Negative/zero/positive rows; no-allocation error; one-shell allocation/free; direct/imported/function-value return and Drop; malformed HIR/ABI. |
| `e.put_i64(name, values: slice<i64>)`; `e.put_f64(name, values: slice<f64>)`; `e.put_bool(name, values: slice<bool>)`; `e.put_str(name, values: slice<str>)` — each `-> Result<(), Error>` | `e` is a bound mutable encoder and is not consumed. Receiver, name, then values evaluate once. `name` accepts `str|string`, must be nonempty valid UTF-8, fit u32 length, and be byte-unique among successful columns. At most 1024 successful columns are admitted. `values.len()` must equal `rows`. `put_str` additionally requires total copied cell bytes to fit signed i32. The complete candidate name, count, length, kind-specific sizes, final canonical length, and every string cell are checked before the first mutation. | Pure. Success appends exactly one column. Any detectable invalidity or representability/format-limit failure returns `Err(Error.Invalid)` and leaves encoder bytes, column count, order, and future output unchanged. OOM aborts. Numeric values retain exact little-endian bits; bool packs LSB-first; strings copy bytes and canonical i32 offsets. | Name and values are borrowed only for the call and copied into encoder-owned staging. No input region is retained. Staging may grow inside the visibly constructed encoder; no exact allocation count, peak-byte ratio, or performance promise is made. | Four distinct checked operations and keyed runtime entries share one pre-mutation validation/commit owner. `slice<str>` uses its settled header layout and is never passed as an extern view. Runtime visits headers under the compiler-private valid-range precondition. | Left-to-right evaluation; all invalidity products and deterministic precedence; failure-then-retry transactional state; 1024th success and 1025th no-op error; duplicate exact/case-different names; every row count; numeric/NaN bits; bool packing/tails; UTF-8/NUL/LF/empty strings; allocation and fatal failpoints; whole/per-unit and generic carriers. |
| `e.finish() -> buffer` | `e` is a bound initialized encoder and is consumed exactly once after receiver checking. Zero successfully added columns is valid for any nonnegative row count. | Pure. Produces the one canonical v1 envelope below. It cannot return a recoverable error because every source-dependent limit was admitted transactionally by construction; OOM aborts. | The returned Move `buffer` owns the final contiguous bytes. Finish consumes/frees the encoder shell and staging and nulls its source. At the return point only the output buffer remains live. | One checked consuming operation and keyed runtime finisher reuse the existing buffer representation and Drop. It emits one exact allocation for the final byte range; staging allocation behavior remains unpromised. | Zero/one/four columns, order preservation, finish after failed put, source nulling, early/control-flow returns, exact final live owner, golden bytes, allocation/fatal failpoints, whole/per-unit parity. |

## Source surface

```align
import core.codec

fn encode(
  ids: slice<i64>,
  scores: slice<f64>,
  flags: slice<bool>,
  names: slice<str>,
) -> Result<buffer, Error> {
  mut out := codec.encoder(ids.len())?
  out.put_i64("id", ids)?
  out.put_f64("score", scores)?
  out.put_bool("active", flags)?
  out.put_str("name", names)?
  return Ok(out.finish())
}

fn first(input: slice<u8>) -> Result<str, Error> {
  batch := codec.open(input)?
  index := batch.find("name") else { return Err(Error.Invalid) }
  names := batch.strs(index) else { return Err(Error.Invalid) }
  return Ok(names.at(0) else "")
}
```

Declarations and positional calls stay distinct: there are no written type arguments, named
arguments, inferred schema reflection, variadic columns, map value, macro, annotation, or RPC
method. Column order is insertion/wire order. Names are identity, not lookup normalization.

## Canonical v1 envelope

All integers in the envelope and buffers are little-endian. Every buffer **offset** is a multiple
of 8, but the enclosing `slice<u8>` address may have any alignment; accessors use alignment-1 loads.
`total_len` is exact; no prefix/suffix framing or trailing bytes are admitted.

### Header — 32 bytes

| Offset | Width | Field | Canonical rule |
|---:|---:|---|---|
| 0 | 8 | magic | ASCII `ALNCOL01` (`41 4c 4e 43 4f 4c 30 31`) |
| 8 | 8 | `total_len` | exact complete envelope length, `32..=i64::MAX` |
| 16 | 8 | `row_count` | `0..=i64::MAX` |
| 24 | 4 | `column_count` | unsigned count in `0..=1024`; larger values reject before descriptor access |
| 28 | 4 | reserved | all zero |

Exactly `column_count` 48-byte descriptors follow immediately.

### Column descriptor — 48 bytes

| Relative offset | Width | Field | Canonical rule |
|---:|---:|---|---|
| 0 | 8 | `name_offset` | exact next byte in the packed name section |
| 8 | 4 | `name_len` | positive byte length |
| 12 | 1 | `kind` | exact `codec.kind` tag 0..3 |
| 13 | 1 | flags | zero; v1 has no nullability/compression/dictionary bit |
| 14 | 2 | reserved | zero |
| 16 | 8 | `data_offset` | exact aligned next buffer offset |
| 24 | 8 | `data_len` | exact kind-derived data length |
| 32 | 8 | `aux_offset` | exact string-values buffer offset; zero for other kinds |
| 40 | 8 | `aux_len` | exact string-values length; zero for other kinds |

The name section starts at `32 + 48 * column_count`. Names are nonempty valid UTF-8, byte-unique,
and packed in descriptor order without separators or gaps. Zero bytes pad its end to 8. Buffers then
appear in descriptor order. Each data buffer starts at the current aligned cursor and is followed by
zero padding to 8; a `Str` auxiliary buffer follows its offsets buffer under the same rule. The last
padding ends at `total_len`. Every padding byte is zero, so one semantic batch has one byte encoding.

### Kind buffer rules

| Kind | `data` | `aux` | Additional canonical rules |
|---|---|---|---|
| `I64` | `rows * 8` contiguous signed i64 values | absent | Arrow fixed-width primitive data buffer, no validity bitmap |
| `F64` | `rows * 8` contiguous IEEE-754 binary64 bit patterns | absent | Arrow fixed-width primitive data buffer, no validity bitmap; all bit patterns admitted |
| `Bool` | `ceil(rows / 8)` bytes | absent | Arrow boolean bitmap, row `i` at `(byte[i/8] >> (i%8)) & 1`; unused high tail bits zero |
| `Str` | `(rows + 1) * 4` signed i32 offsets | concatenated UTF-8 bytes | Arrow UTF-8 variable-binary layout without a validity bitmap; first offset 0, offsets monotonic/nonnegative, final offset equals `aux_len <= i32::MAX`, and every adjacent range is valid UTF-8 |

This is Arrow-compatible **physical buffer layout**, not Arrow IPC, FlatBuffers metadata, the Arrow
C Data Interface, Parquet, compression, or a promise that an `ALNCOL01` envelope is accepted by an
Arrow implementation directly. V1 is a non-null struct batch whose child buffers match Arrow's
non-null `Int64`, `Float64`, `Boolean`, and 32-bit-offset `Utf8` layouts. Absence is modeled outside
the batch; nullable values and validity bitmaps are deferred together.

The format and source surface are target-independent. Accessors decode little-endian bits inline;
a big-endian target adds the required byte swap without copying the column or changing its API.

## Validation order and error precedence

`codec.open` returns the same `Error.Invalid` value for every malformed envelope, but its work and
pre-side-effect behavior are deterministic. It stops at the first failing step:

1. Validate the safe-view private precondition (nonnegative length, positive length implies nonnull),
   minimum header length, magic, reserved header bytes, `total_len`, and exact input length, in that
   order. Base address alignment is not a validity condition.
2. Validate row/count exposure, the `column_count <= 1024` limit, descriptor multiplication/addition
   without overflow, and complete descriptor-table bounds before reading a descriptor.
3. For descriptors in ordinal order, validate kind, flags/reserved, scalar representability, and
   kind-derived lengths without reading names or buffers.
4. Validate the packed name topology in ordinal order, then every name's nonempty length, bounds,
   and UTF-8 before checking any duplicate. Fill two fixed `[u16; 1024]` stack arrays (4096 bytes),
   stable bottom-up merge-sort ordinals by byte-lexicographic name then ordinal in exactly ten
   passes, and reject equal adjacent names. Validate the zero name padding last. Thus an invalid
   later name precedes an otherwise-present duplicate, and name byte work is O(total name bytes ×
   ceil(log2(columns))) rather than quadratic.
5. Recompute the complete data/aux cursor in ordinal order; validate exact offsets, exact lengths,
   8-byte bounds, absent-aux zeros, zero inter-buffer/final padding, and final `total_len` equality.
6. In ordinal order validate content: Bool unused tail bits, or Str offsets from first through last,
   then each string cell's UTF-8. Numeric buffers need no content validation.

Every arithmetic check precedes the memory read whose address it protects. No length prefix causes
allocation. Every runtime multi-byte field read decodes little-endian bytes without forming an
aligned typed reference. A multi-invalid fixture owns every precedence boundary; validation never
writes an output scalar before all steps succeed.

Encoder calls use the following precedence before mutation: receiver validity; name length/
nonempty/UTF-8; admitted-column-count limit; row length; kind-specific representability and cell
walk; duplicate name; complete prospective envelope arithmetic. OOM is terminal and is not
converted to `Error.Invalid`.

## Golden vectors

The design and implementation keep two independent codecs: the production encoder and the
validation/view decoder. Tests must not obtain expected bytes by calling the other side. The
checked-in goldens include, at minimum:

| Vector | Semantic value | Required purpose |
|---|---|---|
| `empty-0x0` | 0 rows, 0 columns | fixed header, zero-column/nonzero-row twin, exact length/trailing rejection |
| `i64-two` | name `i`, values `[-1, 2]` | signed little-endian values and padding |
| `f64-bits` | name `f`, `1.5` plus a fixed quiet-NaN payload | exact IEEE bit preservation |
| `bool-tail` | name `b`, values `[true, false, true, true, false, false, false, true, true]` | LSB order, byte boundary, and zero tail bits |
| `str-mixed` | name `s`, values `["", "a\0", "あ\n"]` | repeated i32 offset, NUL, multibyte UTF-8 boundary, and LF |
| `all-four` | four named columns with equal row count | descriptor/name/buffer order and cross-kind topology |

The exact lowercase hexadecimal v1 bytes are:

```text
empty-0x0 (32 bytes)
414c4e434f4c3031200000000000000000000000000000000000000000000000

i64-two (104 bytes)
414c4e434f4c3031680000000000000002000000000000000100000000000000
5000000000000000010000000000000058000000000000001000000000000000
000000000000000000000000000000006900000000000000ffffffffffffffff
0200000000000000

f64-bits (104 bytes; 1.5 = 0x3ff8000000000000,
fixed quiet NaN = 0x7ff8000000000042)
414c4e434f4c3031680000000000000002000000000000000100000000000000
5000000000000000010000000100000058000000000000001000000000000000
000000000000000000000000000000006600000000000000000000000000f83f
420000000000f87f

bool-tail (96 bytes)
414c4e434f4c3031600000000000000009000000000000000100000000000000
5000000000000000010000000200000058000000000000000200000000000000
0000000000000000000000000000000062000000000000008d01000000000000

str-mixed (112 bytes)
414c4e434f4c3031700000000000000003000000000000000100000000000000
5000000000000000010000000300000058000000000000001000000000000000
6800000000000000060000000000000073000000000000000000000000000000
02000000060000006100e381820a0000

all-four (296 bytes; rows are i64 [-1,2], f64 [1.5,-0.0],
bool [true,false], str ["x",""])
414c4e434f4c3031280100000000000002000000000000000400000000000000
e0000000000000000100000000000000e8000000000000001000000000000000
00000000000000000000000000000000e1000000000000000100000001000000
f800000000000000100000000000000000000000000000000000000000000000
e200000000000000010000000200000008010000000000000100000000000000
00000000000000000000000000000000e3000000000000000100000003000000
10010000000000000c0000000000000020010000000000000100000000000000
6966627300000000ffffffffffffffff0200000000000000000000000000f83f
0000000000000080010000000000000000000000010000000100000000000000
7800000000000000
```

Each vector has fixed semantic-to-byte and byte-to-semantic assertions. One-byte mutations cover
every magic/version/tag/reserved/padding/offset/length/tail/UTF-8 class; truncation is parameterized
over every byte boundary. A separate fixture places each valid vector at base-address residues 0
through 7 and proves identical semantic results. The encoder round-trip first binds
`finished := encoder.finish()`, then reopens `finished.bytes()` while that owner remains live; it
does not form a view from an unbound temporary `buffer`.

## Type, region, and placement closure

The compiler-private scalar layouts are fixed. A batch is `{ input_ptr, input_len }`; validation
does not allocate or attach a hidden certificate pointer. I64, F64, and Bool columns are each
`{ bytes_ptr, row_len }`. A string column is `{ offsets_ptr, data_ptr, row_len }`; adjacent validated
i32 offsets determine each cell length, so no fourth field is retained. Each pointer uses the target
pointer width and each length is i64. No column pointer is promoted to a typed-aligned pointer.
`codec.encoder` is one nonnull runtime-owned pointer while live. None of these private layouts is a
source `layout(C)` record or an admitted extern parameter/return.

`codec.batch`, all four typed columns, and their
`Option`/`Result`, struct-field, parameter, return, and control-flow carriers propagate the exact
input region and storage generation through the canonical region-bearing classifiers. They are not
constructible by user literals, casts, `raw`, extern returns, constants, globals, array elements,
parallel elements/results, or closure/task capture in this capability.

`codec.encoder` follows the existing bare Move accumulator class used by `buffer`/`builder`: local,
by-value/shared/mutable parameter, direct return, and ordinary Option/Result/user-sum/struct carriers
are admitted only where the canonical single-owner classifiers admit the same transition. Move-in,
move-out, replacement, consuming match, `else`, `?`, `map_err`, branch/loop joins, early return,
finish, and Drop must leave one live owner or none and must null every consumed source. Arrays,
slices, fixed arrays, tuples, boxes, parallel values, captures, globals/constants, user native/extern
ABIs, and unbound mutating receivers are rejected before MIR.

Encoder staging copies its borrowed arguments during each successful call, so it never accumulates
a region. A failed put is a no-op, including allocation failpoints before commit. Finish is the sole
transfer from encoder-owned staging to the returned `buffer`; Drop without finish publishes nothing.

## Effects, allocation, and performance boundary

Every source operation is Pure: it reads or builds in-memory values and performs no externally
observable I/O. Allocation is still visible in the `codec.encoder(...)` constructor and returned
Move storage. `open` and every batch/column accessor allocate zero owned/heap bytes; `open` uses the
fixed 4096-byte name-sort stack scratch. Encoder construction, staging growth, and final output use
the ordinary hard-OOM policy. V1 validation performs ten merge passes and at most 9,217
lexicographic name comparisons at 1024 columns, then O(columns) `find` and O(1) ordinal projection/
element access. The encoder maintains a sorted name index, uses binary search before commit, and is
O(total output bytes + columns² fixed-index movement) under the same cap. It does not promise a throughput, syscall, exact staging
allocation count, peak-memory ratio, SIMD width, or compression ratio, so no benchmark is a
correctness gate.

All four typed `at` paths remain optimizer-visible and do not call an opaque per-element runtime
helper. Numeric loads explicitly carry alignment 1, so unaligned input is safe and still available
to LLVM's unaligned-load vectorization. `pkg.frame` owns any future virtual pipeline/fused scan over
these views; a static-input validation artifact likewise requires a measured consumer. Neither
changes v1 wire bytes or appears before that consumer's reviewed ledger.

## Capability boundary and deferrals

V1 deliberately has no nulls/validity bitmap, unsigned/narrow integers, f32, binary cells, nested
list/struct/dictionary/union columns, timestamps/decimal, mutable batch, row append, schema
reflection, `soa<T>` conversion, mmap/file API, stream/fragments, compression, encryption/checksum,
endianness switch, Arrow IPC/Flight/C Data export, RPC, query plan, dataframe operation, or stable
cross-major compatibility promise. `pkg.frame` is the first dynamic consumer and remains separate.

A v2 format gets a different eight-byte magic and a new complete ledger. V1 decoders never accept a
future flag/tag and v1 encoders never emit one. A widening must preserve the one-way source surface
or replace it before release; no compatibility alias or permissive reader lands speculatively.

## Implementation closure matrix

Implementation may begin only after independent review accepts this ledger. The one implementation
capability must close every applicable row; a parameterized invariant owner may cover many cells.
The hand-written implementation diff is expected to exceed roughly 1,000 lines. Keeping it one
capability is deliberate: source formation, six new checked-HIR type families, region/Move
classification, MIR operations, LLVM layout/access lowering, eight runtime rows, and the independent
encoder/decoder owners form one dormant producer-to-consumer chain. Splitting it would leave no
useful stable consumer, duplicate type/ABI/provenance proof, and permit an inactive intermediate to
drift; atomic activation has less integration risk.

| Axis | Required implementation closure | Owner evidence |
|---|---|---|
| Type formation and identity | Unique module/types/enum, imports, nominal identity, interface/generic/whole/per-unit round trip, canonical type records, capability and runtime fingerprint, fail-closed new-type sweep. | Sema positives/negatives; interface and canonical-type goldens; checked-HIR variant sweep. |
| Batch formation and validation | Exact six-stage validator, no pre-success output write, arithmetic before read, no owned/heap allocation, 1024/1025 count boundary, fixed 4096-byte name scratch, ten stable merge passes, canonical topology/content, independent decoder. | Runtime unit mutation/truncation/precedence/allocation matrix; exact-limit and rejected-next fixtures; common-prefix distinct/duplicate names with the 9,217-comparison maximum; driver `codec.open` owner. |
| Region and generation | Direct/input-field/parameter/return and Option/Result/user-sum/struct carriers; `if`/`match`/`else`/`?`/`map_err`/block/loop joins; mutation/replacement/drop rejection; malformed HIR. | Parameterized provenance and borrow-liveness owner across batch, name, and all four typed columns. |
| Projection/codegen | Four kind products, total ordinal behavior, alignment-1 header/descriptor loads, four inline element paths, alignment-1 numeric/string-offset loads, target byte order, optimized/unoptimized parity, no per-element native call or typed pointer before bounds/kind success. | Driver count/metadata/byte results at base alignments 0..7, LLVM structural assertions, differential index matrix, NaN payload and endian owners. |
| Encoder ownership | Construction, all four successful puts, every failed put followed by retry/finish, move-in/out, source nulling, Drop, replacement, return, carriers and control joins, fatal allocation at every acquisition. | Driver ownership/control matrix and runtime allocation/failpoint ledger. |
| Native ABI | Exact compiler declaration/Rust export parity, output initialization, null/alignment/signed-length checks, no unwind, status mapping, allocator provenance, whole/per-unit link. | Runtime ABI registry/attribute owner and direct malformed-ABI unit tests. |
| Canonical encoding | Transactional checks, exact names/order/buffers/padding, all semantic limits, sorted name index and byte-exact binary search, 1025th pre-mutation rejection, independent encoder, final sole buffer owner. | Six fixed goldens both directions, per-field mutation, 1024th-success/1025th-no-op, common-prefix names, failure-then-retry, allocation parity. |
| Compatibility | Existing `slice<u8>`/`buffer` binary operations, arrays, JSON/SoA, caches, build modes, and current little-endian targets remain unchanged; the explicit byte-order lowering is correct on a big-endian target without changing source or wire bytes. | Focused compatibility set plus whole/per-unit/cache edit-revert and synthetic endian-lowering twins. |

## Design-review finding closure

| Finding | Ledger-first closure |
|---|---|
| P1 encoder output could not guarantee the 8-byte base alignment required by its own decoder because existing `buffer` stores `Vec<u8>` | Remove base alignment from wire validity and replace standard numeric slices with symmetric opaque i64/f64 column views. All header/descriptor/numeric/string-offset reads lower with alignment 1 and explicit little-endian semantics; every base alignment succeeds, a bound finished buffer's `.bytes()` view round-trips, and no cross-cutting Buffer representation change is required. |
| P2 allocation-free duplicate-name validation admitted quadratic amplification up to u32 columns | Fix the v1 admitted column count at 1024 in both decoder and encoder; reject 1025 before descriptor access or mutation. Decoder uniqueness uses two fixed `[u16; 1024]` arrays and ten-pass bottom-up merge sort, at most 9,217 lexicographic comparisons; encoder keeps a sorted name index and binary-searches it. Exact-limit, rejected-next, common-prefix work, scratch-size, and duplicate-precedence owners close the class. |
| Re-review P2 left a direct-slice promise in the design rationale after the public surface moved to opaque typed views | Replace it with the optimizer-visible typed-column path and sweep every current codec source-of-truth for direct/ordinary/numeric/standard-slice and slice-pipeline promises. The only remaining matches describe this finding or unrelated settled numeric-slice behavior. |
| Rebased review P2 described `rows`/`columns` as fields absent from the fixed `{ input_ptr, input_len }` batch scalar | Read the validated envelope header instead: alignment-1 little-endian i64 at byte 16 and u32 at byte 24, with target-required swaps and u32-to-i64 zero extension. Propagate the rule through checked-HIR, ABI, and projection owners. |
| Rebased review P2 asked an unbound `finish().bytes()` temporary to outlive its Move owner | Keep the existing `BufferBytes` rule unchanged. Bind the consumed encoder result to a local `finished`, take `finished.bytes()`, and keep `finished` live through `codec.open`; update every round-trip acceptance phrase in both language mirrors. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/core-design/ja/codec.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`,
`docs/impl/19-hir-validation-ledger.md`, and `docs/impl/20-runtime-abi-ledger.md` must agree before
implementation. The implementation PR may update the two ledgers with concrete variant/symbol rows
without reopening this public contract.

Author-side pass completed for the design candidate:

- every source argument/result has one exact type, evaluation, ownership, region, allocation, and
  error rule;
- all detail/kind/ordinal/presence states have exhaustive field and unavailable-value rules;
- input encoding, UTF-8, embedded NUL, validation precedence, and pre-side-effect behavior are fixed;
- every scalar width/tag/order/padding and malformed-input rule is exact, with independent goldens;
- no ambient configuration, reflection, source/artifact I/O, later milestone, or RPC surface enters;
- runtime inspection reads producer-validated envelope tables rather than source or reflection;
- examples separate declarations from positional calls and use currently settled syntax; and
- acceptance owners cover every ledger invariant; no unclaimed performance benchmark is required.
