# Mixed borrowed and owned JSON graphs (media AM-R02)

## Evidence and capability boundary

Media receipts combine borrowed prompt/version text with owned model definitions
and numeric arrays. The unchanged AM-R02 witness rejects the owned-graph `str`
gate. Voice metadata and LLM completion/persisted artifacts also use this shared
boundary; their fully owned decode results must remain independent. All three
registers and these uses were inspected. Consumer implementation, compiler pins
and adoption checks remain external.

Complete one existing type-directed JSON graph by admitting borrowed text leaves
beside owners. This supersedes the mixed-view exclusions of Plans25/47/188, not
their other scalar, layout, depth, error, source-borrow or cleanup rules. No second
encoder, implicit clone, dynamic JSON type or ownership model is introduced.
One complete schema/lifetime/cleanup/transport capability is expected to exceed
1,000 handwritten changed lines including owners and mirrored contracts. An
independent schema producer would have no useful consumer, while independent
admission would bypass its lifetime/identity proof; keeping them together avoids
duplicate proof and unsafe intermediate states.

## Public-contract ledger

| Surface | Exact rule and owner |
| --- | --- |
| API and roots | Existing `core.json`: `json.encode(value: T) -> Result<string, Error>`, `json.encode_bounded(value: T, max_bytes: i64) -> Result<string, Error>`, `json.decode(input: str) -> Result<T, Error>`. T is the existing direct natural-layout record or dynamic AoS record-array root selected by a transitive owned string. One shared graph admits `str` in every existing text-leaf position: direct field, Option payload, array element, and recursively below records/options/arrays. No defaults, flags, ambient configuration or new syntax. No-owned-string routes, scalar roots, SoA, scanner, fixed arrays, tuples, sums, nested Option and array-of-array/Option admission stay unchanged. Sema/HIR own formation. |
| Encoding | Stable named root is shared-borrowed, never consumed or mutated. Every transitive borrowed text source stays live/read-only through the call, including evaluation of max_bytes. Both encoders use the same declaration-order writer and produce byte-identical output on bounded success. Success owns one independent string, retains no input/arena and requires no final copy. Existing nonfinite/negative/exceeded-cap Error.Invalid and terminal allocation/size policy remain. Existing output String/Result and source-reservation owners apply. |
| Decoded text | `string` always owns independently allocated bytes. Clean `str` borrows the input; a selected escaped `str` uses the explicit enclosing arena. Without an arena that selected escape returns existing Error.Code(1). Empty/static or optional-absent values do not weaken the type-directed lifetime rule. No hidden arena/owner and no implicit conversion between str/string. Runtime kind3/kind8 remain distinct. |
| Decoded ownership | Every string and dynamic array spine is individually allocated even inside an arena; only escaped borrowed text uses the arena. `array<str>` owns its spine, not its text; arrays of mixed records recursively drop only owners. Complete mixed results conservatively retain both input and enclosing-arena lifetimes. Fully owned results retain neither. Input replacement/Drop and arena expiry invalidate mixed results and views; a mixed result cannot escape either. No new selected-field lifetime precision is promised. Sema content facts, regions, storage generations and native allocation mode must agree. |
| Error and cleanup | Arity/expected root/full graph admission precede input typing; bounded encode validates source/schema before cap typing, then complete eager borrow checks before publication. Runtime preserves existing UTF-8, string grammar, object/array grammar and selected-field validation order. Malformed/duplicate/missing/wrong-kind/range/selected-escape-without-arena/trailing decode errors return Code(1), publishing no partial result. Complete initialized owners free once in declaration/element order, then spines; str never frees its input/arena. Failed arena text remains arena-owned until arena exit. Plan187 full array grammar-before-owner-allocation remains. |
| Control and transport | Existing Move/Drop/Result behavior covers construction, assignment, field extraction, return, `if`, `match`, `else`, `?`, `map_err`, loops and early exits. Borrowed inputs remain reserved during encoding. Monomorphization and whole/per-unit/cache replay preserve exact graph, source dependencies and individual ownership. No change to native function signatures or runtime keys. |
| Compiler identity | One V4 graph replaces V3 for all selected roots, with the exact format delta below; interface format18 becomes19. Complete reachable structural identity includes Str versus String even though their machine headers match. Exported root association remains nominal. Generic/private graphs reconstruct in the consumer and enter MIR/compiler/runtime cache identity; no legacy reader, fallback or alias. |
| Scope and performance | No performance improvement claim, benchmark threshold, source reflection, process-global mutable state, new allocator option or backend-only semantics. Existing per-call allocation and concurrent independent use remain. No consumer adoption or release-version publication. |

## V4 descriptor and interface contract

Plan47 section4 remains authoritative for every unchanged byte, scalar width,
node, ordering, target binding, hash algorithm and validation phase. Apply exactly
these changes atomically: descriptor version3 ->4; envelope version3 ->4; interface
FORMAT_VERSION18 ->19; add the following leaf in Value/Payload/Element positions:

```text
0x11 Str: empty payload; size=16, align=8; owns/drop=0/0
```

String remains tag0x10 with owns/drop1/1. Str is Copy, does not increment constructor
depth and creates no Drop obligation. Option<str> size24/alignment8/owns0/drop0;
array<str> size16/alignment8/owns1/drop4 with element-drop-version1 and a Str node.
Record ownership remains derived from its actual reachable owner nodes. The
unchanged ABI cells describe the common 16/8 text header, including borrowed Str;
there is no new target ABI. Envelope hash includes the changed version byte.
All roots, including existing fully owned schemas, use V4. The root-list,
root-first DFS, stable natural layout, DAG/cycle/depth128 and name rules remain.
Unknown tags, stale versions, forged owns/drop, invalid size/alignment/offset,
truncation, extra bytes, unreachable/omitted graph roots and target mismatch reject
before any cached graph or object is trusted. No mixed schema silently falls
back to the old nonowned route.

Independent semantic-to-byte and byte-to-semantic vectors must cover:
`Mixed { label: str, owned: string, maybe: Option<str>, tags: array<str> }`,
with fields in that declaration order. Natural offsets are0,16,32,56; record
size72/alignment8/owns1/drop2. Include a nested mixed record graph, owned-only
controls, all supported target envelope prefixes/hashes and mutation controls.
Goldens are hand-encoded from this record, never generated by the production
serializer under test. Existing V3 vectors become V4 vectors with the specified
version/hash delta, retaining all former scalar/graph coverage.

### Independent V4 vectors

The Mixed record descriptor is 151 bytes:

```text
04000101000000000000004800000008000000010204000000050000006c6162
656c111000000008000000000000000000050000006f776e6564101000000008
000000010110000000050000006d617962652118000000080000000000000000
0008000000111000000008000000000020000000040000007461677322100000
0008000000010401111000000008000000000038000000
```

Target envelope prefixes and the following 16 hash bytes (little-endian lanes):

- `x86_64-pc-linux-gnu`: prefix `04130000007838365f36342d70632d6c696e75782d676e7500000808010110081008010104040808`; hash `830252f0d17ffd3bdde2a1eff2f9e182`.
- `aarch64-unknown-linux-gnu`: prefix `0419000000616172636836342d756e6b6e6f776e2d6c696e75782d676e7500000808010110081008010104040808`; hash `2e33cf129a32f212aae5af2b4fecec4f`.
- `arm64-apple-darwin`: prefix `041200000061726d36342d6170706c652d64617277696e01000808010110081008010104040808`; hash `c37ccb8a4a1b8d5b597d67f94982ae3c`.

Append u32 descriptor length and those descriptor bytes. The independent integer-arithmetic hash oracle also reproduces the existing V3 x86 envelope golden before deriving V4. The mixed field offsets and ownership bytes are encoded directly from the ledger, without the compiler serializer.

## Implementation closure matrix

| Axis | Implementation seam and discriminating owner |
| --- | --- |
| Formation and direction | Shared sema graph allows Str at direct/Option/dynamic-array leaves, independent interface parser/layout/Drop agrees, HIR reconstruction equals canonical graph. Parameterize encode/bounded/decode over these leaves, nested records/options/record arrays, both roots, no-owned-text/fully-owned controls and unchanged unsupported shapes/depth/layout. Update obsolete mixed rejection owners, never remove their unrelated negatives. |
| Input/arena lifetime | JsonOwnedDecode derives borrowed input and explicit lexical arena only when graph contains str. Region, BorrowRootAnalyzer, escape-value/content facts, view-bearing element facts and checked-HIR replay agree. Check-only owners reject local input escape/replacement, named/unnamed arena escape, direct/optional/array/nested text and helper/import returns. Positive controls keep fully owned outputs independent and mixed results usable while both sources live. |
| Allocation authority | Native individual allocation remains distinct from content region: string and array spines free individually in and outside arenas. Fresh generations and release places use individual ownership for owners, shared authority for decoded text. The MIR output producer certifies selected owning leaves without promoting input/arena text to exclusive storage. Native counters and source lifetime/replacement owners exercise both roots and all mixed leaf positions. |
| Construction/transfer/cleanup | Runtime existing kind3 borrowed versus kind8 owned, kind7 scalar/text arrays and recursive tables; canonical Drop frees owners only. Success/failure/error-after-owned-prefix, optional absence, empty values, duplicate/late range/trailing input and speculative/fallback paths require complete allocation/free parity. Source owner covers direct whole/field moves, replacement, discarded result, argument/constructor later failure, if/match/else/?/map_err and loop/early return. Reuse current owned-only cleanup controls. |
| Eager encoding borrow | Both encoders borrow the complete mixed source and transitive text through limit evaluation; source/cap effects retain source order and divergence. A parameterized negative owner covers consumed/replaced direct text, Option text, array text and nested record backing during cap evaluation. Success reads source afterward; output survives input/arena/record Drop. Both encoders compare exact bytes for clean/escaped/NUL/unicode/missing/null plus error caps/nonfinite controls. |
| IR and malformed input | Add optional arena operand to MIR JsonOwnedDecode; lower the current explicit arena for view-bearing graphs, None for fully owned graphs. Update every operand walk, printer/fingerprint, producer and LLVM validator; backend uses only the checked operand. Reject incorrect graph/root/result/Error/input/output slot, invalid arena type, nonexistent/expired arena and forged ownership facts at every public checked entry, including unused/partition peers. Do not reconstruct language semantics in LLVM. |
| Interface and cache | Rename V3 graph/record/field types to V4, interface19, Str exact serialization/parser/layout/ownership, target envelope and canonical-root count. Independent byte vectors and exhaustive scalar-tag mutation owner; whole/per-unit imports and concrete generic schemas plus cold/hit/edit/restore demonstrate no stale Str/String cache reuse. |
| Owners and source agreement | Extend m5_owned_json, m5_json_bounded, interface owned_json, checked-HIR and MIR producer owners where they discriminate this change. Add one bounded mixed_json driver owner for integrated lifetime/control/allocation/cache cases and native mixed descriptor cleanup owner if existing native tests do not close it. Keep Plan187 grammar and Plan188 array-root owners. Update draft, condensed spec, design-notes, Settled, core-design/json EN/ja, Plans25/47/188 current-contract pointers, ledger19 and HANDOFF at capability completion. ABI inventory changes only for changed operand/provenance promises, not row-count narration. |

## Author and review checkpoint

Before implementation, extract this ledger's every/exact/before/reject/required
obligations and map them to the matrix and inspected seams. The complete encoding,
decoding, ownership and identity boundary gets one fresh independent adversarial
strategy review. Before publication, map every applicable cell to implementation
and a passing owner, run align-self-review and one fresh inspection-only full-diff
review, fix valid findings as a class, and run final-SHA preflight before pushing.
