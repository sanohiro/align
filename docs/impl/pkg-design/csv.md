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
| `pub fn decode<R: SoaPlain>(input: str, out: region, options: DecodeOptions) -> Result<soa<R>, Error>` | Arguments evaluate exactly once, left-to-right. `R` is inferred only from the expected result type and must satisfy the complete existing `SoaPlain` domain: any nonempty record whose fields are integers, floats, `bool`, `char`, or `str`. CSV adds no schema-field-count or layout restriction; explicit record layout/alignment does not narrow this bound because SoA column layout is independent of AoS record layout. One leading UTF-8 BOM is removed at absolute byte zero; another is data. `Header.Present` requires 1..=1024 unique nonempty decoded header names, maps every declared field exactly once by byte-exact name, and grammar-validates but does not convert extra columns. Thus a schema wider than 1024 can succeed only with `Header.Absent`; a Present header of at most 1024 columns cannot cover it and is `Invalid`, while a 1025th physical header column is still the earlier `LimitExceeded`. `Header.Absent` requires exactly `R`'s field count in declaration order. Every data record has the selected physical width. Scalar cells use the exact lexical conversions below; `str` preserves decoded CSV text. | Pure for ordinary sequential calls and sequential closures/pipelines. Purity does not make the destination capability Send: because every call requires `out`, `decode` cannot be invoked by a `spawn` or `par_map` worker through a region reachable directly or through a captured function value. The shared worker-transfer provenance gate follows concrete callable targets and nested environments through moves, joins, and helper summaries, and rejects before lifted-worker publication, MIR, generated identity, runtime call, or allocation. The raw ABI first verifies UTF-8 without allocation; invalid bytes are private status `-1`. Decode pass 1 then validates the complete successful input, selected conversions, row bound, normalized byte count, and output layout without heap or arena allocation. For `N > 0` only, decode pass 2 allocates one exact aligned arena block and fills columns directly, with no AoS intermediate or transpose; `N == 0` returns canonical `{ null, 0 }` without allocation. Unquoted and quoted-without-doubled-quote `str` cells borrow exact input subranges; only quoted cells containing `""` are copied once, with each pair collapsed to `"`, into the same output arena block. Primitive-only results depend on `out`; a result containing `str` depends on both `input` and `out`, including the existing `Frame`-bounded synthetic owner when an unbound owned `string` is auto-borrowed. The returned `soa<R>` is Copy, cannot outlive `out` or that input frame, and carries no Drop. Error leaves `out` unchanged by this call. | Canonical root `pkg.csv` owns one public generic wrapper whose body calls the compiler-private spelling `pkg.csv.internal.descriptor.decode`; that internal module declares no source function and the package `internal` rule prevents application imports. The abstract template check forms a template-only `CsvDecode` whose `row` is existing `Ty::Param(p)` and whose result contains existing `SoaParam(p)`; that HIR is discarded. Rechecking a concrete monomorph forms the emitted operation with `row = Ty::Struct(id)` and an exact concrete `soa<id>` result. HIR/MIR retain the concrete `R`, input, destination region, options, effect, and region facts. LLVM emits one immutable compiler-owned `CsvField` table and calls reserved row A123 `align_rt_csv_decode_soa_v1`. Interface/frontend identity includes canonical root/internal source, the generic body, concrete nominal `R` graph, checked operation, and descriptor semantics. Object/final-link keys additionally retain all ordinary explicit target, CPU/feature, profile, pipeline, runtime, and linker inputs. No ambient runtime CPU detection, file, locale, MIME metadata, environment, or allocator setting changes CSV semantics. | Shipped generics/`SoaPlain`, named regions, SoA layout, source package sealing, compiler-private internal descriptor spelling, checked-HIR validation, arena allocation, UTF-8 prevalidation, and two decode-pass patterns. Acceptance crosses abstract-template/concrete-monomorph formation and visibility, unbounded schema width versus bounded Present headers, exact header/grammar/value products, BOM, bounds, zero-copy/copy placement, region/storage generations including fresh/control-wrapped owned-input temporaries, shared transitive `spawn`/`par_map` region-capture rejection, no-allocation error, direct-to-column layout, ABI/descriptor/status and one-pass descriptor work bounds, whole/per-unit/generic monomorphization, cache edit/revert, optimized/unoptimized lowering, and local allocation/work-count evidence. |

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
import pkg.csv.internal.descriptor

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

pub fn decode<R: SoaPlain>(
  input: str,
  out: region,
  options: DecodeOptions,
) -> Result<soa<R>, Error> = pkg.csv.internal.descriptor.decode(input, out, options)
```

The vendorable subtree also contains exact module `pkg.csv.internal.descriptor`, with no imports or
source items:

```align
module pkg.csv.internal.descriptor
```

Its `decode` spelling is a compiler-private descriptor operation, like the shipped
`pkg.db.internal.descriptor` family, rather than a private source declaration. This keeps the public
generic body self-contained in its interface: it references no omitted private item, and an
importing unit can monomorphize the retained body against the vendored internal module identity.
The package `internal` rule prevents application source from importing that module. A same-named
application function, extern, added internal item, changed wrapper, or noncanonical package cannot
select `CsvDecode`; canonical package admission rejects it before body evaluation.

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

The physical header cap is not a schema cap. A `SoaPlain` schema with more than 1024 declared fields
works in `Header.Absent` mode. In `Header.Present`, a grammar-valid header within the cap necessarily
misses a declared field and returns `Invalid` during required-name coverage; input that reaches a
1025th physical header field returns the earlier `LimitExceeded` before coverage.

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
validated first; `R` must satisfy the complete existing `SoaPlain` bound, which already requires a
nonempty record with unique source field names. CSV imposes no additional schema-field-count limit.
Explicit AoS layout/alignment remains admitted because the result's SoA column layout does not
consume it. The input, destination, and options expressions then evaluate once in
written order. A terminating expression prevents every later evaluation and native action.

The private runtime boundary follows this exact order:

1. Validate the writable output header and live destination arena before forming any slice,
   loading input, or allocating. Set the private output header to `{ null, 0 }`. A failure is the
   impossible private status and the wrapper aborts.
2. Validate `Header`, then `LineEnding`, then `max_rows >= 0`. Source-valid enum values make the first
   two checks invariant guards; a negative row bound returns `Invalid` before input inspection.
3. Validate the descriptor count as positive and exactly representable, then the complete table
   byte-size/alignment/non-null guards. In declaration order validate each record's positive
   name length and nonnull/range arithmetic, exact source-identifier bytes while hashing once,
   matching `name_hash`, tag, then zero reserved field. Do not compare descriptor names with
   one another: source record formation and checked descriptor emission are the uniqueness
   authority, and pairwise uniqueness is an exact-compatible unsafe-caller precondition. A failure
   in the mechanically checked fields returns the impossible private status. Thus a negative row
   bound wins over a malformed descriptor whenever output and arena are valid.
4. Validate the input pointer/length representation. A negative length or null pointer with positive
   length is malformed private ABI. Length zero accepts either pointer and performs no pointer
   dereference or slice formation. For positive length, first validate the nonnull/range-arithmetic
   guards; the compiler or unsafe caller must already guarantee a readable range of exactly that
   many bytes for the complete call. Only then form the byte slice and validate the complete range
   as UTF-8. Invalid UTF-8 returns private `-1` before BOM removal, CSV parsing, or arena allocation;
   it cannot arise from a safe source `str`. For valid UTF-8, strip one leading BOM and select the
   first-record path. Invalid private representation is impossible; public malformed CSV is not.
5. When present, parse and validate the complete header in physical order: grammar and selected
   EOL, 1024-column bound, nonempty names, duplicate names, then required declared-name coverage.
   `LimitExceeded` is selected only for the 1025th header column; every earlier lexical/name error
   wins.
6. Parse data records in source order and cells in physical order. For each cell validate CSV
   grammar first, then the selected target conversion when any. Validate record width after its
   fields. Only after a complete otherwise-valid row would increment the count, compare it to
   `max_rows`; that rejected-next row returns `LimitExceeded`. Earlier malformed content wins, and
   later input is not inspected after the limit result.
7. After successful EOF, validate normalized escaped-string byte accumulation, every current SoA
   column offset/size/alignment computation, the appended normalized-byte area, total i64 size, and
   target allocation-size representation. Any failure returns `LimitExceeded` before allocation.
8. For `N == 0`, publish canonical `{ null, 0 }` without allocation. For `N > 0`, allocate one exact
   aligned block from `out`, rescan the already-validated input, fill each declared column directly,
   and copy only doubled-quote `str` cells into the normalized tail. A mismatch in this infallible
   fill pass is an internal invariant violation and aborts; it never returns a partial `soa` or
   changes the selected public error.

This establishes one observable precedence: malformed output/arena ABI abort; invalid options;
malformed descriptor/input ABI abort; header grammar/identity; earliest data
grammar/conversion/width; row limit; final representability; OOM or fill invariant abort. No
failure performs heap allocation. No recoverable failure advances `out`.

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

When `input` is an implicit `str` borrow of an unbound owned `string` expression, the existing
path-local synthetic owner is part of that same input storage fact. A string-bearing result retains
the owner at `Frame` through a direct function result or literal and through block, `if`, `match`,
`else`, `?`, `map_err`, and value-carrying `loop` wrappers; it cannot be returned past that frame.
If `out` or `options` terminates after the input completed, no `CsvDecode` action or result is formed
and the already-armed synthetic owner follows the existing cleanup/no-cleanup sink rule. A
primitive-only successful result retains no input fact and the temporary remains only its ordinary
path-local owner, never a result dependency.

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
row:        template-only Ty::Param(p), or emitted Ty::Struct(id)
input:      exact str
arena:      exact region capability
options:    exact pkg.csv.DecodeOptions
result:     template-only Result<soa<Param(p)>, pkg.csv.Error>,
            or emitted Result<soa<id>, pkg.csv.Error>
effect:     Pure
```

The exact canonical wrapper's abstract body check admits `row = Ty::Param(p)` only when parameter
`p` carries `SoaPlain` and the stored result is the matching existing `SoaParam(p)` composite. It
performs no field-count, name, descriptor, layout, provenance, MIR, or native work because the
template has no concrete record. That temporary HIR is discarded by the existing generic-template
path and is neither canonically encoded nor emitted. Consumer instantiation rechecks the retained
source AST with the concrete substitution; only then may sema replace the pair with
`row = Ty::Struct(id)` and `soa<id>`, call the existing single `soa_plain_ok` authority, and enforce
the package-identity rules without a schema-field-count restriction. Forwarding through another
`T: SoaPlain` template repeats
the symbolic check and reaches the same concrete recheck when its outer monomorph is instantiated.

The emitted checked-HIR validator rejects `Ty::Param`, `SoaParam`, or any non-`Struct` row before
processing the three children in source evaluation order and the relational concrete-result rule.
`options` is evaluated once as one record child; MIR projects its checked fields once before the
runtime call. Any mismatch refuses before MIR, native declarations, runtime calls, arena allocation,
artifact generation, or cache publication. Replay/clone, depth, effect, ownership, region, escape,
type placement, semantic projection, interface reconstruction, monomorphization, capability
discovery, and variant-tripwire passes name the emitted concrete expression explicitly; an owner
also proves that no abstract form reaches any of them.

`R` and every `pkg.csv` declaration remain nominal language/interface identities. Two records with
the same field spellings in different nominal scopes are distinct. Fingerprints encode that nominal
identity plus the complete ordered reachable definition graph; `SoaPlain` makes each reachable leaf
one primitive. The runtime descriptor is intentionally structural execution metadata and cannot
erase the nominal compiler/cache distinction.

The frontend/interface key consumes canonical root and empty-internal module source, the retained
generic body, its checked-operation identity, and the complete nominal `R` graph. The object action
key additionally consumes the existing target triple/object format, resolved CPU and feature set,
profile, pass pipeline, optimization/relocation/code model, LLVM version, runtime-LTO mode/digest,
PGO mode, exports, and dependency hashes. Final-link identity consumes its ordinary ordered object,
runtime, and library inputs. CSV adds no ambient runtime feature detection or data-dependent cache
input; it does not remove any existing explicit build input.

MIR retains the same struct id and children and yields private i32 status plus a zeroed SoA output
slot. Status 0 constructs `Ok`; 1 constructs `Err(Invalid)`; 2 constructs
`Err(LimitExceeded)`; private `-1` and every other i32 reach `process.abort()`. The runtime returns
`-1` for a malformed private ABI and never returns another status. The error edge publishes no output
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
three times and mutate only the supplied arena and output header after validation, so `readonly`,
`readnone`, `nofree`, and `willreturn` would be false.

For the complete call, a nonnull arena pointer is aligned to `align_of::<Arena>()` and denotes one
live exclusive `Arena`; a nonnull output pointer is aligned to `align_of::<AlignStr>()` and denotes
one writable exclusive header. They are disjoint from each other and every immutable range. Input
length zero permits either pointer and causes no dereference; positive input length requires a
nonnull readable byte range of exactly that length. The descriptor count is positive, converts
exactly to `usize`, and has no CSV-specific upper bound; its nonnull pointer is aligned to
`align_of::<CsvField>()` and denotes that many immutable records.
Every positive descriptor-name length has a nonnull readable byte range of exactly that length.
All lengths, counts, address additions, and byte products must fit both their declared integer type
and the target pointer-offset domain before a reference, slice, or field load is formed.

Input bytes, descriptor records, and descriptor-name bytes remain immutable for the complete call.
Input may lie in a prior live allocation owned by the same arena because a new arena allocation
cannot overlap it. The runtime treats a mechanically detectable negative/null/misaligned/overflowing
representation as `-1` before typed access. Once those guards pass, an exact-compatible unsafe
source caller must establish dereferenceability, lifetime, provenance, immutability, and overlap;
compiler-produced calls establish them from checked regions and aligned compiler-owned static
descriptors before lowering. The guards cannot prove that an arbitrary nonnull address owns the
promised range and do not make an otherwise invalid unsafe call defined.

`CsvField` is one target-native `#[repr(C)]` record matched by LLVM's non-packed struct:

```text
{ name_ptr: ptr, name_len: i64, name_hash: u64, tag: i32, reserved: i32 }
```

The record's target size/alignment and all five field offsets come from this exact `repr(C)` type;
the descriptor global uses at least that alignment. `name_hash` is the canonical
`align_hash::wyhash(name_bytes, WY_SEED)` with `WY_SEED = 0`. Codegen and runtime both call that one
shared implementation rather than maintaining parallel hash algorithms. `align_hash` owns the
algorithm and seed convention; `pkg.csv` owns descriptor formation, validation order, and reuse of
the authenticated stored value. The runtime recomputes it
during name validation and rejects a mismatch with private
`-1`; later header projection reads the validated stored value without rehashing the descriptor.
`reserved` is zero. `tag` packs
`(signed << 16) | (kind << 8) | width`: integer kind 0 with width
1/2/4/8 and the sign bit; bool kind 1 width 1; float kind 2 width 4/8; str kind 3 width 16; char kind
4 width 4. All other bits are zero. Names are nonempty, pairwise byte-unique static source identifiers in declaration
order: first byte ASCII `_`/letter and remaining bytes ASCII `_`/letter/digit, excluding exact
reserved tokens `fn`, `return`, `mut`, `pub`, `module`, `import`, `if`, `else`, `true`, `false`,
`arena`, `task_group`, `match`, `loop`, `break`, `template`, `unsafe`, `extern`, and `as`. NUL,
non-ASCII, invalid UTF-8, punctuation, a reserved token, and every other spelling are malformed
private ABI `-1` when mechanically inspected. The runtime validates every record and hashes each
name exactly once before input access or arena effects, but does not perform a pairwise descriptor-
name scan. Source record validation and checked descriptor emission prove uniqueness for every
compiler-produced call; an exact-compatible unsafe caller must provide the same declaration-order
unique-name invariant. Violating that invariant is an unsafe-contract violation and is not promised
private `-1`; the runtime does not authenticate it. The descriptor is compiler-owned inspection data, not reflection: no
pointer or field metadata reaches source.

Descriptor validation therefore visits exactly `F` records and hashes exactly the `B` descriptor-
name bytes. `Absent` mode performs no later name lookup. `Present` mode has `H <= 1024` physical
header names and performs one bounded fixed-table lookup per descriptor; even with confirmed hash
collisions its descriptor/header candidate comparisons are at most `F * H`, and compared descriptor
name bytes are at most `B * H`. The implementation exposes these test counters. This keeps work
linear in an uncapped schema width under the fixed Present-header bound and gives wide common-prefix
schemas no quadratic descriptor-to-descriptor path.

The exact native order is writable output, live arena, output zeroing; header tag, line-ending tag,
then row bound; positive representable descriptor count/table/fields/names; input representation;
complete UTF-8 validation; CSV/header/data/layout;
and only then a nonempty allocation/fill. Mechanically detected malformed private ABI returns `-1`; a negative row bound
returns 1 before descriptor inspection; public parse/conversion errors return 1; public bound/layout
errors return 2. This is the same order as the preceding public precedence ledger.

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
| descriptor hashes | source names `a`, `_`, `field_1025`, `common_prefix_0001` | exact canonical wyhash-seed-0 u64 values `0x28d2053309d28531`, `0xa648b00f869de2d5`, `0x3009310c40178c99`, `0x94516051bd4291e2`; codegen and runtime use the shared `align_hash` implementation, while an independent reference vector also pins the settled `score` canary `0x1300a50cfadb78d9` |

Byte offsets 0..7 and target endian twins must produce identical values. Malformed-input mutation
walks every comma, quote, doubled quote, CR, LF, BOM, header, numeric digit/sign/dot/exponent, width,
and EOF boundary. A separate type-to-column oracle checks the exact SoA layout and projected values
without invoking the CSV decoder. A test-only reference encoder independently maps the semantic
field vectors to valid quoted/unquoted CSV bytes before decode comparison; it is fixture machinery,
not a public `pkg.csv` encoder or a source of production parser code.

## Complexity and performance boundary

Positive-length success performs exactly three sequential input walks: one UTF-8 prevalidation and
two decode passes. It makes one exact arena allocation for nonempty output and direct column writes.
It materializes no AoS row and performs no transpose, heap
allocation, per-row allocation, per-field owned string, or copy of unselected/clean text. Header
lookup uses bounded stack state and hash-plus-equality; hash collision cannot affect semantics.
Descriptor preparation visits `F` records and hashes `B` name bytes once. Absent mode performs no
name lookup; Present mode performs at most `F * H` candidate comparisons and `B * H` compared
descriptor bytes under its fixed `H <= 1024` header bound, with no descriptor-to-descriptor scan.

The implementation should share the existing architecture-parity byte-classifier machinery where
it improves the quote/comma/newline scan, with x86, ARM64, and scalar paths matching one oracle. No
specific SIMD width, throughput, latency, allocation address, or speedup is a public promise.
`bench/csv_decode` is a local non-gating measurement over narrow/wide, quoted/clean, LF/CRLF, and
scalar/string corpora. It records input bytes, rows, physical/selected columns, normalized bytes,
UTF-8 and decode passes, arena allocations, and field conversions from producer-owned counters; it exists to catch
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
| Public formation and identity | Canonical root plus empty `pkg.csv.internal.descriptor`; exact four public types/one public generic wrapper/one compiler-private internal operation spelling and no private source item; abstract `Ty::Param(p)` plus matching `SoaParam(p)` only under `SoaPlain`, then concrete recheck to `Ty::Struct(id)`; complete existing `SoaPlain` domain with no schema-count cap, including explicitly laid-out records; no application import/same-name interception; direct/imported/local/function-field/control-joined calls. | Root/internal source and interface byte/hash owner; producer acceptance proves the public generic body has no private-item reference; abstract wrapper and generic-forwarder formation; concrete substitution/recheck; 1/1024/1025-field and larger generated schemas under Absent, Present missing-coverage and 1025th-physical-header precedence; explicit `layout(C)`/`align(N)` positives; wrong/missing bound and abstract-emission negatives; schema/module/body/internal-item mutations; parameterized call-target shapes; whole/per-unit/generic monomorphs. |
| Evaluation, checked HIR, and MIR | Input/out/options once left-to-right; terminating child stops later work; exact child/type/id/effect/region records; status mapping and no error output; every traversal/replay/validation pass explicit. | Variant sweep tripwire; one-field HIR mutations; source termination/control matrix; MIR status/output and process-abort assertions. |
| Parallel-worker eligibility | `CsvDecode` remains Pure for sequential in-memory use, but `region` is non-Send independently of effect. One shared fail-closed source/HIR worker-transfer authority consumes the existing `BorrowFact`/`CallableProvenance` graph for `spawn` and every staged/terminal `ArrayParMap` callable. It follows concrete `ClosureTarget`/`ClosureCapture` paths recursively through nested function values, moves, reassignment, and control joins; direct/imported/concrete-indirect helper summaries are translated against completed actual values. A local reachable `ArenaHandle` rejects before lifted-worker publication, MIR, generated identity, runtime call, or allocation; an explicit parameter root remains in summary `params` and a lifted environment slot in `captures` for caller-side checking, with only public explicit ordinals serialized as `parallel_transfer_params`. Unknown target/environment provenance selects every compatible input, while a known noncapturing function has an empty environment. Codegen defensively rejects a direct `ArenaHandle` in handcrafted parallel MIR; checked HIR owns opaque nested function environments. Sequential direct calls, local closures, `map`, and `reduce` may retain the capability under the ordinary lexical region rule. | Direct and helper-wrapped `decode` captures plus one- and two-level function values closing over `out` in `spawn` and staged/terminal `par_map`; local move/reassignment and `if`/`match`/`else`/loop joins with one unsafe branch; direct/imported/concrete-indirect/unresolved helper transport; raw malformed `ArrayParMap` HIR target/environment/summary mutation and direct `ParMapParallel`/`ParMapReduce` MIR `ArenaHandle` captures; whole-program/per-unit and concrete `CsvDecode` monomorph parity; existing spawned-region negative; known noncapturing and region-free capturing function values plus sequential Pure direct/closure/map/reduce positives; failing paths prove no worker/capability/context/kernel/call/allocation publication. |
| CSV lexical closure | BOM position/count; unquoted/quoted/doubled quote; comma; spaces/NUL/UTF-8; CRLF/LF choice; quoted line breaks; final EOL/EOF; blank/trailing-empty records. | Independent grammar oracle; exhaustive bounded mutation/fuzz corpus; exact accepted/rejected byte vectors for every lexical state and EOF transition. |
| Header/projection closure | Present/Absent; declaration/physical reorder; unknown skip; all names nonempty/unique; every declared name once; 1/1024/1025; hash collisions; exact width per row. | Fixed vectors plus generated unique/duplicate/collision tables; conversion counters prove unknown fields skipped; mapping-to-column oracle. |
| Typed conversion | Every integer width/sign edge; float lexical/exact bits/overflow/underflow; bool case; char scalar count; empty/whitespace; selected versus unselected. | Parameterized field-kind oracle and rejected-next twins; optimized/unoptimized, endian, and whole/per-unit parity; no conversion of extras. |
| Bounds and precedence | Invalid enum/HIR, negative/zero/exact/next rows, header cap, normalized-byte arithmetic, every SoA offset/size/alignment and target-size overflow; output/arena before options, options before descriptor/input, earliest lexical/conversion/width before row limit; no later scan after terminal error. | Multi-invalid pairwise precedence matrix including negative bound plus malformed descriptor; target-representability twins; counters for last inspected row/field and zero allocator calls. |
| Allocation and atomicity | Pass 1 no heap/arena; pass 2 one exact aligned arena block only after full success; no AoS/transpose; zero-row null/zero; OOM abort; fill invariant abort; error leaves arena unchanged. | Arena/heap allocation and byte counters; failpoints at layout/allocation/fill boundaries; exact one-block topology inspection; pre/post arena cursor owner. |
| String ownership and regions | Clean unquoted/quoted input views; doubled-quote normalized tail; mixed cells; type-level input+out retention; primitive-only out retention; input already rooted in `out`; direct/field/Option/Result/join/projection/pipeline/storage generations; implicit `str` borrow of fresh and control-wrapped owned `string` temporaries, including later-argument termination. | Pointer-range classification plus exact bytes; distinct-owner and same-arena input/output twins; input/out escape and mutation negatives; primitive input-release positive; fresh function/literal and block/`if`/`match`/`else`/`?`/`map_err`/loop temporary owners; cleanup-carrying and no-cleanup later-argument sinks; generic and control-flow carrier matrix. |
| SoA layout and pipeline | One shared layout authority; direct declaration-order columns; every field alignment/base residue; index/projection/window/where/map/reduce; str columns read-only; no new pipeline path. | Independent layout oracle; current SoA regression bundle; base residues 0..7; generated mixed-width schemas and str pipeline owners. |
| Native ABI and descriptor | Exact A123 signature/key/symbol/attributes/export; input null/zero versus positive-readable range; nonnull correctly aligned live arena/output; positive representable uncapped `CsvField` count and pointer aligned to `align_of::<CsvField>()`; complete table/tag/reserved/source-identifier/name-hash validation after options; compiler-owned and unsafe-precondition declaration-order uniqueness without a pairwise runtime scan; complete input UTF-8 validation before BOM/CSV and allocation; negative/overflow pointer products; no Rust reference/slice before guards; output zeroing; no unwind. | Registry/golden/export/compatibility mutation owners; direct runtime matrix for mechanically detectable null/zero, null/positive, unaligned descriptor/arena/output, zero/negative/overflowing counts, invalid name start/continuation/non-ASCII/NUL/hash, invalid UTF-8 input, and negative-option-plus-malformed-descriptor/input precedence without dereferencing an invalid address; shared `align_hash::wyhash` seed-0 codegen/runtime identity plus independently derived byte/hash goldens and settled `score` canary; emitted unique/duplicate-field producer mutation and unsafe-contract owners; wide/common-prefix `F/B/H` counters prove one descriptor pass and the exact `F * H`/`B * H` bounds; rt-LTO on/off. |
| Cache and distribution | Root/internal source, generic body, concrete nominal schema graph, checked op, descriptor, runtime key/body invalidate their exact frontend/object/link identities; all existing explicit target/CPU/features/profile/pipeline/runtime/link inputs remain keyed; unrelated source and ambient runtime detection do not; vendorable/prebuilt inventory changes only when source ships. | Whole/per-unit root/internal/public/schema/runtime and explicit-target edit-revert twins; prebuilt add/remove/layout owners; no-op, ambient-runtime, and unrelated-unit hits. |
| Performance shape | One UTF-8 prevalidation plus two decode passes; one descriptor validation/hash pass with no descriptor-to-descriptor comparison; Present projection bounded by `H <= 1024`; direct selected-column fill; unknown/clean text not copied; no heap/per-row/AoS/transpose; SIMD implementations match scalar when present. | Producer counters prove wide/common-prefix descriptor/header work bounds; `bench/csv_decode` measures generated wide and common-prefix cases locally; scalar/x86/ARM64 equality owners; benchmark remains non-gating. |

## Sources of truth and author consistency pass

This English ledger, `docs/impl/pkg-design/ja/csv.md`, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, `docs/history.md`,
`docs/open-questions.md`, `docs/impl/07-roadmap.md`,
`docs/impl/03-types.md`, `docs/impl/04-mir.md`, `docs/impl/05-backend-llvm.md`,
`docs/impl/11-parallel-execution-optimization.md`,
`docs/impl/12-pipeline-closure-memory-io-simd-audit.md`,
`docs/impl/17-library-boundary-prerequisites.md`, `docs/impl/19-hir-validation-ledger.md`,
`docs/impl/20-runtime-abi-ledger.md`, and `HANDOFF.md`
must agree before implementation. User guides and vendorable/prebuilt inventories must continue to
describe only shipped packages until implementation activates the package.

The author-side pass must prove:

- every public record above has exact source order, tag, type, default, evaluation, error, effect,
  ownership, lifetime, allocation, owner, identity, prerequisite, and acceptance evidence;
- Header × line ending × empty/header-only/data × quoted/clean/doubled-quote × selected/extra × every
  field type × zero/exact/next limit has one field-presence, row/column order, and error rule;
- public CSV text is UTF-8 with NUL as data; raw input rejects invalid UTF-8 privately before CSV;
  descriptor names use the exact ASCII source-identifier/non-keyword grammar and reject NUL;
  checked emission proves declaration-order descriptor uniqueness without a pairwise runtime scan,
  unsafe callers promise the same invariant, header equality is byte-exact, and no borrowed input
  or compiler descriptor is retained beyond the result's stated region;
- every multi-invalid product follows the written validation precedence before arena effects;
- Pure-effect replay and the independent non-Send provenance rule reject `region` at every `spawn`
  and `par_map` worker boundary, including through nested callable environments and helper
  summaries, while leaving known region-free function values and sequential calls/closures admitted;
- abstract generic checking, generic forwarding, concrete substitution/recheck, and emitted-HIR
  rejection of every symbolic row form have one exact representation and owner;
- the one-pass descriptor work bounds, two-pass/direct-column, and clean-versus-normalized
  allocation promises have producer-owned counters and no benchmark is used as a correctness gate;
- A123 fixes every scalar width, tag, descriptor-name hash, parameter order, pointer role, validation precondition,
  null/length/alignment/range rule, status, attribute, output initialization, and activation count
  in both directions;
- checked HIR, MIR, LLVM, runtime, package interface, whole/per-unit compilation, monomorphization,
  cache identity, and prebuilt inventory agree atomically;
- all examples parse with accepted syntax and declarations remain separate from positional calls;
  and
- no implementation cell consumes streaming, nullable SoA, owned rows, dialect inference, or any
  other later capability.

## Independent design review

The fresh full-diff review of candidate `5f9f978f` reported one P1 and three P2 findings. The
authoritative ledger changed first, and the synchronized repair closes the complete finding set:

| Finding | Ledger-first repair |
|---|---|
| P1: a public generic template cannot reference a private helper omitted from its interface | Replace the private source bridge with the shipped compiler-private `pkg.csv.internal.descriptor.decode` spelling in an exact empty internal module. Package-internal import sealing, root-only formation, retained generic-body/interface identity, and whole/per-unit monomorph owners close the boundary. |
| P2: the options ledger preceded descriptor inspection while the native sequence did the reverse | Split safe output/arena validation from descriptor validation; validate header tag, line-ending tag, and row bound before the complete descriptor. Pin negative-bound-plus-malformed-descriptor precedence. |
| P2: the cache sentence excluded CPU features even though object keys require them | Preserve every ordinary explicit target/CPU/features/profile/pipeline/runtime/link input and exclude only ambient runtime detection or data-dependent CSV state. Add explicit-target edit/revert owners. |
| P2: summary prose promised an allocation for zero-row success | Make `{ null, 0 }` and zero allocation exact for `N == 0` in the ledger and every normative summary; the one-block promise applies only to `N > 0`. |

The required strategy re-review of candidate `b4d15acd` found three P1 and two P2 issues. A new P1
on the second review reopened the closure matrix under `generic-ffi-schema`; this is a boundary
redesign, not another line-local patch:

| Finding | Reopened-matrix repair |
|---|---|
| P1: concrete-only `CsvDecode.struct_id` made the generic wrapper unformable during abstract checking | Replace `struct_id` with exact `row: Ty`. The discarded template HIR admits only bound-matching `Ty::Param(p)` plus `SoaParam(p)` result; retained-AST monomorphization rechecks to `Ty::Struct(id)`, and emitted HIR rejects every symbolic form. Add wrapper, forwarding, substitution, and abstract-emission owners. |
| P1: empty input can validly be `{ null, 0 }`, but the ABI did not define it | Accept either pointer at zero length without dereference or slice formation; require a nonnull complete readable range only for positive length. Add null/zero and null/positive direct-runtime twins. |
| P1: the typed descriptor table had no alignment precondition | Require `align_of::<CsvField>()`, validate it before any typed load/reference/slice, align compiler globals accordingly, and add unaligned direct-runtime and extern-contract owners. Audit arena/output alignments in the same class. |
| P2: a package-only natural-layout rule narrowed the existing `SoaPlain` bound | Remove the extra rule. SoA layout ignores AoS record layout; explicit `layout(C)` and `align(N)` records remain valid and become positive owners. |
| P2: the precedence summary placed every private abort before option errors | Split malformed output/arena ABI from malformed descriptor/input ABI around the option phase, matching the numbered native order and multi-invalid owner. |

The next fresh review of candidate `5b1b6aaf` found one P1 and two P2 issues. The P1 showed that the
reopened bound matrix had closed layout but not cardinality, so it reopened again under
`bound-capacity-raw-text`:

| Finding | Reopened-matrix repair |
|---|---|
| P1: the 1024 schema-field cap still narrowed `SoaPlain` | Remove the schema cap from sema, HIR, descriptor count, and every summary. Keep 1024 only as the physical Present-header cap. Absent mode admits 1025-field and larger schemas; Present returns `Invalid` for missing coverage unless a 1025th physical header column first selects `LimitExceeded`. |
| P2: the raw input ABI omitted invalid UTF-8 semantics | Add one complete allocation-free UTF-8 prevalidation after pointer/descriptor phases and before BOM/CSV. Invalid bytes return private `-1`; success therefore has one UTF-8 walk plus two decode passes. |
| P2: descriptor names omitted embedded-NUL semantics | Validate the exact ASCII source-identifier grammar and reserved-token exclusion during the descriptor phase. NUL and every non-source spelling return private `-1` before input access. |

The fresh review of candidate `5dbebb23` found one P1 and one P2. The missed lifetime cell reopened
the closure matrix under `descriptor-work-temporary-owner`:

| Finding | Reopened-matrix repair |
|---|---|
| P1: an auto-borrowed owned input temporary could be dropped while returned string columns still view it | Make the existing path-local synthetic owner part of `CsvDecode` input storage provenance. Retain it at `Frame` through fresh function/literal and every block/`if`/`match`/`else`/`?`/`map_err`/loop wrapper; reject return escape, and close cleanup/no-cleanup later-argument termination. Primitive schemas publish no input dependency. |
| P2: uncapped descriptor uniqueness used a quadratic pairwise scan | Keep the complete `SoaPlain` domain and remove descriptor-to-descriptor comparison. Add the pinned `name_hash` field so one runtime validation/hash pass authenticates and retains each lookup hash. Source validation and checked emission are the uniqueness authority; exact-compatible unsafe callers promise the same invariant. The pass visits `F` records and `B` name bytes; bounded Present lookup proves at most `F * H` candidates and `B * H` compared bytes for `H <= 1024`, with wide/common-prefix counter owners. |

The fresh review of candidate `c9383e6f` found one P1 and one P2. The parallel-safety finding
reopened the closure matrix under `parallel-region-abi-history`; the repair strengthens the one
general region rule instead of adding a CSV-only effect or allocation exception:

| Finding | Reopened-matrix repair |
|---|---|
| P1: Pure `decode` could be called from `par_map` by capturing the Copy destination region and allocate concurrently into one arena | Make `region` explicitly non-Send to every parallel worker, independent of inferred effect. Use one fail-closed source/HIR capture authority for `spawn` and `ArrayParMap`; reject before lifted-worker publication, MIR, generated identity, call, or allocation. Preserve Pure sequential direct/closure/`map`/`reduce` use, make codegen reject a direct handcrafted-MIR capability before publication, and add direct/helper/whole/per-unit/concrete-monomorph plus malformed-HIR/MIR owners. |
| P2: the frame activation history still named A123 as the next unreserved shape | Keep its then-current count as history, but update the ABI ledger and frame/log English/Japanese mirrors to record the later A123 reservation and current A124 next design shape. Audit every `A12x` next-unreserved statement as the same drift class. |

The fresh review of candidate `065bf7b9` found one P1 and one P2. A new P1 after the revised diff
reopened the closure matrix under `transitive-send-canonical-hash`; the repair reuses the existing
callable-provenance and canonical-hash authorities instead of adding parallel representations:

| Finding | Reopened-matrix repair |
|---|---|
| P1: an exact-type capture check lets a sequential closure close over `out`, then lets a worker capture and invoke that function value | Consume the existing `BorrowFact` trie and concrete `CallableProvenance` at every worker-transfer sink. Recursively follow `ClosureTarget`/`ClosureCapture` paths through nested closures, moves, reassignment, and control joins; translate full same-program helper summaries and imported `parallel_transfer_params` against completed actual values; fail closed on unavailable target/environment provenance. A known noncapturing function has an empty environment. Checked HIR owns this transitive fact before MIR; backend's malformed-MIR defense remains exact for the direct self-describing `ArenaHandle` case. Add one-/two-level closure, helper, join, malformed-HIR, whole/per-unit, concrete-monomorph, and region-free controls. |
| P2: CSV specified a second FNV-1a implementation after Align settled one canonical non-cryptographic hash | Define `CsvField.name_hash` as `align_hash::wyhash(name_bytes, WY_SEED)` with seed 0. Codegen and runtime call the same crate implementation; retain independent semantic vectors for `a`, `_`, `field_1025`, `common_prefix_0001`, and the settled `score` canary so descriptor constants and validation cannot drift. |
