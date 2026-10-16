# Independent Copy reads from unfinished array builders

Status: independent boundary review accepted; complete capability implemented.
The focused owner bundle and final preflight verify the candidate before merge.

LLM Request28 needs accumulated numeric columns to be readable while growing.
`expert_trace.align` currently preallocates `name_start`, `name_end`,
`name_family_len`, `op_start` and `op_end`, assigns a new entry once, then reads
old entries during interning. Plan140 supplies length observations but no element
read. This capability supplies an independent value read without exposing builder
storage. The settled no-views-before-freeze strategy remains intact. Text-builder
byte views and view-bearing element reads need a separate provenance boundary;
they do not follow from this value-copy capability or resume K1/Plan61.

All three request registers were reconciled. Media uses existing heap scalar and
record builders, including tokenization and tensor-shape accumulation. Voice
uses the same builders; V006 concerns the deliberately closed heap-record field
grammar and is not reopened by reading already-admitted elements. No sibling
code, compiler pin, tests or adoption state may be changed by provider delivery.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Expression | `b[i] -> T` on an already-admitted `array_builder<T>`, exactly one index with the existing i64 index typing. The receiver is a stable bound local or named parameter, matching builder length observations. Immutable, mutable, ByValue, Borrow and BorrowMut receivers may read where their existing type/mode formation is admitted. Temporary/control-result receivers must first be bound. No field, aggregate, closure/task capture or new parameter-mode admission. |
| Element domain | T must satisfy the existing `independent_copy_type` complete-graph predicate from Plans182/205/207. Intersect that predicate with existing builder-element formation, rather than introducing a parallel type list: valid numeric/bool/char/unit/RNG leaves, numeric vectors/masks, ordinary records, tuples, fixed arrays and Option/Result/sum carriers are eligible only when the existing heap or region builder already admits their exact shape. Every reachable field, zero-length element and inactive payload participates. Views, strings and other owners, pointers, callables, malformed IDs/shapes and inline cycles reject. No new element, heap-record field, generic bound or layout rule. |
| Value and lifetime | Read one initialized element by value. It is an independent Copy result with no retained builder, payload-storage, caller, region or generation dependency and may survive later push/append, build, replacement, move, return or Drop. The builder remains the sole owner of its header and backing. No address, slice or borrowed descriptor becomes a source value; no implicit deep clone, source nulling, element extraction or per-result Drop. |
| Evaluation and access | Evaluate the receiver once, reserve its complete stable owner, then evaluate the index once. Keep that read reservation through the native bound check and completed value copy. Moving, finishing, replacing or exclusively accessing the builder during index evaluation rejects; unrelated mutation and nested shared observations remain valid. A diverging index performs no native read. Reads release their reservation at completion, so a copied result does not block later growth or finish. |
| Validation and errors | Source validation checks the receiver expression, stable-place/domain admission, then index typing; any failure returns Error, never a typed Index. Checked HIR independently reconstructs the same receiver/element/result and i64 index. Runtime first converts total initialized length to i64, then checks `0 <= i < len`, then selects the initialized slot and copies it. An unrepresentable length raises existing allocation-size failure; an invalid index raises the existing hard bounds error, with no allocation or mutation. Reserved capacity does not count. Empty and zero-width builders use exactly the same signed bounds rule. |
| Effects and allocation | Pure, read-only, nonconsuming observation. No heap/region allocation, reallocation, compaction, mutation, I/O, synchronization or cleanup is performed by the read. Ordinary by-value copying of inline T is the visible value operation; compiler stack temporaries retain existing rules. Heap and region allocation/growth/build/Drop policies remain unchanged. No latency, asymptotic-performance or application-speedup promise. |
| Native ABI | Add `ArrayBuilderAt` / `align_rt_array_builder_at(*const ArrayBuilder, i64) -> *const u8`, existing A44 `ptr(ptr, i64)`. A null builder has length zero and every index fails bounds. A nonnull builder must address a live valid native header and initialized prefix; no mutation/free may overlap the complete read. After representable-length and bounds checks, return the selected native bytes for an immediate compiler load; zero-width elements return null and must not be dereferenced. The address is invalidated by any later mutation/build/free and is never a source-level result. Nonzero offsets use checked size arithmetic and the existing allocation-size limit. Heap storage uses its contiguous prefix; region storage locates the ordinal among initialized chunks. No native text/wire input or persistent format. |
| Native effects | Conservative IndirectStorage, ArgMem Unstated, params [], escapes [0], release None, fresh false, divergence false, with no curated LLVM noalias/readonly/nocapture claims. Source purity does not imply a total/speculatable call: bounds failure must remain observable. The compiler's dedicated stack-header eligibility recognizes the operation as a call-scoped read without making arbitrary calls nonescaping. |
| Compiler proof and lowering | Reuse HIR Index with an exact builder-receiver branch. Lower to one explicit `Rvalue::ArrayBuilderRead { builder, index, elem }`; MIR validates the exact readable builder element and the complete independent Copy result, including discarded results. LLVM only lowers that semantic operation: native checked address selection followed immediately by an alignment-1 typed load, or an existing zero-value representation for a zero-byte element without dereferencing null. No raw-address SSA or source view is published in MIR. The canonical builder layout/stride and target type layout remain authoritative. |
| Artifacts and platforms | Existing builder type descriptors and concrete template rechecking apply; generic bodies may use the operation only after ordinary concrete admission succeeds. No new bound, interface field/version, CLI/ambient option, cache-format promise, package or runtime header layout. Compiler/source and structural MIR identity invalidate affected artifacts. Whole/per-unit and existing frontend-cache paths must execute the same values on Linux x86_64/ARM64 and macOS. |
| Sources of truth | Update draft, condensed spec, design notes and Settled builder observations, Plan17's builder boundary, checked-HIR ledger19 and runtime ABI ledger20, core array/slice/pipeline English/ja and a narrow guide example with its ja mirror. Add a forward pointer to Plan140 rather than rewriting historical delivery. HANDOFF records the capability once after implementation; R28 receives partial provider delivery only after merge/release build. |

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Formation and all descriptor families | Shared `array_builder_copy_element` composes descriptor extraction, the existing shared `array_builder_element_type_ok` formation predicate, constructor-domain admission and `independent_copy_type`, consumed by source, checked HIR and MIR. Reuse `producer_plain_copy_type_graph` for malformed/inactive/zero-length/recursive graph refusal. New `builder_copy_reads::formation_and_domains` covers all five existing builder descriptor families, heap and region modes, scalar widths, bool/char/floats, Copy records, vectors/masks, fixed arrays/fixed-record arrays and admitted zero-width elements. Pair Move/view-bearing/string/raw/callable/temporary and wrong-index negatives with admissible controls. |
| Eager reservation and control | `builder_copy_reads::evaluation_and_control` crosses index side effects, nested len/read, invalid negative/end/empty indices, if/match/else/?/map_err, branch/loop joins and index return/break. Replacement, build, move and exclusive helper calls during the index must reject; independent writes must pass. Observable index output precedes bounds failure. No load or following instruction is appended after a terminating index. |
| Independent results and lifetime | `builder_copy_reads::lifetime_and_growth` reads primitive and nested Copy values before growth, then finishes/replaces/moves/drops the builder and uses the saved results. Return values across local/arena/loop retirement and helper calls. Forbidden view-bearing graphs remain rejected rather than losing provenance. Existing array-builder transfer and region lifetime owners retain real builder and stored-element dependencies. |
| Native initialized storage and resource parity | New `array_builder_read_tests::array_builder_reads_initialized_prefix` parameterizes heap/stack headers and one/multiple region chunks, first/last/boundary ordinals, reserved-only tails, zero stride and all represented byte widths. Check unchanged header/data/chunk identity and exact element bytes before/after reads, growth, append and finish. Existing thread-local allocation observations with an active positive witness prove no allocation/free; existing ownership guards retire headers/chunks. `array_builder_read_tests::array_builder_read_failures` owns isolated bounded negative/high/null/unrepresentable-length probes and validation precedence, using existing owned child/temporary machinery. No fabricated dangling pointer is tested as valid input. |
| Checked HIR and malformed input | `builder_copy_read_hir_matrix` mutates receiver family, local identity, temporary receiver, index type, result and element graph. All four public checked lowerers reject forged records before MIR/LLVM, including discarded expressions. HIR Index visitor/source-shape/effect/region/escape/Move/borrow/replay/finalization consumers receive an explicit sibling audit; the unchanged ExprKind inventory tripwires still run. |
| MIR producer and LLVM | `builder_copy_read_mir_matrix` mutates owner/read authority, type, element, index, use/unused result and retired/replaced source. MIR reads retain the exact stable-slot Load snapshot; a finite CFG-state pass tracks native ownership identity through slots, arguments and Load/Use SSA. Publishing a moved owner consumes older aliases; Drop/build retire every alias, and replacement/exclusive access invalidates the read reservation. An exclusive callee may replace the header, so it retires pre-call SSA aliases and publishes a fresh caller-place identity; source nulling retains only the pending moved SSA until its consuming operation. Canonical identity renaming converges through allocation loops without reviving retired SSA. The malformed owner crosses original/forwarded SSA, Drop/build/DropValue, straight/joined control and used/discarded reads; whole/partition certification and LLVM reject each. Legal move, fresh replacement across repeated loop iterations, and post-call reload after exclusive helpers remain positive controls. Builder parameters in callable types remain excluded by existing scalar-signature formation; indirect-call cases are therefore outside the admitted source domain, while the common MIR call-mode classifier handles them conservatively. Existing checked-HIR region replay owns region retirement. The operation cannot manufacture an independent value from an unreadable or wrong-layout owner. Codegen validates before native address selection; `builder_copy_read_native_layout` covers bool/sub-byte memory, scalar widths, vectors, nested aggregates, padding and zero-byte values. Runtime ABI typed signatures, canonical key/shape/effect/export inventories and goldens cover the new A44 symbol in every probe feature configuration. |
| Optimizers and stack headers | Audit every ArrayBuilderLen consumer plus Index classification and every MIR Rvalue visitor/hash/print/pass. `builder_copy_read_header_selection` pairs nonescaping owned headers with borrowed named calls and reads between pushes/appends. Reads remain ordered with writes and retain the existing stack/boxed/arena header decision, build transfer and one cleanup. No generic call receives a new nocapture assumption. |
| Import/generic/cache parity | `builder_copy_reads::imports_and_cache` executes imported ordinary and concretely instantiated generic helpers in whole/per-unit and cold/hit/type-edit/restore frontend caches. No unresolved parameter reaches emitted HIR/MIR. A generic body whose actual view-bearing T fails the closed predicate must reject after instantiation. Existing builder parameter/transfer/region targets retain neighboring behavior. |
| Ownership lifecycle | Existing construction, move-in/move-out, source nulling, Drop, replacement and return are unchanged. New owners insert reads before/after those existing boundaries and keep exact cleanup parity; stored Move/view contents cannot enter the new read surface. Borrowed builder parameters still cannot finish/store/escape. No task/native/process-global state changes. |
| Test topology and docs | Driver compile/link/run/cache/resource probes use `helpers/owned_fixture.rs` unchanged, with one exclusive ArtifactStage and bounded process group. Reuse its lifecycle owner. Check only the newly changed normative examples, keep EN/ja code equal, and run the appropriate focused owner bundle, bounded gate, separate-target Clippy and local DB verification if the committed classifier requires it. No benchmark without a new measured performance promise. |

## Author ledger pass and capability boundary

The public record fixes the exact receiver/index/result, complete element graph,
evaluation/reservation interval, error precedence, value lifetime, allocation and
native pointer preconditions. The native address never becomes a source value or
a MIR raw-address operand; only the authenticated operation may consume it.
Zero-width elements retain bounds checking without a load. Every allocation-mode,
header-mode and descriptor-family state is in the parameterized owners above;
there are no optional arguments or new configuration states. The operation
neither promises a retained view nor changes builder construction/growth policy.

This read is useful independently of the text-view portion of R28: numeric span
columns can grow while their initialized values remain readable. Source admission,
reservation, native selection, immediate typed copy and all validation boundaries
must ship together. The exhaustive MIR consumer/ABI sweep and owner matrix may
exceed roughly 1,000 handwritten lines; splitting a dormant native producer from
its only authenticated consumer would duplicate proof and increase integration
risk, so retain one complete capability.

One fresh independent adversarial review of this ledger and boundary is required
before implementation. Resolve findings in this ledger first. Before the code
review, map every row to the actual diff and discriminating test. The request
pass ends after all remaining requests are processed; it must not continue into
unrelated planned work under the owner's latest instruction.

## Implementation closure

- Source formation shares the existing descriptor gate; root Unit, RNG and tuple
  descriptors remain excluded even though independent records may contain some
  such leaves where existing region formation permits them. Abstract RegionPlain
  templates defer the independent-graph check to concrete body rechecking.
- HIR Index retains existing traversal, effect and replay machinery. Region,
  escape content, header, storage-root and borrow-fact consumers detach only the
  completed authenticated copy. MoveCheck reserves the complete stable owner
  before evaluating the index, outside the eager child worklist.
- MIR uses ArrayBuilderRead and an identity-preserving stable-slot snapshot check
  through CFG joins;
  exact native-owner producer contracts cover both whole and projected independent
  results. Embedded-type collection/remapping, print identity and loop-fact
  rejection explicitly classify the operation. Byte preparation/storage optimizers
  retain their fail-closed defaults; cold inference sees no new program call.
- Native selection uses existing heap/chunk layouts and A44; the immediate LLVM
  load has alignment 1 and zero-byte values skip the load while retaining the call.
  The stack-header classifier permits this call-scoped receiver use. Native owners
  check Rust and C allocation/reallocation/free counters, each with an active witness.
- `builder_copy_reads` owns domain/representation execution, index exclusion and
  side effects, terminating indices, independent lifetimes, imported ordinary and
  generic bodies, and cold/hit/public-type-edit/restore cache execution. Existing
  builder transfer and region owners retain storage/Move contents outside this read.
  `builder_copy_read_hir_matrix` covers all checked lowerers; codegen's three
  `builder_copy_read_*` owners cover malformed/unused/retired MIR, target layout
  and stack/boxed headers. `producer_plain_copy_type_graph` also checks builder
  read exclusion for malformed complete graphs. Native owners cover initialized
  bytes/chunks, growth/finish, zero stride and bounded failure precedence probes.
