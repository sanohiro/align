# Indexed JSON record-array grammar closure

## Evidence and boundary

While qualifying llm Request133, a provider-only source witness at e53a4de9
showed that array<Row> with Row { id: i64 } accepts doubled/missing/trailing
commas, a scalar array member, and an undeclared bogus token. The existing
lean index records braces/brackets/colons, not the full grammar. AoS and SoA
share indexed field writers. The result violates the existing malformed-input
contract; it is not a language restriction or consumer defect.

Repair this already-shipped native parser failure domain before widening R133's
compiler/schema admission. The repaired decoder is independently useful to all
current array/SoA consumers. R133's V3 graph/typed-root/ownership admission can
then reuse the exact native grammar owner. No new public API, schema family,
ABI, IR variant, error model or arbitrary nesting limit is introduced here.

## Contract and implementation closure matrix

| Axis | Strategy and acceptance |
| --- | --- |
| Existing inputs/results | Length-delimited UTF-8 input; one complete JSON array containing only objects; optional JSON whitespace; empty array accepted. Both existing AoS/SoA targets retain their types/layout/regions and exact field schema. Invalid complete syntax or wrong root/member shape yields existing native status1 / Error.Code(1), without output publication. No new knobs/defaults/ambient state. |
| Complete grammar | Replace the existing whole-input string-token validation pass in the two indexed array entrypoints with one shared iterative syntax-and-shape walker. Use existing JsonParser string and number lexical primitives; validate object keys/colon, array/object value positions, comma placement, matching delimiters, literals and token boundaries, including undeclared nested values. Do not convert skipped numeric values, unescape/copy strings, construct a JSON tree or recurse on the native stack. A private Vec of small parser states is O(nesting depth); allocation failure follows existing terminal parser scratch policy. Do not impose a new depth limit on previously indexed input. |
| State/row counts | The walker starts with an outer array and requires each direct member to be an object. Exhaustive state enum distinguishes empty-container allowance from required values/keys after commas, and after-value/colon positions. It returns a checked usize direct-object count only after EOF; SoA reuses it instead of the old bracket-count-only pass. AoS's filled row count must agree before publication. Empty/null result behavior remains. |
| Selected token exactness | write_field_indexed must consume the complete selected value before the surrounding comma/closing-object delimiter; int targets cannot accept the prefix of 1.5 or 1e2. The optional-null arm obeys the same boundary check. Existing target-width numeric conversion, Option missing/null, declared-key duplicate rejection and unknown-key projection remain authoritative. Strict object/field decoders already own their separators. |
| Ownership/publication | Full grammar validation replaces the prior lexical prepass and completes before indexed staging, column/escaped-byte allocation or output writes. Thus a syntactically invalid later row creates no decoded owners. Valid JSON with a later schema/type/range/duplicate/missing-field failure still exercises completed/current-row deep cleanup. Speculation still cleans partially written owners before fallback; existing exact-once cleanup and generated Drop remain. No output/source alias change or retained syntax metadata. |
| Call and control boundaries | Native calls are synchronous with per-call private parser/scratch; input remains live and immutable; no process/connection-global state, overlapping retained operations or new failure precedence between observable error variants. Existing MIR Result formation, ?, else, match, map_err, branch/loop/early exits, source nulling, replacement and return are unchanged. No changes to generic/interface/cache schemas or native ABI; compiler/runtime build identity invalidates old executable artifacts. |
| Native owner | One parameterized owner crosses AoS/SoA, first/second learned-pattern/fallback rows, empty/many, whitespace, full/projected fields, nested unknown values, escaped keys/strings, duplicate declared vs unknown keys, mismatched/open delimiters, missing/doubled/trailing commas/colons, scalar root members, token junk/prefixes, leading/trailing text, numeric wrong-kind/range and nonfinite lexical magnitudes. Assert exact successful fields/row counts and unchanged error-output sentinel. Deep valid/malformed unknown nesting establishes stack safety without a new depth cutoff. Native allocation owner proves no decoded allocation for invalid grammar and zero retained owners after valid-syntax schema failure. Existing failure-count tests adjust only cases now rejected before staging; preserve positive prefix-allocation controls. |
| Source owner | New exclusive owned_fixture/direct-driver owner checks exact AoS and SoA observations under whole/per-unit compilation, ordinary Result recovery and source/arena lifetimes. Existing typed JSON/SoA/numeric and decoded-owner speculation/cleanup tests qualify unchanged semantic/storage behavior. No consumer fixture or pin changes. |
| Author and independent checks | Before implementation inspect this strategy independently. Before PR run the owner tests plus bounded gate and Clippy on final commit with one fresh full-diff inspection. Audit sibling indexed paths for the same incomplete-grammar/value-prefix class; scanner/framing and scalar/string roots retain their distinct existing parser owners. |
| Performance | Run the repository's json_decode and json_soa before/after local measurements because their existing benchmark contract requires it for parser changes. Retain full/projected same-input results, producer hashes and raw samples; correctness is mandatory, timings are a cost observation, no fixed speedup promise. Reuse compatible caches; clean sealed binaries afterward while retaining compact evidence. |
| Documentation | This owning plan records implementation closure and measurement costs. Existing public malformed-input promises do not change; amend other normative prose only if the actual chosen strategy changes a promise. Record one capability boundary in HANDOFF after implementation. R133 stays proposed until separate provider delivery. |

## Author consistency pass

All changed native inputs, state alternatives, selected-token boundaries,
publication points, allocation/cleanup paths and shared indexed consumers are
covered above. The syntax walk replaces an existing full-input lexical pass;
it does not add numeric conversion for projected fields or a hidden schema.
The count belongs to the grammar producer, not an independent bracket guess.
The full owner matrix must distinguish valid syntax with a schema failure from
invalid syntax before staging. This change is expected to stay below 1,000
hand-written lines; any larger boundary needs an updated explanation.

The fresh independent strategy review was CLEAN before implementation. It
checked both reproduced array layouts, selected-value boundaries, lexical
projection, staged owner cleanup and the iterative-state/measurement scope.

## Local measurement setup

The sealed json_decode wrapper was attempted before the change. Missing locked
crates were fetched, after which this Mac's system linker rejected the produced
Rust runtime dylib with `mis-aligned LINKEDIT string pool`. The failed wrapper
retired its children and cleared executable/build artifacts; compact failure
logs remain. This is a host linking limit, not a parser result.

The local cost comparison therefore uses the unchanged json_decode/json_soa
Align kernels through the ordinary static-runtime executable build. It retains
the same LCG-generated four-field input, full/projected AoS/SoA paths, row-count
and exact aggregate checks, and three fresh processes with forty samples per
path/size. Compiler/runtime/source/input identities and all raw samples are
retained. Both sides use release compiler/runtime, release generated code, native
CPU, disabled runtime LTO and codegen cache. Input I/O and validation checks are
outside timing; every measured aggregate must equal the independent generator.
This is a before/after Align cost observation, not the unavailable serde duel.
