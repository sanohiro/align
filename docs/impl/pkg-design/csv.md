# pkg.csv design

Status: **DESIGNED**. This document is the authority for the first `pkg.csv` capability. The
implementation must not widen it. V1 decodes one in-memory UTF-8 CSV document directly into an
arena-backed `soa<R>`; streaming, encoding, files, dialect inference, and owned rows are separate
capabilities.

## Public-contract ledger

This table is authoritative. Later prose may make a cell more explicit but must not add a public
surface, producer, error, allocation, or input that is absent here.

| Public record | Exact type, inputs, defaults, and semantics | Error, effect, ownership, lifetime, and allocation | Compiler/runtime/package owner, artifact and cache identity | Prerequisite and exact acceptance evidence |
|---|---|---|---|---|
| `pub Header { Present, Absent }` | Closed source/discriminator order is exactly `Present = 0`, `Absent = 1`. `Present` consumes the first logical record as a header; `Absent` maps physical columns to `R` fields in declaration order. There is no inference or default. | Copy and Pure. No borrow, allocation, Drop, ambient MIME metadata, or retained state. | `pkg.csv` owns the nominal sum. Its spelling, tags, and order enter ordinary whole-program/per-unit interface, dependency, and cache identity. | Shipped closed sums. Exact tag/order, construction/match, malformed checked-HIR, interface-mutation, and cache owners. |
| `pub LineEnding { CrLf, Lf }` | Closed source/discriminator order is exactly `CrLf = 0`, `Lf = 1`. The selected sequence is the only record separator outside quotes; a final record may omit it. `CrLf` rejects lone CR/LF and `Lf` rejects every outside-quote CR. There is no auto-detect, mixed mode, lone-CR mode, platform default, or ambient newline setting. | Copy and Pure. Line breaks inside a quoted field remain data byte-for-byte, including CR, LF, and CRLF. No allocation or retained state. | `pkg.csv` owns the nominal sum and interface identity. The runtime receives one checked i32 tag. | Shipped closed sums. Exact tag/order; final-terminated/unterminated and mixed/lone separator products; whole/per-unit and malformed-HIR owners. |
| `pub DecodeOptions { header: Header, line_ending: LineEnding, max_rows: i64 }` | Exact field/source order as shown. No defaults. `max_rows` is an inclusive nonnegative data-row bound; a negative value is `Error.Invalid`, zero admits a header or empty document but no data row, exact fit succeeds, and the next complete otherwise-valid row is `Error.LimitExceeded`. | Copy and Pure. Validation occurs in field order before descriptor/input inspection or arena allocation. The value is retained nowhere and allocates nothing. | `pkg.csv` owns the nominal record and complete reachable definition graph. All fields participate in interface/dependency/cache identity. | Shipped records and i64 bounds. Field/order/default, negative/zero/exact/next, direct/imported/generic, whole/per-unit, and cache owners. |
| `pub Error { Invalid, LimitExceeded }` | Closed source/discriminator order is exactly `Invalid = 0`, `LimitExceeded = 1`. `Invalid` covers invalid options, CSV grammar, selected line ending, header identity/uniqueness/completeness, record width, and selected-field conversion. `LimitExceeded` covers more than 1024 physical header columns, the first row beyond `max_rows`, or exact output/normalization layout not representable in i64 and the target allocation domain. | Copy and Pure. OOM and an impossible compiler/runtime status remain hard aborts. No message, path, row/column position, partial output, fallback, retry, logging, or second cleanup error exists. | Ordinary package sum identity. Private runtime statuses map bijectively as `0 = success`, `1 = Invalid`, `2 = LimitExceeded`; every other result reaches the package's explicit `std.process` dependency and `process.abort()`. | Shipped `Result`, tag-only sums, and abort. Every producer/status/tag/order, multi-invalid precedence, malformed ABI, whole/per-unit, and exhaustive-match owners. |
| `pub fn decode<R: SoaPlain>(input: str, out: region, options: DecodeOptions) -> Result<soa<R>, Error>` | Arguments evaluate exactly once, left-to-right. `R` is inferred only from the expected result type and must be a nonempty natural-layout record with at most 1024 fields, each an integer, float, `bool`, `char`, or `str`. One leading UTF-8 BOM is removed at absolute byte zero; another is data. `Header.Present` requires 1..=1024 unique nonempty decoded header names, maps every declared field exactly once by byte-exact name, and grammar-validates but does not convert extra columns. `Header.Absent` requires exactly `R`'s field count in declaration order. Every data record has the selected physical width. Scalar cells use the exact lexical conversions below; `str` preserves decoded CSV text. | Pure. Pass 1 validates the complete successful input, selected conversions, row bound, normalized byte count, and output layout without heap or arena allocation. Only then pass 2 allocates one exact aligned arena block and fills columns directly, with no AoS intermediate or transpose. Unquoted and quoted-without-doubled-quote `str` cells borrow exact input subranges; only quoted cells containing `""` are copied once, with each pair collapsed to `"`, into the same output arena block. Primitive-only results depend on `out`; a result containing `str` depends on both `input` and `out`. The returned `soa<R>` is Copy, cannot outlive `out`, and carries no Drop. Error leaves `out` unchanged by this call. | Canonical `pkg.csv` source owns one public generic wrapper and one private exact bridge; application source cannot construct or invoke the bridge directly. Sema forms one checked `CsvDecode` operation after exact package/signature/schema validation; HIR/MIR retain `R`, input, destination region, options, effect, and region facts. LLVM emits one immutable compiler-owned `CsvField` table and calls reserved row A123 `align_rt_csv_decode_soa_v1`. Interface and persistent frontend/object/final-link keys include the package source, concrete `R` definition, bridge identity, checked operation, descriptor bytes, runtime key, ABI version, and implementation body; no file, locale, MIME metadata, CPU feature, environment, or allocator setting is an input. | Shipped generics/`SoaPlain`, named regions, SoA layout, source package sealing, checked-HIR validation, arena allocation, and two-pass typed decode patterns. Acceptance crosses formation/visibility, exact schema/header/grammar/value products, BOM, bounds, zero-copy/copy placement, region/storage generations, no-allocation error, direct-to-column layout, ABI/descriptor/status, whole/per-unit/generic monomorphization, cache edit/revert, optimized/unoptimized lowering, and local allocation/work-count evidence. |

## Decision and scope

V1 has one typed materialization boundary:

```text
in-memory UTF-8 CSV + explicit dialect + explicit destination region
  -> validated selected columns
  -> one arena-backed soa<R>
```

It deliberately does not add a dynamic row, map, record reflection API, parser object, iterator, or
second row-major result. The expected `soa<R>` annotation is the schema. `Header.Present` permits a
wide input to be projected into a narrow declared record by name; `Header.Absent` uses the only
unambiguous positional rule, exact declaration order and exact width.

The source declarations are:

```align
module pkg.csv

import std.process

pub Header {
  Present
  Absent
}

pub LineEnding {
  CrLf
  Lf
}

pub DecodeOptions {
  header: Header,
  line_ending: LineEnding,
  max_rows: i64,
}

pub Error {
  Invalid
  LimitExceeded
}

fn decode_bridge<R: SoaPlain>(
  input: str,
  out: region,
  options: DecodeOptions,
) -> Result<soa<R>, Error> = process.abort()

pub fn decode<R: SoaPlain>(
  input: str,
  out: region,
  options: DecodeOptions,
) -> Result<soa<R>, Error> = decode_bridge(input, out, options)
```

`decode_bridge` is compiler-recognized only as that private declaration and body inside canonical
`pkg.csv`. It is not exported in the package interface. A same-named application function, extern,
or noncanonical vendored package is an ordinary declaration and cannot select `CsvDecode`; the
canonical package validator rejects a changed bridge before evaluating its placeholder body.

## Public use

Declarations and calls remain separate, and the expected result supplies `R`:

```align
import pkg.csv

Trade {
  active: bool,
  amount: i64,
  symbol: str,
}

fn active_total(input: str) -> Result<i64, csv.Error> {
  options := csv.DecodeOptions {
    header: csv.Header.Present,
    line_ending: csv.LineEnding.Lf,
    max_rows: 1000000,
  }
  arena out {
    rows: soa<Trade> := csv.decode(input, out, options)?
    return Ok(rows.where(.active).amount.sum())
  }
}
```

An owned `string` input is auto-borrowed by the ordinary `str` parameter rule. There are no written
type arguments, named arguments, defaults, implicit current arena, ambient MIME parameters, or
platform newline selection.

## CSV grammar

V1 uses the RFC 4180 record/quote model over Align's always-valid UTF-8 `str`. It intentionally
widens the original printable-ASCII `TEXTDATA` range to UTF-8 text, as the later `text/csv`
registration does, while rejecting binary invalid UTF-8 at the existing bytes-to-`str` boundary.
The public `LineEnding` value makes the one common LF compatibility path explicit.

Let `EOL` be exactly CRLF for `CrLf` and exactly LF for `Lf`. After removing at most one absolute
leading BOM, the accepted grammar is:

```text
file          = empty / record *(EOL record) [EOL]
record        = field *(COMMA field)
field         = unquoted / quoted
unquoted      = *UTF8-EXCEPT-COMMA-CR-LF-DQUOTE
quoted        = DQUOTE *(UTF8-EXCEPT-DQUOTE / COMMA / CR / LF / DQUOTE DQUOTE) DQUOTE
COMMA         = %x2C
DQUOTE        = %x22
BOM           = %xEF %xBB %xBF
```

The grammar is byte-exact:

- spaces and tabs are field data; there is no trim;
- a quote starts a quoted field only at its first byte, a quote is forbidden in an unquoted field,
  and only `""` encodes one quote inside a quoted field;
- after a closing quote, only comma, the selected `EOL`, or EOF is valid;
- comma, CR, LF, CRLF, NUL, and non-ASCII UTF-8 inside quotes are data and are not normalized;
- outside quotes, only the selected `EOL` is a record separator. `CrLf` rejects a lone CR or LF;
  `Lf` rejects every CR outside quotes;
- a final `EOL` terminates the preceding record and does not create another empty record;
- an empty document has zero logical records. A blank record has one empty field, so it is valid
  only when the selected physical width is one; and
- a trailing comma creates an empty final field. It is ordinary data width, not a syntax error by
  itself.

One BOM is recognized only at byte offset zero and removed before header/data interpretation. A BOM
at any later position, including a second consecutive BOM, is ordinary U+FEFF field data. Thus a
BOM-only document is empty under `Header.Absent` and invalid under `Header.Present` because no
header record exists.

## Header and projection rules

`Header.Absent` fixes the physical width to the number of declared fields in `R`. Physical ordinal
`i` decodes into declared field `i`. Every record, including a one-field blank record, must have that
exact width.

`Header.Present` consumes the first logical record even when it is the final unterminated record.
It requires 1..=1024 physical header fields. Each decoded name must be nonempty, and all names—
including undeclared names—must be byte-unique. Matching is byte-exact and case-sensitive after CSV
quote decoding; there is no Unicode normalization. Every declared field name must occur exactly
once. Extra unique columns are admitted, retain their physical positions for width validation, and
are grammar-checked in every row but are not type-converted or copied.

The header map is built in bounded fixed-capacity stack scratch from decoded hashes and input spans.
It performs no heap or arena allocation. Hash equality is always confirmed by decoded byte equality,
so collisions cannot change membership, duplicate rejection, or mapping. Result columns remain in
`R` declaration order regardless of input order.

## Typed cell conversion

CSV quoting is removed before conversion. The logical cell must match its target grammar in full;
no prefix parse, whitespace removal, locale, thousands separator, hexadecimal form, suffix, or
default is accepted.

| Target field | Exact logical spelling and result |
|---|---|
| signed `i8/i16/i32/i64` | `-?[0-9]+`; leading zeros and `-0` are admitted; checked decimal accumulation must fit the target width. |
| unsigned `u8/u16/u32/u64` | `[0-9]+`; leading zeros are admitted; checked decimal accumulation covers the complete target range, including `u64::MAX`. |
| `f32/f64` | `-?[0-9]+(.[0-9]+)?([eE][+-]?[0-9]+)?`; decimal point and exponent each require digits, a leading plus and symbolic `NaN`/`inf` spellings are rejected. Conversion is directly to the target IEEE width with nearest-even rounding; overflow produces signed infinity and underflow may produce signed zero. |
| `bool` | exact lowercase ASCII `true` or `false`. |
| `char` | exactly one decoded Unicode scalar value. |
| `str` | the complete decoded field, including empty text, NUL, whitespace, comma, and embedded line-break bytes. |

The dot in the float grammar is literal ASCII `.`. Empty cells are valid only for `str`. A selected
field whose grammar or range is wrong returns `Error.Invalid`; an unselected extra field has no
target type and therefore receives only CSV grammar validation.

Quoted scalar cells without doubled quotes remain contiguous input spans and parse directly.
Doubled quotes cannot occur in a valid numeric/bool spelling. For `char`, `""""` is the one-byte
quote character and is handled without temporary allocation. No scalar conversion needs normalized
scratch storage.

## Validation and error precedence

Source checking precedes runtime work. The canonical package/signature and concrete `R` are
validated first; `R` must satisfy `SoaPlain`, have 1..=1024 fields, use natural layout, and have
unique source field names. The input, destination, and options expressions then evaluate once in
written order. A terminating expression prevents every later evaluation and native action.

The private runtime boundary follows this exact order:

1. Validate the writable output header, live destination arena, descriptor count/table, every
   descriptor name range/tag/reserved field, and descriptor-name uniqueness before forming any
   slice, loading input, or allocating. Set the private output header to `{ null, 0 }`. A failure is
   the impossible private status and the wrapper aborts.
2. Validate `Header`, then `LineEnding`, then `max_rows >= 0`. Source-valid enum values make the first
   two checks invariant guards; a negative row bound returns `Invalid` before input inspection.
3. Validate the input pointer/length representation, strip one leading BOM, and select the empty or
   first-record path. Invalid private representation is impossible; public malformed CSV is not.
4. When present, parse and validate the complete header in physical order: grammar and selected
   EOL, 1024-column bound, nonempty names, duplicate names, then required declared-name coverage.
   `LimitExceeded` is selected only for the 1025th header column; every earlier lexical/name error
   wins.
5. Parse data records in source order and cells in physical order. For each cell validate CSV
   grammar first, then the selected target conversion when any. Validate record width after its
   fields. Only after a complete otherwise-valid row would increment the count, compare it to
   `max_rows`; that rejected-next row returns `LimitExceeded`. Earlier malformed content wins, and
   later input is not inspected after the limit result.
6. After successful EOF, validate normalized escaped-string byte accumulation, every current SoA
   column offset/size/alignment computation, the appended normalized-byte area, total i64 size, and
   target allocation-size representation. Any failure returns `LimitExceeded` before allocation.
7. Allocate one exact aligned block from `out`, rescan the already-validated input, fill each
   declared column directly, and copy only doubled-quote `str` cells into the normalized tail. A
   mismatch in this infallible fill pass is an internal invariant violation and aborts; it never
   returns a partial `soa` or changes the selected public error.

This establishes one observable precedence: private invariant abort; invalid options; header
grammar/identity; earliest data grammar/conversion/width; row limit; final representability; OOM or
fill invariant abort. No failure performs heap allocation. No recoverable failure advances `out`.

## Ownership, region, and storage-generation closure

The output uses the existing target-specific SoA column layout authority. For row count `N`, each
field has one contiguous `N`-element column in declaration order with the same offset/alignment
formula used by `to_soa`, `json.decode -> soa`, projection, indexing, and pipelines. The CSV runtime
must call the shared layout authority rather than maintain a sibling formula. The normalized string
tail begins after the aligned column area; it changes no projected column address or element stride.

For `N == 0`, the result is canonical `{ null, 0 }` and allocates nothing. For `N > 0`, the one
arena block contains every primitive value, every `str` header, padding, and all doubled-quote
normalized bytes. Padding is initialized, but its byte value is not public because this is not a
wire format.

An unquoted `str` points at its complete input field. A quoted field with no doubled quote points at
the interior between its surrounding quotes. A quoted field with at least one doubled quote points
at its exact decoded bytes in the normalized tail. Empty input-backed strings keep a valid
zero-length input position; no canonical nonnull promise is exposed for a zero-length view.

Every output depends on `out` because its column block lives there. When `R` contains a `str`, the
whole `soa<R>` additionally retains the input storage root and generation even if this particular
document happens to normalize every cell into `out`; region semantics depend on the type, not data.
That fact flows through direct bindings, fields, `Option`/`Result`, control joins, calls, projected
columns, indexing, and pipelines. Reassignment, move, or Drop of the input owner while any such view
is live rejects. Primitive-only output carries no input root after the call.

`input` may already be a view rooted in `out`; this is not alias rejection. Arena allocation
appends without relocating or overwriting existing live bytes, so both the original input root and
the new result remain valid until the one arena owner exits. Compiler-produced layout places the
new block outside every prior live arena allocation.

`out` is a non-owning capability. Passing it neither transfers nor stores the arena owner. The
lexical `arena out {}` remains the sole cleanup owner, and leaving it frees the complete result in
bulk. `soa<R>` itself is Copy and has no element or whole-value Drop.

## Compiler/package boundary and checked HIR

The implementation adds one HIR expression and one MIR rvalue, both named `CsvDecode`; it does not
add a new source type, generic bound, collection, or pipeline terminal. The HIR record contains:

```text
struct_id: valid concrete SoaPlain record id with 1..=1024 fields
input:      exact str
arena:      exact region capability
options:    exact pkg.csv.DecodeOptions
result:     exact Result<soa<struct_id>, pkg.csv.Error>
effect:     Pure
```

The checked-HIR validator processes scalar ids and non-child fields first, then the three children
in source evaluation order, then the relational result rule. `options` is evaluated once as one
record child; MIR projects its checked fields once before the runtime call. It calls the existing single
`soa_plain_ok` authority plus the 1024-field/package-identity rule. Any mismatch refuses before MIR,
native declarations, runtime calls, arena allocation, artifact generation, or cache publication.
Replay/clone, depth, effect, ownership, region, escape, type placement, semantic projection,
interface reconstruction, monomorphization, capability discovery, and variant-tripwire passes name
the new expression explicitly.

`R` and every `pkg.csv` declaration remain nominal language/interface identities. Two records with
the same field spellings in different nominal scopes are distinct. Fingerprints encode that nominal
identity plus the complete ordered reachable definition graph; `SoaPlain` makes each reachable leaf
one primitive. The runtime descriptor is intentionally structural execution metadata and cannot
erase the nominal compiler/cache distinction.

MIR retains the same struct id and children and yields private i32 status plus a zeroed SoA output
slot. Status 0 constructs `Ok`; 1 constructs `Err(Invalid)`; 2 constructs
`Err(LimitExceeded)`; every other i32 reaches `process.abort()`. The error edge publishes no output
and performs no Drop. Bounds and region semantics exist in MIR; LLVM performs pure lowering and may
not invent a package rule.

## Runtime ABI reservation

The design reserves keyed runtime shape A123 without activating it or changing current inventory
counts:

| Runtime key | Exact symbol | Exact LLVM declaration | Exact Rust ABI |
|---|---|---|---|
| `CsvDecodeSoaV1` | `align_rt_csv_decode_soa_v1` | `i32 @SYM(ptr, i64, ptr, i64, ptr, i32, i32, i64, ptr)` | `unsafe extern "C" fn(*const u8, i64, *const CsvField, i64, *mut Arena, i32, i32, i64, *mut AlignStr) -> i32` |

Parameter order is input pointer/byte length, descriptor pointer/count, destination arena, header
tag, line-ending tag, inclusive row bound, writable output header. It is C calling convention and
`nounwind`, with no curated memory, return, or parameter attributes. The runtime may read the input
twice and mutate only the supplied arena and output header after validation, so `readonly`,
`readnone`, `nofree`, and `willreturn` would be false.

For the complete call, input bytes, descriptor records, and descriptor-name bytes are immutable;
the live arena control object and output header are separately exclusive and disjoint from each
other and from every immutable range. Input may lie in a prior live allocation owned by that same
arena because a new arena allocation cannot overlap it. An exact-compatible unsafe source extern
must establish these provenance/overlap conditions; compiler-produced calls establish them from
checked regions and compiler-owned static descriptors before lowering.

`CsvField` is one target-native `#[repr(C)]` record matched by LLVM's non-packed struct:

```text
{ name_ptr: ptr, name_len: i64, tag: i32, reserved: i32 }
```

`reserved` is zero. `tag` packs `(signed << 16) | (kind << 8) | width`: integer kind 0 with width
1/2/4/8 and the sign bit; bool kind 1 width 1; float kind 2 width 4/8; str kind 3 width 16; char kind
4 width 4. All other bits are zero. Names are nonempty static UTF-8 field-name bytes in declaration
order. The runtime validates the complete table before input or arena effects. The descriptor is
compiler-owned inspection data, not reflection: no pointer or field metadata reaches source.

Activation atomically adds the key, symbol, exact declaration golden, runtime definition/export,
checked operation, and owner tests. It advances the keyed/base/each-four-row-probe/maximum totals
from 330/348/352/356 to 331/349/353/357 and makes A124 the next unreserved shape. A source extern
declaration cannot activate the row or select `CsvDecode`; after activation, exact compatible
source-extern reuse follows the ordinary registry rule. No partial producer may activate separately.

## Deterministic examples and golden vectors

Tests keep semantic input and expected columns independent of the production parser. At minimum:

| Vector | Options/schema | Exact result or error |
|---|---|---|
| empty | `Absent`, either EOL, `R { value: str }` | zero rows, `{ null, 0 }`, no allocation |
| BOM-only | same | zero rows; a second BOM followed by EOF is one `"\u{feff}"` row |
| selected-wide | `Present/Lf`, header `ignored,amount,active,symbol\n` | reordered declared columns, unknown first cell skipped, source row order retained |
| quoted | `Present/CrLf`, `"symbol","note"\r\n"A,B","say ""hi""\r\nnow"` | first values borrow interiors; second value is exactly `say "hi"\r\nnow` in `out` |
| scalar edges | absent rows for every integer min/max, `u64::MAX`, f32/f64 exponent/overflow/underflow, bool, and Unicode char | exact target bits/values; rejected-next integer and malformed lexical twins are `Invalid` |
| row bound | valid `max_rows` zero/exact/next | zero/exact success; first complete valid rejected-next row is `LimitExceeded` |
| separators | each terminated/unterminated mode plus mixed/lone CR/LF | only the selected outside-quote spelling succeeds; quoted bytes remain unchanged |
| headers | reordered/extra/missing/duplicate/empty/case-different/1024/1025 | exact mapping; missing/duplicate/empty are `Invalid`; 1025th physical header is `LimitExceeded` |

Byte offsets 0..7 and target endian twins must produce identical values. Malformed-input mutation
walks every comma, quote, doubled quote, CR, LF, BOM, header, numeric digit/sign/dot/exponent, width,
and EOF boundary. A separate type-to-column oracle checks the exact SoA layout and projected values
without invoking the CSV decoder. A test-only reference encoder independently maps the semantic
field vectors to valid quoted/unquoted CSV bytes before decode comparison; it is fixture machinery,
not a public `pkg.csv` encoder or a source of production parser code.

## Complexity and performance boundary

Success performs exactly two sequential input passes, one exact arena allocation for nonempty
output, and direct column writes. It materializes no AoS row and performs no transpose, heap
allocation, per-row allocation, per-field owned string, or copy of unselected/clean text. Header
lookup uses bounded stack state and hash-plus-equality; hash collision cannot affect semantics.

The implementation should share the existing architecture-parity byte-classifier machinery where
it improves the quote/comma/newline scan, with x86, ARM64, and scalar paths matching one oracle. No
specific SIMD width, throughput, latency, allocation address, or speedup is a public promise.
`bench/csv_decode` is a local non-gating measurement over narrow/wide, quoted/clean, LF/CRLF, and
scalar/string corpora. It records input bytes, rows, physical/selected columns, normalized bytes,
passes, arena allocations, and field conversions from producer-owned counters; it exists to catch
AoS, transpose, copied-clean-string, or convert-unselected regressions.

## V1 non-goals and later boundaries

V1 has no CSV encoder; reader/file/mmap/fragment input; streaming scanner; pipeline-source fusion;
row-major `array<R>` output; owned output; dynamic row/value/map; schema reflection; inferred header
or line ending; lone-CR or mixed-newline mode; delimiter/quote/escape/comment configuration;
whitespace trimming; blank-line skipping; null/default/missing value; nullable column; date/time or
decimal type; locale; Unicode normalization; case-insensitive header; duplicate-header policy;
column aliases; output column renaming; malformed-row recovery; partial output; diagnostic message
or row/column/byte payload; parallel decode; or external CSV library dependency.

A later `csv.scan` must be separately consumer-driven and decide its chunk/view lifetime and Copy
row rule rather than reusing this arena-backed materializer by name. Encoding must separately choose
canonical line endings, quoting, float spelling, and a visible output bound. Nullable fields wait
for a nullable SoA representation; they are not encoded through magic empty strings.

## Implementation closure matrix

The capability is expected to exceed roughly 1,000 changed hand-written lines. One atomic package,
HIR/MIR, LLVM, runtime, and owner boundary has lower risk than dormant slices: no public wrapper is
useful without the concrete schema descriptor and runtime fill, and no runtime producer is safely
reachable until checked package formation, region facts, status mapping, and exact SoA layout agree.
The independently useful prerequisite is already shipped named-region/`SoaPlain` support, so there
is no smaller stable consumer to extract.

| Axis | Required implementation closure | Exact owner evidence |
|---|---|---|
| Public formation and identity | Canonical `pkg.csv`; exact four public records/one wrapper/one private bridge; inferred concrete `R`; 1..=1024 `SoaPlain` fields; no same-name/intercepted bridge; direct/imported/local/function-field/control-joined calls. | Package source/interface byte and hash owner; positive/negative schema matrix; exact bridge-body mutation; parameterized call-target shapes; whole/per-unit/generic monomorphs. |
| Evaluation, checked HIR, and MIR | Input/out/options once left-to-right; terminating child stops later work; exact child/type/id/effect/region records; status mapping and no error output; every traversal/replay/validation pass explicit. | Variant sweep tripwire; one-field HIR mutations; source termination/control matrix; MIR status/output and process-abort assertions. |
| CSV lexical closure | BOM position/count; unquoted/quoted/doubled quote; comma; spaces/NUL/UTF-8; CRLF/LF choice; quoted line breaks; final EOL/EOF; blank/trailing-empty records. | Independent grammar oracle; exhaustive bounded mutation/fuzz corpus; exact accepted/rejected byte vectors for every lexical state and EOF transition. |
| Header/projection closure | Present/Absent; declaration/physical reorder; unknown skip; all names nonempty/unique; every declared name once; 1/1024/1025; hash collisions; exact width per row. | Fixed vectors plus generated unique/duplicate/collision tables; conversion counters prove unknown fields skipped; mapping-to-column oracle. |
| Typed conversion | Every integer width/sign edge; float lexical/exact bits/overflow/underflow; bool case; char scalar count; empty/whitespace; selected versus unselected. | Parameterized field-kind oracle and rejected-next twins; optimized/unoptimized, endian, and whole/per-unit parity; no conversion of extras. |
| Bounds and precedence | Invalid enum/HIR, negative/zero/exact/next rows, header cap, normalized-byte arithmetic, every SoA offset/size/alignment and target-size overflow; earliest lexical/conversion/width before row limit; no later scan after terminal error. | Multi-invalid pairwise precedence matrix; target-representability twins; counters for last inspected row/field and zero allocator calls. |
| Allocation and atomicity | Pass 1 no heap/arena; pass 2 one exact aligned arena block only after full success; no AoS/transpose; zero-row null/zero; OOM abort; fill invariant abort; error leaves arena unchanged. | Arena/heap allocation and byte counters; failpoints at layout/allocation/fill boundaries; exact one-block topology inspection; pre/post arena cursor owner. |
| String ownership and regions | Clean unquoted/quoted input views; doubled-quote normalized tail; mixed cells; type-level input+out retention; primitive-only out retention; input already rooted in `out`; direct/field/Option/Result/join/projection/pipeline/storage generations. | Pointer-range classification plus exact bytes; distinct-owner and same-arena input/output twins; input/out escape and mutation negatives; primitive input-release positive; generic and control-flow carrier matrix. |
| SoA layout and pipeline | One shared layout authority; direct declaration-order columns; every field alignment/base residue; index/projection/window/where/map/reduce; str columns read-only; no new pipeline path. | Independent layout oracle; current SoA regression bundle; base residues 0..7; generated mixed-width schemas and str pipeline owners. |
| Native ABI and descriptor | Exact A123 signature/key/symbol/attributes/export; full `CsvField` layout/tag/reserved/name validation before input/allocation; null/negative/overflow pointer products; output zeroing; no unwind. | Registry/golden/export/compatibility mutation owners; direct runtime malformed-ABI matrix; rt-LTO on/off; allocation provenance. |
| Cache and distribution | Package source/bridge, concrete schema graph, checked op, descriptor, runtime key/body invalidate exact frontend/object/link identities; unrelated source and ambient inputs do not; vendorable/prebuilt inventory changes only when source ships. | Whole/per-unit source/private/public/schema/runtime edit-revert twins; prebuilt add/remove/layout owners; no-op and unrelated-unit hits. |
| Performance shape | Two scans; direct selected-column fill; unknown/clean text not copied; no heap/per-row/AoS/transpose; SIMD implementations match scalar when present. | Producer counters plus `bench/csv_decode`; scalar/x86/ARM64 equality owners; benchmark remains non-gating. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/csv.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`,
`docs/impl/19-hir-validation-ledger.md`, `docs/impl/20-runtime-abi-ledger.md`, and `HANDOFF.md`
must agree before implementation. User guides and vendorable/prebuilt inventories must continue to
describe only shipped packages until implementation activates the package.

The author-side pass must prove:

- every public record above has exact source order, tag, type, default, evaluation, error, effect,
  ownership, lifetime, allocation, owner, identity, prerequisite, and acceptance evidence;
- Header × line ending × empty/header-only/data × quoted/clean/doubled-quote × selected/extra × every
  field type × zero/exact/next limit has one field-presence, row/column order, and error rule;
- all text boundaries are UTF-8, NUL is data, header equality is byte-exact, and no borrowed input
  or compiler descriptor is retained beyond the result's stated region;
- every multi-invalid product follows the written validation precedence before arena effects;
- the two-pass/direct-column and clean-versus-normalized allocation promises have producer-owned
  counters and no benchmark is used as a correctness gate;
- A123 fixes every scalar width, tag, parameter order, pointer role, validation precondition,
  status, attribute, output initialization, and activation count in both directions;
- checked HIR, MIR, LLVM, runtime, package interface, whole/per-unit compilation, monomorphization,
  cache identity, and prebuilt inventory agree atomically;
- all examples parse with accepted syntax and declarations remain separate from positional calls;
  and
- no implementation cell consumes streaming, nullable SoA, owned rows, dialect inference, or any
  other later capability.
