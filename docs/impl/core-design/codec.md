# core.codec — columnar batch wire format

> 🌐 **English** · [Japanese](./ja/codec.md)
>
> **Status:** designed; implementation pending.

## Authoritative public-contract ledger

This table is the authority for the first `core.codec` capability. Later prose and implementation
may make a field more explicit but must not widen it. The format carries data batches, not calls,
services, executable plans, or arbitrary user types.

| Public surface | Exact inputs, validation, and evaluation | Exact result, errors, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner and identity | Acceptance owner |
|---|---|---|---|---|---|
| `codec.kind { I64, F64, Bool, Str }` | One closed builtin tag-only sum. Source ordinals and wire tags are exactly `I64=0`, `F64=1`, `Bool=2`, and `Str=3`. There is no numeric conversion, custom kind, nullable modifier, extension metadata, alias, or ambient registry. | Copy and Pure. Equality follows the existing closed-enum rule. No source-formed operation errors or aborts. | The settled one-field enum aggregate `{ i32 tag }`; no borrow, allocation, or Drop. | `align_sema` owns the unique builtin enum behind `import core.codec`; HIR/MIR keep the ordinary enum aggregate. Interfaces serialize its nominal named definition. The source ordinal is also the exact descriptor `kind` byte. | Import/type/variant positives, exact ordinals, wrong module/type negatives, checked-HIR enum identity and closed-tag validation, whole/per-unit identity. |
| `codec.open(input: bytes) -> Result<codec.batch, Error>` | Exactly one positional argument. `input` is borrowed and evaluated once. A successful input is the complete canonical v1 envelope and is 8-byte aligned at its first byte. Validation is allocation-free, bounded by the supplied length, and follows the exact precedence below before constructing a result or retaining a fact. It rejects every unknown tag/flag/version, overflow, noncanonical offset/order/padding, duplicate/invalid name, malformed Arrow-layout buffer, invalid UTF-8 string cell, trailing byte, and a positive-length null or unaligned base. | Pure. Success returns one validated batch view. Every detectable malformed input returns `Err(Error.Invalid)` and performs no allocation or mutation; there is no partial batch. Compiler-private dangling pointers remain outside the safe ABI precondition. | `codec.batch` is a Copy opaque view into `input`, carries its storage generation, and has exactly `region_of(result) = region_of(input)`. The result allocates and owns nothing. Moving, replacing, dropping, or mutably borrowing the input owner while the batch or a derived view remains live is rejected. | Sema forms the region-bearing builtin result; checked HIR proves the exact `bytes`, `Result<codec.batch, builtin Error>`, and input-region fact. MIR calls one keyed validator and constructs the fixed batch scalar only after status zero. Runtime owns byte validation but no retained allocation. The capability, format version, native row, and new canonical type leaves enter interface/implementation/cache and runtime-ABI fingerprints. | Independent semantic-to-byte and byte-to-semantic goldens; every validation-precedence class; truncation at every byte; overflow/allocation-bomb prefixes; unaligned base; no-allocation success/failure; input mutation/escape/control-flow carriers; whole/per-unit parity; malformed checked HIR and ABI. |
| `b.rows() -> i64`; `b.columns() -> i64` | `b` is a validated `codec.batch`, borrowed read-only. | Pure total Copy counts. They perform no revalidation. | No allocation, retained borrow, or mutation. | Checked batch scalar fields lowered as direct loads. | Empty/nonempty/max-admitted counts, repeated calls, imported and carrier-held batches. |
| `b.name(i: i64) -> Option<str>`; `b.kind(i: i64) -> Option<codec.kind>`; `b.find(name: str|string) -> Option<i64>` | Receiver, then argument evaluate exactly once. Negative or out-of-range `i` returns `None`. `find` performs byte-exact, case-sensitive name comparison in ordinal order and returns the unique first match; absence returns `None`. An owned `string` is auto-borrowed. | Pure and total. `name` is a zero-copy UTF-8 view; `kind` and the ordinal are Copy. No error or abort. | `name` and its `Option` carry the batch/input region. `find` retains nothing. No allocation. | Name/kind projection and `find` lower to optimizer-visible descriptor loads and an ordinal-order byte loop over the already-validated envelope. There is no additional native ABI row, source/artifact I/O, or reflection. | Cartesian negative/in-range/upper-bound ordinals; empty/non-ASCII/NUL names; exact-case miss; ordered lookup; repeated access; input-region escape/invalidation; no revalidation/allocation. |
| `b.i64s(i) -> Option<slice<i64>>`; `b.f64s(i) -> Option<slice<f64>>`; `b.bools(i) -> Option<codec.bool_column>`; `b.strs(i) -> Option<codec.str_column>` | Receiver then ordinal evaluate once. A negative/out-of-range ordinal or a different `codec.kind` returns `None`; the exact kind returns a view of all rows. No accessor revalidates bytes. Numeric slices are naturally aligned because `open` already proved the base and canonical buffer topology. | Pure and total. All projections are zero-copy. `f64` preserves every IEEE bit pattern, including NaNs and infinities. There is no coercion between column kinds. | Every returned view and `Option` is Copy and region-bound to the batch input and its generation. No allocation, ownership transfer, or Drop. | Four distinct checked HIR/MIR projections; numeric results reuse the standard slice scalar, while bool/string use the two opaque view scalars below. LLVM forms direct descriptors from validated offsets. | Four-by-four kind/accessor product; negative/out-of-range product; exact numeric bytes and alignment; NaN payload preservation; region carriers/control joins/return and mutation rejection; no runtime call in numeric element loops. |
| `c.len() -> i64`; `c.at(i: i64) -> Option<bool>` for `codec.bool_column` | `c` is borrowed. `at` evaluates the index once, returns `None` outside `0..len`, and otherwise reads Arrow LSB-first bit `i`. | Pure, total, Copy, allocation-free. | The bool result has no region; the column view remains input-region-bound. | Checked bool-column operations lower to a bounds comparison plus byte load, shift, and mask. | Empty, byte boundary, tail-bit, negative/upper-bound, repeated and optimized/unoptimized owners. |
| `c.len() -> i64`; `c.at(i: i64) -> Option<str>` for `codec.str_column` | `c` is borrowed. `at` evaluates the index once, returns `None` outside `0..len`, and otherwise uses validated adjacent i32 offsets to return exactly that UTF-8 cell. Empty strings and embedded NUL/LF are ordinary data. | Pure, total, zero-copy, allocation-free. | The `str` and its `Option` carry the original batch/input region and generation. | Checked string-column operations lower to one bounds comparison, two aligned little-endian i32 loads, and a view construction. `open` has already proved monotonic offsets, bounds, and per-cell UTF-8. | Empty/non-ASCII/NUL/LF cells; repeated offsets; negative/upper-bound; return/carrier/control region closure; optimized/unoptimized and whole/per-unit parity. |
| `codec.encoder(rows: i64) -> Result<codec.encoder, Error>` | Exactly one positional argument, evaluated once. A negative row count returns `Err(Error.Invalid)` before allocation. A nonnegative row count creates an encoder with zero columns. No ambient schema, allocator setting, file, clock, or target input participates. | Pure. Success returns one initialized encoder. OOM follows the language-wide hard-abort policy. | The returned Move handle owns one encoder shell and all later copied column/name staging. It retains no argument. Drop releases every staged byte exactly once and produces no output. | One nominal `Ty::CodecEncoder`/MIR scalar and one keyed runtime constructor/drop pair. Interface identity uses the named builtin type and existing Move return rules. | Negative/zero/positive rows; no-allocation error; one-shell allocation/free; direct/imported/function-value return and Drop; malformed HIR/ABI. |
| `e.put_i64(name, values: slice<i64>)`; `e.put_f64(name, values: slice<f64>)`; `e.put_bool(name, values: slice<bool>)`; `e.put_str(name, values: slice<str>)` — each `-> Result<(), Error>` | `e` is a bound mutable encoder and is not consumed. Receiver, name, then values evaluate once. `name` accepts `str|string`, must be nonempty valid UTF-8, fit u32 length, and be byte-unique among successful columns. `values.len()` must equal `rows`. `put_str` additionally requires total copied cell bytes to fit signed i32. The complete candidate name, length, kind-specific sizes, final canonical length, and every string cell are checked before the first mutation. | Pure. Success appends exactly one column. Any detectable invalidity or representability/format-limit failure returns `Err(Error.Invalid)` and leaves encoder bytes, column count, order, and future output unchanged. OOM aborts. Numeric values retain exact little-endian bits; bool packs LSB-first; strings copy bytes and canonical i32 offsets. | Name and values are borrowed only for the call and copied into encoder-owned staging. No input region is retained. Staging may grow inside the visibly constructed encoder; no exact allocation count, peak-byte ratio, or performance promise is made. | Four distinct checked operations and keyed runtime entries share one pre-mutation validation/commit owner. `slice<str>` uses its settled header layout and is never passed as an extern view. Runtime visits headers under the compiler-private valid-range precondition. | Left-to-right evaluation; all invalidity products and deterministic precedence; failure-then-retry transactional state; duplicate exact/case-different names; every row count; numeric/NaN bits; bool packing/tails; UTF-8/NUL/LF/empty strings; allocation and fatal failpoints; whole/per-unit and generic carriers. |
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

fn first(input: bytes) -> Result<str, Error> {
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

All integers in the envelope and buffers are little-endian. The top-level input address and every
buffer offset are multiples of 8. `total_len` is exact; no prefix/suffix framing or trailing bytes
are admitted.

### Header — 32 bytes

| Offset | Width | Field | Canonical rule |
|---:|---:|---|---|
| 0 | 8 | magic | ASCII `ALNCOL01` (`41 4c 4e 43 4f 4c 30 31`) |
| 8 | 8 | `total_len` | exact complete envelope length, `32..=i64::MAX` |
| 16 | 8 | `row_count` | `0..=i64::MAX` |
| 24 | 4 | `column_count` | unsigned count; descriptor multiplication and exposed count must fit i64 |
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

The format is target-independent but zero-copy numeric projection requires a little-endian target.
The first implementation supports the repository's little-endian x86-64 and AArch64 targets and
rejects any future big-endian target at compile time rather than silently copying or byte-swapping.

## Validation order and error precedence

`codec.open` returns the same `Error.Invalid` value for every malformed envelope, but its work and
pre-side-effect behavior are deterministic. It stops at the first failing step:

1. Validate the safe-view private precondition (nonnegative length, positive length implies nonnull),
   8-byte base alignment, minimum header length, magic, reserved header bytes, `total_len`, and exact
   input length, in that order.
2. Validate row/count exposure, descriptor multiplication/addition without overflow, and complete
   descriptor-table bounds before reading a descriptor.
3. For descriptors in ordinal order, validate kind, flags/reserved, scalar representability, and
   kind-derived lengths without reading names or buffers.
4. Validate the packed name topology in ordinal order, then each name's nonempty length, bounds,
   UTF-8, and byte uniqueness against earlier names. Validate the zero name padding.
5. Recompute the complete data/aux cursor in ordinal order; validate exact offsets, exact lengths,
   8-byte bounds, absent-aux zeros, zero inter-buffer/final padding, and final `total_len` equality.
6. In ordinal order validate content: Bool unused tail bits, or Str offsets from first through last,
   then each string cell's UTF-8. Numeric buffers need no content validation.

Every arithmetic check precedes the memory read whose address it protects. No length prefix causes
allocation. A multi-invalid fixture owns every precedence boundary; validation never writes an
output scalar before all steps succeed.

Encoder calls use the following precedence before mutation: receiver validity; name length/
nonempty/UTF-8; row length; kind-specific representability and cell walk; duplicate name; complete
prospective envelope arithmetic. OOM is terminal and is not converted to `Error.Invalid`.

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
over every byte boundary. A separate fixture proves an otherwise-valid envelope at an unaligned
input address rejects.

## Type, region, and placement closure

The compiler-private scalar layouts are fixed. A batch is `{ input_ptr, input_len }`; validation
does not allocate or attach a hidden certificate pointer. A bool column is `{ bits_ptr, row_len }`.
A string column is `{ offsets_ptr, data_ptr, row_len }`; adjacent validated i32 offsets determine
each cell length, so no fourth field is retained. Each pointer uses the target pointer width and
each length is i64. Numeric projections use the existing `{ data_ptr, row_len }` slice scalar.
`codec.encoder` is one nonnull runtime-owned pointer while live. None of these private layouts is a
source `layout(C)` record or an admitted extern parameter/return.

`codec.batch`, `codec.bool_column`, and `codec.str_column` are opaque Copy views. They and their
`Option`/`Result`, struct-field, parameter, return, and control-flow carriers propagate the exact
input region and storage generation through the canonical region-bearing classifiers. They are not
constructible by user literals, casts, `raw`, extern returns, constants, globals, array elements,
parallel elements/results, or closure/task capture in this capability. Numeric projections are the
existing `slice<i64>`/`slice<f64>` and follow their ordinary placement rules.

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
Move storage. `open` and every batch/column accessor allocate zero bytes. Encoder construction,
staging growth, and final output use the ordinary hard-OOM policy. V1 promises O(input bytes +
columns² name comparison) validation, O(columns) `find`, O(1) ordinal projection/element access,
and O(total output bytes) encoding; it does not promise a throughput, syscall, exact staging
allocation count, peak-memory ratio, SIMD width, or compression ratio, so no benchmark is a
correctness gate.

The hot numeric path is a standard slice after one projection. Bool/string `at` lowering remains
optimizer-visible and does not call an opaque per-element runtime helper. A future measured
consumer may justify fused scans or a static-input validation artifact, but neither changes v1 wire
bytes or appears before that consumer's reviewed ledger.

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

| Axis | Required implementation closure | Owner evidence |
|---|---|---|
| Type formation and identity | Unique module/types/enum, imports, nominal identity, interface/generic/whole/per-unit round trip, canonical type records, capability and runtime fingerprint, fail-closed new-type sweep. | Sema positives/negatives; interface and canonical-type goldens; checked-HIR variant sweep. |
| Batch formation and validation | Exact six-stage validator, no pre-success output write, arithmetic before read, no allocation, canonical topology/content, independent decoder. | Runtime unit mutation/truncation/precedence/allocation matrix plus driver `codec.open` owner. |
| Region and generation | Direct/input-field/parameter/return and Option/Result/user-sum/struct carriers; `if`/`match`/`else`/`?`/`map_err`/block/loop joins; mutation/replacement/drop rejection; malformed HIR. | Parameterized provenance and borrow-liveness owner across batch, name, numeric, bool, and str projections. |
| Projection/codegen | Four kind products, total ordinal behavior, direct numeric slices, inline bool/string element access, optimized/unoptimized parity, no per-element native call. | Driver byte results, LLVM structural assertions, differential index matrix, NaN payload owner. |
| Encoder ownership | Construction, all four successful puts, every failed put followed by retry/finish, move-in/out, source nulling, Drop, replacement, return, carriers and control joins, fatal allocation at every acquisition. | Driver ownership/control matrix and runtime allocation/failpoint ledger. |
| Native ABI | Exact compiler declaration/Rust export parity, output initialization, null/alignment/signed-length checks, no unwind, status mapping, allocator provenance, whole/per-unit link. | Runtime ABI registry/attribute owner and direct malformed-ABI unit tests. |
| Canonical encoding | Transactional checks, exact names/order/buffers/padding, all semantic limits, independent encoder, final sole buffer owner. | Six fixed goldens both directions, per-field mutation, failure-then-retry, allocation parity. |
| Compatibility | Existing `bytes`/`buffer` binary operations, slices, arrays, JSON/SoA, caches, build modes, and little-endian targets remain unchanged; unsupported targets fail explicitly. | Focused compatibility set plus whole/per-unit/cache edit-revert twins. |

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
