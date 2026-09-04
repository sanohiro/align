# std.xml — public contract and implementation design

> 🌐 **English** · [Japanese](./ja/xml.md)

> **Status:** IMPLEMENTED 2026-09-05.

## Authoritative public-contract ledger

This table is the authority for the first `std.xml` capability. Later prose and implementation may
make a cell more explicit but must not widen it. V1 is one bounded, forward-only reader for an
in-memory UTF-8 XML 1.0 document. It is not a DOM, validating parser, namespace processor, query
language, serializer, file or network loader, or general entity processor.

| Public surface | Exact inputs, defaults, validation, and evaluation | Exact result, errors, and effects | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner, artifact, and cache identity | Prerequisite and acceptance owner |
|---|---|---|---|---|---|
| `xml.event { Start, End, Text }` | Closed source/discriminator order is exactly `Start = 0`, `End = 1`, `Text = 2`. There is no comment, declaration, processing-instruction, attribute, EOF, error, or unknown event tag. | Copy and Pure. A source-formed value has no error or abort path. `None` from `next` is the sole EOF representation. | The settled one-field enum aggregate `{ i32 tag }`; no borrow, allocation, Drop, numeric conversion, or retained state. | `align_sema` owns the unique builtin nominal definition and qualified type/variant resolution behind `import std.xml`. HIR/MIR retain the ordinary enum aggregate; interface and cache identity use the existing nominal named-type grammar. | Exact tag/order, construction/match, qualified import, malformed checked-HIR/MIR, interface round trip, and cache-mutation owners. |
| opaque Move `xml.reader` | A reader exists only after one successful `xml.parse`. It is a bound, initialized, single-owner forward cursor. It has no public constructor, fields, clone, reset, seek, raw-input accessor, conversion, or default. | Pure as an in-memory value. Its mutable cursor prevents concurrent/shared `next`; the type is not Send, Sync, printable, comparable, collection-storable, capturable by parallel work, global, or native-source-compatible. | Owns the consumed input allocation and one runtime shell. Move transfers both; Drop frees both exactly once. It may move through the ordinary admitted handle carriers and scalar tagged payloads, subject to existing region/Drop rules. | New `Ty::XmlReader` / `Scalar::XmlReader`, checked HIR/MIR ownership, canonical type-codec leaves `72` / `48`, runtime key selection, and Drop key. Whole-program and per-unit identity include the imported module, operations used, compiler/runtime implementation, target, and ordinary build inputs; no locale, environment, filesystem, network, MIME, or namespace registry participates. | Move-in/out/nulling, replacement, direct/Result/user-sum return, `if`/`match`/`else`/`?`/`map_err`/loop/early exits, malformed cleanup, whole/per-unit/generic, exact type codec, and one-shell/input-free owners. |
| `xml.parse(input: string) -> Result<xml.reader, Error>` | Evaluates and consumes `input` exactly once. Input is already valid UTF-8 by `string`; its empty representation is exactly canonical `{null, 0}` and owns no allocation, while a positive length names exactly one allocator-compatible owned allocation. Parsing applies the exact accepted grammar below. One optional leading U+FEFF is consumed. An optional XML declaration must be first, must say exact version `1.0`, may say only case-insensitive `UTF-8`, and may say `standalone='yes'|'no'` in XML declaration order. There is exactly one root. Maximum element depth and attributes on one element are each 256, inclusive; the next item is invalid. There are no options or ambient encoding/MIME defaults. | `Ok(reader)` only after complete-document validation. Empty input, malformed XML, forbidden markup, unsupported declaration/encoding, invalid XML character/name/reference, duplicate attribute, mismatched nesting, second root, or either bound returns `Error.Invalid`. OOM and an impossible compiler-produced status hard-abort. An unsafe caller gets `AL_INVALID` for the mechanically detectable pointer-shape, representation, and supplied-range alias failures below; any pointer that passes those shape checks but lacks required provenance, lifetime, accessibility, or exclusivity violates the unsafe contract and is not promised a safe abort. Pure; no I/O, lookup, logging, callback, entity fetch, or partial publication. | The source operation consumes the input once. After mechanical preflight the runtime accepts either no allocation for canonical empty or the positive-length allocation. Public XML failure releases that responsibility exactly once, a no-op for canonical empty; success retains the unchanged positive-length allocation and allocates exactly one fixed-size reader shell. A mechanically rejected unsafe call leaves output untouched and accepts no input ownership. Validation uses fixed 256-entry stack scratch and no owned allocation. No input copy, event tape, tree, namespace/entity table, or per-element allocation exists. | Checked `XmlParse` HIR/MIR; runtime `align_rt_xml_parse` on existing shape A08 `i32(ptr, i64, ptr)`. Runtime status is `0 = success`, `-1 = public Error.Invalid`, positive `AL_INVALID = mechanically detectable private ABI rejection`; no other result is accepted. Capability/runtime fingerprints select XML only when used. | Shipped owned string/Result/Move handle patterns; W3C and cloud-response corpus, declaration/grammar/character/name/reference Cartesian matrix, exact limits, first-error/no-publication, allocation/free, direct/imported/function-value, whole/per-unit, cache, ABI, and optimized/unoptimized owners. |
| `r.next() -> Option<xml.event>` | `r` is mutably borrowed and advances exactly once. It emits the event stream below. Every empty element emits `Start` and then one synthesized `End`. Repeated calls after EOF return `None`. | Total for a compiler-produced valid live reader, Pure, and allocation-free. It never returns `Error`; a detectable null, moved, or malformed private state hard-aborts before input access. A raw caller's pointer/access/exclusivity violation is outside the unsafe ABI contract. | Borrows the reader for the call and retains no external value. It mutates only cursor/current-event fields in the shell. The returned Copy enum has Static lifetime. | Checked `XmlNext`; `align_rt_xml_next` on A03 `i32(ptr)`, returning `0 = None`, `1 = Start`, `2 = End`, `3 = Text`; every other i32 aborts. | Exact event order, empty-element pair, skipped markup, repeated EOF, mutable receiver, no allocation, malformed state, and direct/imported/generic/whole/per-unit owners. |
| `r.name() -> str` | Valid only while the current event is `Start` or `End`. Returns the exact lexical element `Name` after UTF-8 decoding, with original case and colon bytes. An `End` synthesized for an empty element returns that element's start-tag name. A call before first `next`, on `Text`, or after EOF aborts as a programmer error. | Pure, total in the admitted state, and allocation-free. It performs no namespace expansion, prefix resolution, Unicode normalization, case folding, or entity decoding. | The zero-copy view points into the reader-owned input and is region-tied to the reader's current cursor state. It cannot outlive the reader or remain live across mutable `next`; `.clone()` is the explicit escape. | Checked `XmlName`; `align_rt_xml_name` on A19 `i32(ptr, ptr)`. Complete pointer-shape, shell-field, internal-range, and output-alias preflight failure leaves output untouched. It then zeros `{ptr, i64}`; wrong-state failure leaves canonical zero, while success installs the view. Codegen aborts on nonzero status. | Start/explicit-End/synthesized-End names; Unicode/colon names; current-state negatives; region escape, clone, move/Drop, raw view, and ABI output owners. |
| `r.attribute_count() -> i64` | Valid only on `Start`. Returns the exact source-order attribute count in `0..=256`; namespace declarations count as ordinary attributes. Wrong current state aborts. | Pure, total in the admitted state, and allocation-free. | Borrows the reader only for the call; result is Copy/Static. | Checked `XmlAttributeCount`; `align_rt_xml_attribute_count` on A29 `i64(ptr)`. A negative or value above 256 aborts. | Zero/exact-limit counts; Start/self-closing; wrong-state/malformed-state; no-allocation and ABI owners. |
| `r.attribute_name(index: i64) -> str` | Valid only on `Start`; `index` is zero-based source order and must be in `0..<attribute_count`. Returns the exact lexical XML `Name`. A negative/out-of-range index or wrong state aborts. | Pure, total in the admitted state, and allocation-free. No namespace expansion, prefix resolution, Unicode normalization, or case folding. | The zero-copy view points into reader-owned input and has the same current-cursor region as `name`; it cannot cross `next` or reader Drop without `.clone()`. | Checked `XmlAttributeName`; `align_rt_xml_attribute_name` on A20 `i32(ptr, ptr, i64)`. Complete pointer-shape, shell-field, internal-range, and output-alias preflight failure leaves output untouched. It then zeros the output; wrong-state/index failure leaves canonical zero, while success installs the view. Codegen aborts on nonzero status. | Source order, Unicode/colon/`xmlns` names, bound edges, region escape/clone, wrong state, malformed ABI, and whole/per-unit owners. |
| `r.attribute_value(index: i64) -> string` | Same admitted state and index rule as `attribute_name`. Returns the complete XML 1.0 normalized value: predefined/numeric references are decoded; literal tab/LF/CR are normalized to spaces after document line-end normalization; a character reference contributes its referenced character and is not re-normalized. Quotes are delimiters, not data. | Pure. Invalid state/index aborts. A valid call returns one fresh owned string, including a canonical zero-allocation empty string; OOM aborts. No error, fallback, lazy decode, or retained cache exists. Repeated calls repeat the allocation/copy. | Borrows the reader during decoding only. A nonempty result owns exactly one right-sized allocation and freely outlives cursor/reader; empty is canonical `{null,0}`. The reader and input are unchanged. | Checked `XmlAttributeValue`; `align_rt_xml_attribute_value` on A20 `i32(ptr, ptr, i64)`. Complete pointer-shape, shell-field, internal-range, and output-alias preflight failure leaves output untouched. It then zeros the owned output; wrong-state/index failure leaves zero, and successful fill publishes only the final allocation. | All quote/entity/character/line-end combinations, empty/nonempty exact allocation, repeated calls, state/index negatives, failure no-publication, Drop, and ABI owners. |
| `r.text() -> string` | Valid only on `Text`. An ordinary text run is one maximal nonempty sequence of adjacent XML `CharData` and `Reference` productions; references, including consecutive references, do not split it and are decoded after document line-end normalization. A comment, CDATA section, child start/end tag, or enclosing end tag ends the run. Each nonempty CDATA section is a separate `Text` whose literal character content is returned after line-end normalization; entity-looking bytes remain literal. Comments are skipped. Wrong current state aborts. | Pure. Returns one fresh owned string; nonempty output is exact, empty is never emitted as `Text`. OOM aborts. Repeated calls repeat allocation/copy. | Borrows the reader only during the call. Result owns one right-sized allocation and freely outlives cursor/reader. The reader and input are unchanged. | Checked `XmlText`; `align_rt_xml_text` on A19 `i32(ptr, ptr)`. Complete pointer-shape, shell-field, internal-range, and output-alias preflight failure leaves output untouched. It then zeros the owned output; wrong-state failure leaves zero, and successful fill publishes only the final allocation. | Exact `CharData`/single/consecutive-reference coalescing and comment/CDATA/child boundaries, whitespace/line ends, non-ASCII UTF-8 acceptance/preservation, NUL rejection at parse, repeat allocation, wrong state, Drop, malformed ABI, and whole/per-unit owners. |

## Source surface and use

```text
xml.event { Start, End, Text }
xml.reader

xml.parse(input: string) -> Result<xml.reader, Error>
r.next() -> Option<xml.event>
r.name() -> str
r.attribute_count() -> i64
r.attribute_name(index: i64) -> str
r.attribute_value(index: i64) -> string
r.text() -> string
```

`event` and `reader` are available only through their qualified `xml.event` and `xml.reader`
spellings after `import std.xml`. The existing unqualified I/O `reader` is unchanged. There is no
bare XML alias, overload, default option, declaration API, or source-visible unsafe entry.

Declarations and calls remain separate:

```align
import std.xml

fn first_key(body: string) -> Result<string, Error> {
  mut doc := xml.parse(body)?
  loop {
    event := doc.next() else { break }
    match event {
      Start => {
        if doc.name() == "Key" {
          next := doc.next() else { return Err(Error.Invalid) }
          match next {
            Text => { return Ok(doc.text()) }
            Start => {}
            End => {}
          }
        }
      }
      End => {}
      Text => {}
    }
  }
  Err(Error.NotFound)
}
```

The input is an owned `string`, not `str`, because the reader must retain it. An HTTP response body
is a borrowed byte view; the consumer validates UTF-8 and writes the copy explicitly:

```align
response := http.parse(data)?
body := response.body()
text := body.as_str()?
doc := xml.parse(text.clone())?
```

Passing an owned string local transfers it without copying. This is the only parse path.

## Accepted XML 1.0 profile

The lexical authority is [XML 1.0 Fifth Edition](https://www.w3.org/TR/xml/), with the explicit
profile below. “Well formed” here means the document entity satisfies the applicable XML 1.0
productions and well-formedness constraints after removing the forbidden constructs; it does not
mean DTD validity or Namespaces-in-XML conformance.

The consumer acceptance corpus includes the published
[Amazon S3 ListObjectsV2](https://docs.aws.amazon.com/AmazonS3/latest/API/API_ListObjectsV2.html)
and [Azure List Blobs](https://learn.microsoft.com/en-us/rest/api/storageservices/list-blobs) XML
response forms. Provider status is not parser truth: S3 explicitly permits an HTTP 200 response
whose body is invalid XML, so package consumers must keep `xml.parse` failure distinct from HTTP
status success.

V1 accepts exactly these document components:

- one optional leading U+FEFF byte-order mark;
- one optional XML declaration at absolute document start after that mark;
- XML `S` whitespace and comments before and after the root;
- exactly one properly nested root element, ordinary start/end tags, empty-element tags, attributes,
  character data, CDATA sections, comments, the five predefined entity references, and decimal or
  hexadecimal numeric character references; and
- UTF-8 input only. The declaration is absent or names case-insensitive `UTF-8`; no transport or
  caller encoding label can override the actual `string` encoding.

V1 rejects before reader publication:

- every `DOCTYPE`, internal/external DTD subset, entity/notation/element/attribute declaration,
  conditional section, parameter entity, declared general entity, and external entity;
- every processing instruction other than the initial XML declaration. The XML declaration is not
  an event and is not treated as a processing instruction;
- XML 1.1, any version spelling other than exact `1.0`, every non-UTF-8 encoding declaration, a text
  declaration, and a misplaced or repeated XML declaration;
- invalid XML characters, names, references, comments containing `--` or ending with `-`, ordinary
  character data containing `]]>`, unquoted/duplicate attributes, mismatched tags, text outside the
  root other than XML `S`, a missing or second root, depth 257, and attribute 257 on one element.

The complete XML 1.0 Fifth Edition `Char`, `NameStartChar`, and `NameChar` ranges are accepted.
In particular, names are Unicode and case-sensitive; colon is an ordinary name character. The
Namespaces in XML QName and binding rules are not applied: `p:item` is returned exactly as
`"p:item"`, `xmlns` declarations are ordinary attributes, and undeclared, rebound, or multi-colon
prefix spellings are not rejected if the underlying XML `Name` is well formed. Duplicate attributes
are compared by the exact lexical XML `Name`, not by an expanded namespace pair.

Raw CRLF and raw CR in the document entity normalize to LF before token interpretation, including in
CDATA and attribute literals. A numeric reference such as `&#13;` contributes CR after that phase
and therefore remains CR. Attribute literal tab/LF becomes a space; numeric references to those
characters remain the referenced character. Only `&amp;`, `&lt;`, `&gt;`, `&apos;`, and `&quot;` are
named references. Reference expansion is one scalar at a time and never recursive, so neither XXE
nor exponential entity expansion has a representation in the accepted grammar.

Comments and the XML declaration are validated and skipped. They never produce a public event.
One ordinary `Text` is the maximal nonempty sequence of adjacent XML `CharData` and `Reference`
productions. A reference never splits that event: `<a>x&amp;y&#33;</a>` emits `Text("x&y!")`, and
`<a>&amp;&lt;</a>` emits one `Text("&<")`. A comment, CDATA section, child start/end tag, or the
enclosing end tag ends the ordinary run. Every nonempty CDATA section produces one separate
`Text`; empty ordinary runs and empty CDATA sections produce no event. Whitespace inside the root
is ordinary text and is preserved; whitespace outside the root is skipped.

## Event, cursor, and view rules

For this input:

```text
<a x="1">left<!-- gap --><![CDATA[<&]]><b/>right</a>
```

the exact event sequence is:

```text
Start(a), Text("left"), Text("<&"), Start(b), End(b), Text("right"), End(a), EOF
```

Attributes are not events. On `Start(a)`, `attribute_count()` is one,
`attribute_name(0)` is the view `"x"`, and `attribute_value(0)` is a fresh owned `"1"`. The
current event and its getter state remain unchanged until `next` is called again. An outstanding
`name` or `attribute_name` view prevents that mutable call through the ordinary region rule.

All programmer-state errors use one hard-abort path: a getter on the wrong event, an invalid
attribute index, use after move, or malformed private state does not masquerade as malformed input.
All document errors are discovered by `parse`; `next` never encounters a recoverable parse error.
This gives consumers one error boundary and prevents partially consumed invalid cloud responses.

## Parser, resource, and security contract

`parse` is an iterative complete-document validation pass. It uses fixed 256-entry stack storage for
open-element name spans and no recursive call proportional to input depth. Attribute uniqueness is
checked in source order with fixed 256-entry span/hash scratch and byte confirmation after any hash
match. Hash collisions cannot change acceptance. The limits are inclusive and deterministic:
depth 256 and 256 attributes succeed; attempting to admit the next start nesting or attribute returns
`Error.Invalid` before examining later document bytes.

On success the runtime allocates one fixed-size reader shell. The shell owns the original string
allocation, cursor and current-event offsets, one pending-empty-end record, and fixed current-start
attribute offsets. It stores no tree, event tape, copied name, normalized value, entity table,
namespace map, or source location. `next` scans each source byte a bounded number of times across a
complete traversal; it performs no allocation. `name`, `attribute_count`, and `attribute_name` are
O(1) from current offsets. `text` and `attribute_value` scan their selected source range twice
(validated output length, then exact fill), allocate once for a nonempty result, and never grow a
buffer incrementally. Total parse and one full event traversal are O(input bytes); resident memory
is the input allocation plus one constant-size shell.

No XML operation reads a URL, path, environment variable, catalog, locale, timezone, MIME header,
network response, or process-global parser setting. There is no callback or user code during
validation. Rejecting all DTD, entity, element, attribute-list, and notation declarations and every
processing instruction closes XXE, DTD retrieval, XInclude-by-PI, and billion-laughs-style entity
expansion by construction rather than through a configurable switch. The separately defined one
optional initial XML declaration remains accepted and skipped.

## Validation and error precedence

Source checking first proves exact import, receiver, argument/result types, consuming input mode,
mutable receiver for `next`, and current-view region constraints. Every raw ABI entry is `unsafe`.
The runtime may inspect pointer integers and lengths to mechanically reject null where forbidden,
misalignment, negative length, noncanonical empty, address-range overflow, and supplied-range alias
without dereferencing any pointer. A caller may therefore pass those exact malformed shapes and rely
on the specified rejection. Every nonnull pointer that passes the applicable shape checks and could
then be accessed must have provenance, lifetime, dereferenceability, and read/write accessibility for
its exact range. An output range is exclusive from access not represented by the call. A positive
parse input owns its exact allocator-compatible range; zero length is valid only with null and owns
no allocation. A reader pointer that passes shape checks addresses one live allocation intended as a
shell; getters and count have shared access, while `next` and nonnull `free` have exclusive access.
When the operation will follow the shell's stored input pointer, that live allocation and exact
readable range are also caller preconditions unless the shell was published unchanged by this
runtime. Violating a post-shape-check caller precondition has no behavior guarantee; an aligned,
nonnull integer address alone is not evidence that a pointer is valid.

Within those preconditions, the runtime performs every mechanically detectable range and alias check
with raw address arithmetic and scalar loads before it creates a Rust reference or slice. The parse
boundary uses this exact order:

1. Check the output header for nonnull, alignment, and representable fixed-size range. Check input
   length first; zero requires null, while positive length requires nonnull and a representable
   address range. Check the output header is disjoint from that range. These checks inspect address
   values only. A failure returns `AL_INVALID`, leaves output untouched, and accepts no input
   ownership; in particular, `(nonnull, 0)` is rejected and never freed.
2. After the post-shape-check pointer preconditions hold, store null to the output and accept either
   no allocation for canonical `{null, 0}` or the positive-length allocator-owned input.
3. Treat empty input as public `Error.Invalid`; otherwise validate optional BOM/declaration and then
   the document left-to-right. Every grammar, character, name, reference, uniqueness, nesting,
   depth, and attribute-bound failure returns the same public `Error.Invalid`, frees the input once,
   and leaves output null. Because there is no diagnostic or subcode, order among two XML-invalid
   bytes is not publicly distinguishable; tests pin the earliest-boundary work counters and absence
   of reads beyond it.
4. After complete validation only, allocate and initialize one shell, transfer input ownership into
   it, and publish its pointer. OOM aborts under the language policy.

An output-bearing getter first checks the output and reader pointer shapes and output/fixed-shell
disjointness using address values only. After the post-shape-check pointer preconditions hold, it uses
raw scalar loads to validate shell magic and fields without following the stored input, then checks
stored shell/input internal disjointness and output/input disjointness. Any failure in this complete
mechanical preflight leaves output untouched. Only then does it zero `{ptr, i64}`, validate current
state and index, and form input views. A state/index failure leaves canonical zero, for borrowed and
owned outputs alike. An owned getter validates the exact decoded length,
allocates and fills once, and publishes only the completed allocation. `next` requires exclusive
access to a genuine live shell and validates the shell before following its input. `free` accepts
null; a nonnull argument must be a genuine exclusively held shell. A detectable malformed genuine
shell aborts before following an invalid stored input range, then a valid Drop frees input and shell
exactly once. The runtime does not claim to authenticate an arbitrary dangling address.

Here structural shell-field validation means magic, a valid event discriminator, representable and
in-bounds offsets/counts, and consistent pending state. A structurally valid event that is not
admitted by the selected getter is the later current-state check after output zeroing.

## Compiler, interface, and runtime shape

The capability adds one Move type, one ordinary enum, and eight keyed runtime identities:

| Runtime key | Exact symbol | Existing ABI shape and declaration | Exact Rust ABI and status |
|---|---|---|---|
| `XmlParse` | `align_rt_xml_parse` | A08: `i32 @SYM(ptr, i64, ptr)` | `unsafe extern "C" fn(*mut u8, i64, *mut *mut XmlReader) -> i32`; `0` success, `-1` public invalid, positive `AL_INVALID` malformed ABI |
| `XmlNext` | `align_rt_xml_next` | A03: `i32 @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader) -> i32`; `0..=3` as the EOF/event table |
| `XmlName` | `align_rt_xml_name` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32`; zero only on success |
| `XmlAttributeCount` | `align_rt_xml_attribute_count` | A29: `i64 @SYM(ptr)` | `unsafe extern "C" fn(*const XmlReader) -> i64`; `0..=256` only |
| `XmlAttributeName` | `align_rt_xml_attribute_name` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32`; zero only on success |
| `XmlAttributeValue` | `align_rt_xml_attribute_value` | A20: `i32 @SYM(ptr, ptr, i64)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr, i64) -> i32`; zero only on success; out owns runtime allocation |
| `XmlText` | `align_rt_xml_text` | A19: `i32 @SYM(ptr, ptr)` | `unsafe extern "C" fn(*const XmlReader, *mut AlignStr) -> i32`; zero only on success; out owns runtime allocation |
| `XmlFree` | `align_rt_xml_free` | A62: `void @SYM(ptr)` | `unsafe extern "C" fn(*mut XmlReader)`; null-safe |

All Rust definitions use C calling convention and must not unwind across it. Their generated LLVM
declarations keep the existing shapes' empty curated function, return, memory, and parameter
attribute sets unchanged. A124 remains the
next unreserved shape. Implementation activation adds eight keys/symbols/exports/fingerprint rows
atomically; the design alone changes no active registry count.

`Ty::XmlReader=72` and `Scalar::XmlReader=48` are append-only canonical type-record-v3 leaves.
Their exact bytes are `[3, 0, 0, 0, 0, 72]` and, for
`Ty::Option(Scalar::XmlReader)`, `[3, 0, 0, 0, 0, 4, 48]`. Tags 73 and 49 remain unknown until a
later accepted capability. `xml.event` uses the existing nominal enum grammar and does not consume a
type-codec discriminator or change interface format version 8. Unknown, truncated, missing-payload,
and trailing records reject before cache publication.

## Implementation closure matrix

Implementation must close this matrix in one author-side pass before its one preflight review. One
parameterized owner may close many cells; a row does not require a new fixture when an existing
mutation-discriminating owner covers it.

| Axis | Required implementation closure | Exact regression owner |
|---|---|---|
| Type formation and import | Register `std.xml`, unique `xml.event`, `xml.reader`, exact method set, qualified-only spellings, consuming parse input, mutable `next`, borrowed getters, and their effects. Reject unimported, bare/wrong receiver, arity/type/mode, collision, comparison/print/collection/parallel/native forms before HIR. | Sema unit matrix plus driver import/interface tests and builtin nominal tripwire. |
| Parse construction and failure | Move the selected owned string path exactly once and null its source. Accept only canonical `{null,0}` with no allocation or a positive allocator-owned range. First mechanically reject output null/misalignment/overflow, negative length, noncanonical `(nonnull,0)`, positive-null/overflow, and output/input alias by address values alone; only a shape-passing pointer has provenance/access preconditions. Preflight failure leaves output untouched and accepts no ownership. Compiler-produced public invalid releases accepted input responsibility once, a no-op for empty, with null output. Allocate/publish one shell only after complete success and map exactly `-1`. | Direct local/field/tagged/function-result, block/`if`/`match`/`else`/`?`/`map_err`/loop/early-return matrix; invalid corpus; output null/misalignment/overflow × input negative/canonical-empty/noncanonical-empty/positive-null/positive-valid/overflow × alias owner; no-free for rejected `(nonnull,0)`, input/shell allocation, untouched/zeroed output, and publication counters. |
| Move-out, replacement, return, Drop | Sweep XmlReader through every canonical Move/cleanup/resource predicate, control join, storage generation, interface mode, and Drop-key map. Null-safe Drop; malformed nonnull state abort before input access; input then shell free exactly once. | Handle-type/Drop-key tripwires; move/use-after-move/replacement/return/control cleanup tests; malformed state and allocation balance. |
| Grammar and profile | Implement the W3C ranges/productions and the explicit BOM/declaration/comment/CDATA/reference/DOCTYPE/PI/DTD/namespace profile without lenient library defaults. Keep depth and attribute bounds exact. Attribute uniqueness uses hash candidate selection followed by exact byte confirmation. | Independent semantic corpus: W3C-style valid/invalid vectors, S3/Azure examples, every forbidden opener in prolog/content, Unicode range edges, name/reference/comment/CDATA/tag/attribute products, 255/256/257 bounds. A white-box unit feeds two different valid name spans the same stored candidate hash and accepts them, then rejects identical bytes; the test-only injected hash reaches only the internal confirmation helper and adds no runtime feature/export. |
| Event/cursor matrix | Emit explicit/synthesized starts/ends in exact document order. Coalesce maximal adjacent `CharData` plus single/consecutive `Reference` productions into one ordinary text event; split on comment, CDATA, child, and end-tag boundaries, with each nonempty CDATA separate. Skip declaration/comments/outside-root S; retain current state until next; EOF idempotent. `XmlNext` is one semantic MIR operation returning `Option<xml.event>`: it carries the canonical enum id, while LLVM lowering owns the exact `0=None`, `1=Start`, `2=End`, `3=Text`, other=abort mapping as one checked switch. Generic compare/branch/`MakeEnum` nodes must not carry that identity separately. | Exact `<a>x&amp;y&#33;</a>` and `<a>&amp;&lt;</a>` goldens plus nested/empty/comment/CDATA/child/whitespace documents, repeated getters and EOF, and one-field cursor-state mutations. MIR owners mutate the result type and enum id. The LLVM/codegen owner distinguishes all four valid statuses, all three event ordinals, and the invalid default-abort edge, and proves two valid events cannot be swapped. |
| Views, values, and regions | Preserve the exact reader/current-state region on name views; prevent move/Drop/next while live; clone escapes. Attribute/text outputs decode and normalize exactly, allocate once, retain no reader region, and survive next/Drop. | EscapeCheck direct/imported/function-value/control/carrier matrix; raw pointer identity for views; owned result identity/allocation/Drop; entity and line-normalization Cartesian matrix. |
| HIR replay and validation | Add every new expression/type to depth, clone/replay, effects, ownership, region, traversal, finalization, cache/semantic projection, checked-HIR validation, and malformed-input fail-closed switches. No wildcard silently classifies a new form. | Variant sweep tripwire; one-field child/type/result/effect/mode/region/current-state mutation; replay identity and source-shape tests. |
| MIR and runtime selection | Preserve typed operation/result/event/region facts, add all eight keys and only select reachable rows, reject wrong operation/result/enum/provenance before LLVM, and keep whole/per-unit parity. All seven callable XML operations are semantic MIR rvalues that return their final source result; no raw runtime status or native out slot escapes into generic MIR control flow. Before resource selection or LLVM construction, one sealed producer preflight builds a `ValidatedProducerGraph` from the exact MIR tables. Its exhaustive, no-wildcard `Stmt`/`Rvalue` classification validates each intrinsic result, operand, slot and selected path, constructor, control input, callable, auxiliary result, and runtime out-slot producer; adding a MIR variant fails compilation until that classification is updated. XML preflight consumes only graph-issued inseparable `(exact selected type, presence, access class)` facts, never access combined with an independently trusted side-table type. Function arguments begin as `Owned` for `ByValue`, `Shared` for `Borrow`, `Exclusive` for `BorrowMut`, and unreadable for `Out`. A fresh allocating or authenticated runtime out-slot producer begins `Owned`; views retain their validated source access; a direct, indirect, or imported call result begins `Owned` only when its validated callable identity, return provenance, and required dynamic-cleanup companion jointly prove transferred ownership. Callable identity is producer-derived through `FnAddr`, `Closure`, `Use`, store/load, structural projection, and control joins. Every indirect call must resolve the callee's exact `Ty::Fn(id)`, match all copied parameter modes and types, return, borrow, region, and cleanup fields to `Program::fn_types[id]`, and reject raw/non-callable, mixed, missing, or forged producers before its facts can seed ownership or form an LLVM call type. Every admitted edge validates its complete type relation before propagating access: `Use` requires operand and result equality; store/load requires the operand or result to equal the resolved selected slot-path type; field, tuple, variant, option, and result construction/projection derives the selected child type from the canonical aggregate definition and validates every constructor arity and operand even though unrelated access facts are not merged; a control selection requires an authenticated `bool` condition and equal alternative/result types but excludes the condition from provenance; and a call validates its canonical parameter modes, every argument, return type, selected return path, and cleanup companion. No cast, reinterpretation, `raw`, or otherwise unrelated producer can establish a `string`, `xml.reader`, or callable fact. Producer state distinguishes `Present(access)`, `MaybeAbsent(access)`, proven `Absent`, `Invalid`, and unresolved. A constructor of another option/result/enum alternative yields `Absent`; joining `Absent` with one access yields `MaybeAbsent` carrying that access, and an all-absent selected path remains valid. Whole-value call validation therefore accepts an absent self-owned leaf and requires every possible present case to be `Owned`, while mixed access, unknown tags, missing nodes, and unresolved state reject. Provenance is structural per selected aggregate path: field/tuple/variant/option/result projections follow only their selected component, while `if`/`match`/loop joins merge only alternative producers of that selected value and never their condition or unrelated siblings. Producer validity is a total fixed point over both selected dependencies and validation-only dependencies: every condition, fresh-producer input, call argument, and unrelated constructor operand must converge to a valid typed producer, but validation-only edges never contribute access. Seeded cycles converge with their seed; missing or duplicate definitions, unseeded cycles, ill-typed nodes, and unresolved check-only nodes become `Invalid` without recursive descent or panic. `SubSlice` and every other producer capable of yielding or feeding `str`, `string`, `xml.reader`, a callable, or a structural carrier is explicitly classified by the exhaustive producer authority. `XmlParse` requires present `Owned`, `XmlNext` requires present `Owned` or `Exclusive`, immutable getters accept present `Owned`, `Shared`, or `Exclusive`, and readable integer indices additionally admit an exact typed constant; every borrowed-place descriptor remains noncanonical. This rejects a wrong-mode or foreign-pointer argument even after laundering through a forged value, slot, wrapper, projection, or call boundary. The semantic shape `XmlParse { input, error_enum, cleanup }` makes its `Result<xml.reader,error>` primary value and a distinct boolean cleanup value one atomic MIR definition, like the existing cleanup-returning calls. Preflight requires the cleanup id to be in range, `bool`, distinct from the primary id, absent from every other definition, and defined exactly once by that `XmlParse`; lowering attaches it to the Result, while codegen alone maps `0` to `(Ok(reader), true)` and `-1` to `(Error.Invalid, false)`. `XmlNext` carries the canonical event enum id and returns `Option<xml.event>`; the other operations return their exact final `str`, `string`, or bounded `i64` result. | A compile-time variant sweep owns exhaustive producer classification. Parameterized mutations cross every `Stmt`/`Rvalue` producer class that can feed a checked edge, `ByValue`/`Borrow`/`BorrowMut`/`Out`, same-layout `raw` and other wrong-typed arguments, fresh/runtime-out/direct/indirect/imported results, wrappers/projections/control paths, typed and forged `Use`/store/load/field edges, joins, constants, and every borrowed-place family in whole/per-unit preflight. Direct and indirect whole-value call owners pass `None`, `Err`, and non-owning enum variants, while present owned leaves pass and present shared/mixed leaves reject. Callable owners launder raw values through every admitted fn-value producer and mutate the `Ty::Fn(id)` plus each copied mode/type/return/borrow/region/cleanup field independently in whole/per-unit builds. Projection owners distinguish an owned reader from unrelated shared tuple/enum siblings and a shared-origin condition, including local and cleanup-returning call results, while producer-laundered or unresolved sibling/condition nodes reject without merging their access. `SubSlice`/trim/template/runtime-out-slot/call sources feed `StrClone` and `xml.parse` in both compilation modes. A long acyclic chain, seeded cycle, unseeded selected cycle, unseeded check-only cycle, missing node, and duplicate node own worklist termination and fail-closed behavior. Missing, duplicate, aliased, out-of-range, or wrong-typed parse cleanup ids, result types, cleanup attachment, and error/event enum definition/id mutations reject before LLVM. LLVM/codegen structural owners exclusively prove both status-to-result/cleanup pairings and the default abort; runtime key inventory/bijection, unused import/no-selection, and per-operation exact selection remain exact. |
| LLVM/native ABI | Emit only the eight exact existing-shape declarations, private out-header handling, aggregate construction, owned-result cleanup, and reader destructor. Each semantic XML rvalue lowers its complete status contract atomically: parse maps only `0` to `Ok(reader)` with cleanup true and `-1` to `Error.Invalid` with cleanup false; next maps only `0..=3` to the fixed option/event table; name, attribute-name/value, and text accept only zero; attribute count accepts only `0..=256`; every other value aborts. No generic MIR comparison, branch, enum construction, result wrapper, or cleanup-bit producer participates in those mappings. Inspect address shapes without dereference, then apply provenance/access/exclusivity preconditions only to pointers that pass and would be accessed. For getters, complete pointer-shape/shell-field/internal-range/output-alias preflight failure is untouched; zeroing follows that preflight, and later state/index failures retain zero. No typed input access precedes all validation. No hand-written declaration bypass or shape attribute mutation. | Exact declaration/call IR goldens plus a codegen structural owner for every operation: parse distinguishes `0`, `-1`, and default abort including the exact Invalid ordinal and both cleanup polarities; next distinguishes `0..=3`, all event ordinals, and default abort; each zero-only getter distinguishes zero/default abort; count distinguishes both bounds and default abort. Native owners cover null/misalignment/noncanonical-empty/overflow/input-output-shell alias and malformed fields; rejected-pointer no-dereference/no-free; sentinel untouched versus canonical-zero output; key/symbol reverse lookup; base/export/collision parity; rt-LTO on/off. |
| Interface/cache/generics | Encode exact nominal event plus type leaves 72/48, parameter modes, mutability, effects, returned view regions, and cleanup; instantiate imported generic users without duplicate ownership. A return root is admitted from a by-value parameter only when its type itself may borrow; an opaque/self-owned Move value whose only capability is dynamic cleanup or non-cleanup ownership may root a returned view only through `borrow`/`borrow mut`. Apply that rule recursively to direct, concrete-sum, and generic carriers. Cleanup capability alone is never external borrow provenance. Include used capability/runtime implementation in object/link identity. | Bidirectional exact-byte and malformed codec goldens; direct and concrete/generic carrier summaries crossing every parameter mode, with forged by-value XML roots rejected and borrowed XML roots accepted; producer/consumer signatures; generic whole/per-unit view/Move parity; two-build determinism; surface/private edit-and-revert cache matrix. |
| Allocation and work bounds | Invalid positive-length parse: zero shell allocation and one input free. Canonical empty invalid: zero input allocation/free. Mechanical rejection: zero ownership/free. Valid parse: one shell, retained input, no event/tree/table allocation. `next`/view/count: zero allocation. Each nonempty owned getter: one exact allocation; empty attribute value: zero. Fixed 256 scratch and bounded per-byte passes. | Allocation/finder counters including canonical-empty, rejected noncanonical-empty, and positive-invalid cases; early-invalid read bounds, deep/attribute boundary RSS/stack owner, long document event traversal work counters, repeated getter allocation, no timing benchmark. |
| Diagnostics and documents | Diagnose exact qualified spellings, bound/mutable receiver requirements, consuming input, wrong state/index, and unsupported XML profile. Keep specification/design/history/roadmap/runtime/HIR ledgers and Japanese mirror synchronized. | Diagnostic assertions, syntax-check public example, doc/mirror/link consistency, and source-of-truth diff owner. |

The implementation is one capability boundary. The reader type, full-document validator, cursor,
views, Drop, HIR/MIR forms, and eight runtime rows form one strict producer-to-consumer chain; no
dormant subset leaves a useful stable consumer. It crosses more than three compiler layers and may
exceed roughly 1,000 hand-written lines. Splitting it would duplicate the Move/region/ABI proof or
publish an unusable handle, so the matrix is the lower-risk boundary.

### Design-review finding-to-fix ledger

The revised-diff reviews found that the first P1 correction still conflated pointer shape with
pointer safety and omitted canonical-empty ownership, then that the initial implementation split
three authenticated facts across permissive generic machinery. The post-redesign implementation
review then found that access and type were still authenticated separately and that aggregate
provenance was not structural. The next full implementation review found that presence, total
producer validity, callable identity, and the exhaustive producer inventory were still missing.
The matrix is therefore reopened on the **raw ABI representation phase**, **interface
provenance**, **typed MIR producer relation**, **structural presence**, **callable identity**,
**total validity**, and **runtime-status/result identity** axes. Shape-only rejection, post-shape
caller preconditions, and ownership acceptance are distinct phases; cleanup capability is not
borrow provenance; XML MIR authenticates one typed producer fact per selected structural path;
and each semantic XML rvalue owns one final source result while LLVM lowering owns its complete
checked status map.

| Finding | Class-wide resolution | Closure evidence |
|---|---|---|
| P1: raw ABI pointer provenance, dereferenceability, exclusivity, alias ordering, and failure-output ownership were incomplete and internally inconsistent. | The public ledger, validation order, ABI ledger, and closure matrix now separate exact-compatible unsafe caller preconditions from mechanically detectable rejection. Parse and getters perform raw range/alias preflight before mutation or typed access; preflight failure leaves output untouched, while every later getter failure leaves canonical zero. Parse ownership transfers only after preflight. | Parse/LLVM matrix rows require the full alias product, malformed shell fields, untouched-versus-zero output, and allocation/publication counters; the Japanese mirror and runtime ABI ledger carry the same rule. |
| P2: ordinary `Text` segmentation across XML `Reference` productions was unspecified. | An ordinary event is the maximal adjacent `CharData`/`Reference` sequence; single and consecutive references coalesce, while comments, CDATA, child tags, and enclosing end tags split it. | Exact mixed and consecutive-reference goldens are named in the event/cursor matrix and mirrored in Japanese. |
| Revised P1: zero-length/non-null input could reach ownership acceptance, while null/alignment were both caller preconditions and promised mechanical rejections. | Empty input is exactly `{null,0}` with no allocation. Address-shape checks may reject exact malformed shapes without dereference; provenance/access preconditions begin only after those checks. `(nonnull,0)` is rejected before output mutation or ownership and is never freed. | Reopened Parse/LLVM rows enumerate the address-shape Cartesian product, rejected-pointer no-dereference/no-free evidence, and canonical-empty public-invalid release. Runtime ABI and Japanese mirror use the same phase split. |
| P2: byte confirmation after an attribute-name hash collision lacked a discriminating owner. | The collision rule remains hash candidate selection followed by byte equality. A test-only stored-hash injection reaches only the internal confirmation helper. | A white-box owner forces equal hashes for distinct valid names and requires acceptance, then uses equal bytes and requires duplicate rejection. |
| P3: the text owner incorrectly grouped non-ASCII with NUL rejection. | The owner now independently requires non-ASCII UTF-8 acceptance and preservation and NUL rejection. | Public ledger and Japanese mirror name both polarities. |
| P3: security summaries said every declaration was rejected despite the optional XML declaration. | Rejection is limited to DTD/entity/element/attribute-list/notation declarations; one conforming initial XML declaration remains accepted and skipped. | English/Japanese security prose and roadmap use the qualified set. |
| P2: getter rows and detailed order disagreed on whether malformed shell/internal-range failure left output untouched or zero. | Every complete mechanical preflight failure—pointer shape, shell field, internal range, or output alias—leaves output untouched. Only after preflight does a getter zero output; later state/index failure retains zero. This prevents an aliased output from corrupting shell/input before rejection. | Every public getter row, detailed validation order, runtime ABI ledger, Japanese mirror, and sentinel-output owner use the same two postconditions. |
| Implementation re-review P2: interface root validation treated dynamic cleanup as external borrow provenance and admitted forged roots from by-value `xml.reader` parameters. | Parameter mode and type capability are checked together. By-value roots require an intrinsically borrow-bearing type; opaque/self-owned Move leaves and their concrete/generic carriers require `borrow` or `borrow mut`, regardless of cleanup shape. | Direct and nested carrier summaries cross ByValue/Borrow/BorrowMut; malformed by-value XML roots reject while compiler-produced borrowed-root summaries round trip. |
| Implementation re-review P2: XML MIR operand checks trusted the declared type inside forged borrowed-place descriptors. | XML preflight rejects every borrowed-place family, but shape filtering is not the authority. A producer-derived lattice follows argument mode through canonical stores, loads, moves, and joins: parse requires owned consumable input, next requires owned or exclusive access, getters permit shared access, and `Out`, mixed, or unknown origins reject. | Every operation crosses all argument modes and raw/load/store/join forms in whole/per-unit validation; same-typed values derived from a forbidden origin reject as surely as a raw forged argument. |
| Implementation re-review P2: `XmlNext` status identity was split from generic comparisons, branches, and enum construction, allowing valid-typed Start/End swaps. | All seven callable XML operations become semantic MIR rvalues returning final source results. Parse, next, every getter, and count keep raw statuses and native out slots inside their LLVM lowering; generic MIR nodes cannot rewrite any status/result/abort graph. | MIR mutations own operands, result types, and error/event enum ids. Exact LLVM/codegen structural owners distinguish every documented valid status/result ordinal and every invalid default-abort edge; whole/per-unit output remains identical. |
| Redesign review P1: admitting a typed `Arg` still ignored parameter mode, and a forbidden borrowed origin could be laundered through a same-typed `Value`. | Canonical XML access is producer-derived. Initial argument modes define ownership/access, every admitted producer propagates it, and unknown or mixed joins reject. Operation-specific requirements distinguish consuming parse, mutating next, and immutable getters. | The provenance mutation matrix crosses `ByValue`/`Borrow`/`BorrowMut`/`Out` before and after stores, loads, and joins for every operation in both compilation modes. |
| Redesign review P2: atomizing only `XmlNext` left parse and getter/count status graphs editable through generic MIR. | The semantic boundary now covers every callable XML rvalue and returns only the final source-language result. LLVM lowering privately owns each exact runtime-status table and default abort. | Per-operation MIR graph assertions forbid generic status consumers; exact emitted-IR owners cover parse, next, zero-only getters, and bounded count. |
| Redesign review P2: the proposed MIR owner tried to mutate status constants and abort edges that no longer existed in MIR. | Evidence is split at the owning layer: MIR validation mutates operands, provenance, result types, and enum ids; LLVM/codegen inspection owns runtime constants, ordinals, and invalid edges. | The codegen owner independently distinguishes all valid cases and the default abort for every XML status contract. |
| Focused redesign re-review P1: the final parse Result and its dynamic cleanup bit were not one authenticated semantic result. | `XmlParse { input, error_enum, cleanup }` atomically defines the primary Result and one distinct bool cleanup id. Preflight proves range, type, uniqueness, single definition, and attachment; codegen alone maps `0` to `(Ok(reader), true)`, `-1` to `(Error.Invalid, false)`, and every other status to abort. | Whole/per-unit MIR owners mutate cleanup-id range, bool type, distinctness, single definition, and Result attachment only; exact emitted IR exclusively proves both status pairs and the default edge. |
| Focused redesign re-review P2: fresh and callable results had no provenance seeds, contradicting admitted function-result paths. | Fresh allocation is owned. Direct, indirect, and imported results derive access from validated signature, return provenance, and cleanup-transfer facts; wrapper/projection and control producers propagate that class without treating cleanup capability as borrow provenance. | Direct/imported/function-value and wrapper/control cases cross every access class, with unknown and mixed roots rejected. |
| Post-redesign implementation review P1: access propagation trusted forged value/slot type tables, so `raw` could pass through `Use` or store/load as `xml.reader`. | Access is never a standalone fact. Each admitted graph edge derives and validates the exact source/result type together with access before propagation; a mismatch terminates preflight before LLVM or a native call. | Whole/per-unit mutations launder same-layout `raw` and wrong aggregate components through every admitted value, store, load, field, wrapper, projection, and call edge and require `CodegenError`. |
| Post-redesign implementation review P2: whole-aggregate taint merged unrelated siblings and `Select` conditions, rejecting valid reader projections. | Provenance is recorded per selected structural path. Tuple/enum/option/result/field projection follows only the selected component; a control condition is type-checked but contributes no result provenance; only alternative producers of the same selected value are joined. | Positive tuple and multi-payload-sum carriers combine an owned reader with an unrelated shared sibling, and a shared-origin condition selects owned readers. The selected reader remains owned, while an owned/shared alternative join rejects as mixed. |
| Typed-provenance re-review P1: whole-value call validation treated inactive Option/Result/enum owned leaves as unresolved, rejecting valid non-owning alternatives. | Presence is distinct from validity and access. Proven inactive paths are valid `Absent`; a present/absent join retains a `MaybeAbsent(access)` obligation, and only possible present cases must satisfy the call mode. | Direct/indirect and whole/per-unit call owners pass `None`, `Err`, and non-owning enum variants while still rejecting a present shared or mixed self-owned leaf. |
| Typed-provenance re-review P1: indirect calls trusted copied signature fields and an arbitrary declared `Ty::Fn`, so forged callees or ABI facts could seed ownership and form the wrong LLVM call type. | The validated producer graph derives exact callable identity through every fn-value producer. An indirect call is admitted only when the producer resolves to one canonical `Ty::Fn(id)` and every copied field equals `Program::fn_types[id]`. | Whole/per-unit mutations cover raw/non-callable callee producers, fn-value stores/projections/joins, the type id, and each parameter mode/type, return, borrow, region, and cleanup field. |
| Typed-provenance re-review P1: validation-only dependencies propagated explicit invalidity but not unresolved nodes, allowing unseeded condition/input cycles and laundered unrelated siblings. | One total validity fixed point covers selected and validation-only edges. Unresolved is invalid after convergence; conditions, fresh inputs, call arguments, and every constructor operand require a valid typed producer, while their access remains excluded from a selected-path merge. | Unseeded check-only cycles and raw-laundered sibling/condition owners reject; corresponding valid shared siblings and conditions remain accepted. |
| Typed-provenance re-review P1: producer coverage omitted `SubSlice`, rejecting a valid substring clone passed to `xml.parse`. | One exhaustive no-wildcard producer authority classifies every MIR variant and supplies its exact result/dependency relation or an explicit irrelevant class. New variants cannot compile until classified. | A compile-time sweep plus whole/per-unit substring, trim, template, runtime-out-slot, and call-result XML inputs close both coverage and valid-source acceptance. |
| Typed-provenance re-review P2: the documented HTTP integration cloned `slice<u8>` directly even though only `str`/`string` supports that copy. | HTTP body integration first calls `as_str()?` for explicit UTF-8 validation, then clones the validated `str` into the owned parse input. | The English/Japanese example is identical and a syntax owner checks the complete `http.parse` → `body` → `as_str()?` → `clone` → `xml.parse` sequence. |

## Deferred surface

The first capability deliberately omits streaming from `io.reader`, incremental/fallible `next`,
caller-buffer text decode, raw spans/source locations, subtree skipping, DOM/tree construction,
XPath/CSS selection, namespace expansion, namespace validation, DTD/schema validation, all custom or
external entities, XInclude, processing-instruction delivery, comment events, canonicalization,
mutation, writing, and serialization. A concrete consumer and a new ledger are required before any
of these become work. S3 and Azure Blob need only the accepted forward event stream; their HTTP
status/body bounds and service-specific field semantics remain package-owned.
