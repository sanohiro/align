# Owned JSON record-array root decoding (Request133)

## Evidence and capability boundary

The real consumer uses `Case { id: string, prompt: string, text: string,
ids: array<i64> }` and wants `cases: array<Case> := json.decode(document)?`.
The provider reproduction rejects the existing owned-text element at schema
admission. Encoding this array and decoding the same record as an embedded array
already work. Root position alone supplies no ownership or language rationale
for this restriction. Retain the existing inferred operation and closed V3 graph.
Plan187 independently repairs the indexed native array grammar before this
admission uses that path. Consumer code, pins and application verification remain
external; no wrapper or implicit clone is introduced.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| API and inference | Existing `core.json` import and `json.decode(input: str) -> Result<array<R>, Error>` chosen by expected result type. One positional argument; owned string input is shared-borrowed for the call. No new syntax, defaults, explicit type arguments, options, environment, global state or allocator selection. Purity stays the existing operation rule apart from argument effects. |
| Admitted record | The same existing nonempty natural-layout acyclic V3 owned graph selected by a transitive owned string leaf. Its full recursively accepted scalar/float/bool/string/record/Option/dynamic-array grammar and depth/layout restrictions are unchanged. No root-only extra element rule. View-bearing/mixed graphs, unsupported options/arrays, resources and other excluded field types remain rejected. No-owned-text record arrays retain the existing descriptor route. SoA and scanner admission do not widen. |
| Input and observable result | Complete length-delimited UTF-8 JSON array of objects, optional JSON whitespace and exact EOF. Preserve row order and all decoded field values. Empty input array yields null/zero owned array without Align allocation. Empty strings and empty nested arrays retain canonical zero payloads. Existing duplicate declared-field rejection, required fields, optional missing/null, numeric widths/ranges and unknown-field projection apply to each row. |
| Error order | Arity and expected target/record graph admission precede input typing and checked HIR publication. Input expression executes once; divergence prevents native decode. Native UTF-8/full array grammar and member shape precede decoded allocations, then selected rows/fields use existing writers, then complete output is published. All recoverable failures are existing Error.Code(1); no partial array escapes. Size/allocation failure retains existing runtime policy, with no newly recoverable allocator model. |
| Ownership and lifetime | Success independently owns the contiguous array spine and every reachable string/array allocation. Result is Static even inside an arena and retains no source/arena lifetime. Input is unchanged and can expire or be replaced after decode. Indexing a Move record uses the existing shared borrowed-index access; no owned field extraction widening. Canonical transfer/source nulling/replacement/return/element-then-spine Drop rules apply. |
| Allocation and cleanup | Reuse A06 indexed AoS staging, V3 descriptors and deep partial-row cleanup. Per-call Rust staging/index/syntax scratch remains transient; success allocates one exact-size Align spine plus existing field-owner allocations. The current partial row and completed rows are dropped once on valid-syntax schema failure or failed speculation before fallback. Full grammar errors allocate no decoded owners. No retained schema metadata, hidden clone, implicit arena or new arena-backed owner. |
| Compiler and native ownership | Sema selects the same owned graph; checked HIR reconstructs it and authenticates input/result/root kind. MIR carries owned provenance and exact output slot; LLVM purely selects existing record versus array native lowering and emits the same canonical descriptor table. Runtime parses and cleans; core.json is the source namespace. No package semantics. |
| IR/ABI/cache | Add explicit Record/Array root kind to the existing HIR/MIR JsonOwnedDecode operation; retain one graph and one operation. V3 bytes remain rooted at the element record, identical to the embedded record/array and encoder grammar. Existing interface graph transport and typed function result authenticate the container. No runtime export, key, ABI layout/signature or serialized V3/interface format bump. Compiler build identity invalidates artifacts and replay cloning preserves root kind plus remapped graph. |
| Acceptance and documentation | Native full V3 array owner, malformed HIR/MIR root/result/slot/graph/input mutations, whole/per-unit/imported/cache lifecycle source owner and existing borrowed/scalar/owned-record controls. No performance improvement claim or new parser algorithm: Plan187 owns the changed parser cost. Update draft.md, language-spec, design-notes, Settled open-questions, core-design/json and ja mirror, plans24/25/47/59/183 references where root-decode limits change, HIR ledger19 and one HANDOFF capability boundary. ABI ledger20 changes only if a normative surface actually changes. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Type formation/check order | Use owned_json_route in the existing DynStructArray branch; errors stop before node publication. Existing unsupported element and mixed-view diagnostics remain. Source negative matrix crosses wrong arity/input, malformed existing operand, invalid layout/graph and both no-owned-text and owned routes. |
| Checked producers | JsonOwnedDecode root kind determines exact Result<Struct(root),Error> or Result<DynStructArray(root),Error>, never inferred from an unauthenticated result. Both envelope and derived-type checks authenticate canonical V3 graph. MIR output-slot producer authenticates exact root kind/type/graph, Str input and Owned provenance. Parameterized existing/new mutation owners reject toggled kind, wrong root/result/Error/slot, forged graph and invalid input before LLVM in every validation entry point. |
| Construction/move/Drop | Existing array/record layouts and deep Drop; native output retains individual heap-allocation provenance even inside arenas, and collection backing-storage analysis maps borrowed element views to this array owner. Static result provenance alone is insufficient. The source owner proves independent input/named-arena expiry, returned direct arrays and Results, generic forwarding, discard, replacement, and exact positive allocation/free/live-byte parity. Borrowed element views cannot escape the array or survive replacement. |
| Control paths | Source lifecycle matrix covers if/match/else/?/map_err, branch/loop joins, early Err and terminating input. No new CFG semantics. Compare success/failure cleanup in whole/per-unit modes. |
| Native complete prefix | Direct A06 descriptors generated from complete accepted V3 field classes exercise empty/multiple rows, UTF-8/escapes, nested records/options/arrays, numeric ranges, optional missing/null, speculation/fallback and late schema failures. Exact decoded values, unchanged failed output, positive partial allocation and zero retained owners are required. Plan187 already owns malformed separators, ignored grammar and array/member shape; reuse that owner. |
| Generic/interface/replay | Existing graph remapping and function container typing; explicit root-kind cloning. Imported concrete/generic producer executes under whole/per-unit and cold/hit frontend cache replay. Bind actual compiler/runtime artifacts; no consumer source input to tests. |
| Shared consumers and exclusions | Inspect llm133, voiceV002 and mediaAM02 usage. This closes only owned-array JSON root admission, not field extraction or mixed-view schema admission. Existing owned record and scalar-array roots, borrowed record arrays and SoA controls must pass. |
| Final proof | Author ledger-to-diff and align-self-review pass; one fresh independent strategy review before implementation, one fresh full-diff code review and local owner/gate/Clippy on committed candidate before push. Exact final preflight stamp, PR CI and merge. No forced release version. |

## Author consistency pass

Every new root promise is mapped to the existing graph and canonical ownership
model, exact root-kind derivation, native prefix cleanup, compilation/replay and
negative producer authority. The record/array distinction has one explicit IR
field; backend type heuristics cannot choose semantics. No new serialized graph
or public container is needed. This is one useful capability rather than dormant
IR/compiler seams. Expected source and owner changes should remain near 1,000
hand-written lines; if larger, retain this unified boundary because independently
landing root admission without validation/ownership/cleanup would be unsafe and
repeat the same proof. Record the actual scope before coding if that threshold
is expected to be exceeded.

The fresh independent strategy review found no material issue. It confirmed
V3/A06 descriptor coverage, rollback and output publication, explicit root-kind
authentication and unchanged transport identity. Its implementation checkpoint
requires individual native heap provenance inside arenas and complete collection
backing-storage analysis; the construction/lifetime row names both explicitly.
