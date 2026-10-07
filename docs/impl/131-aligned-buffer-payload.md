# Explicit owned byte-buffer payload alignment

Status: implemented capability, independent of deferred K1 and plan 61.
Requests 33 and 127 in the external align-llm register provide the consumer
evidence: native tensor windows require aligned payloads, and a measured WSL
read-address penalty motivates selecting the allocation rather than padding
each consumer's window. Consumer code, pins and qualification remain external.
Alignment alone makes no throughput, physical residency or GPU promise.

[Plan 135](135-fallible-buffer-construction.md) separately specifies recoverable
construction through try_new/try_filled. This record retains the ordinary
constructors' best-effort/terminal behavior and the shared alignment guarantee.

## Public-contract ledger

Declarations (the last argument is optional builtin syntax, not a new language
default-parameter feature):

```text
buffer(capacity: i64, alignment: i64 = 1) -> buffer
buffer.filled(length: i64, value: u8, alignment: i64 = 1) -> buffer
```

Positional calls:

```align
mut input := buffer(65536, 4096)
mut output := buffer.filled(4096, 0, 64)
```

| Surface | Exact contract and owner |
| --- | --- |
| Inputs/default | Alignment is an ordinary i64 expression, evaluated once after capacity/length and, for filled construction, value. Omission supplies 1. No environment or platform-selected default is exposed. Sema checks the complete argument list in source order; wrong arity/types produce diagnostics. |
| Admission/errors | Supported alignments are powers of two from 1 through 536870912 inclusive, the existing align(N) bound. Every other value terminates with `buffer alignment must be a power of two between 1 and 536870912`, before payload or header allocation, including zero/invalid capacity. This uses existing terminal allocation-precondition behavior, not a new Result or error family. After alignment admission, ordinary capacity retains its nonpositive/unreservable-to-zero policy; filled length retains invalid-size and OOM termination. |
| Storage promise | The first payload address is a multiple of requested alignment. Nonempty storage uses one allocation with that alignment; an empty allocation has a non-dereferenceable aligned sentinel and no payload acquisition. Existing capacity()/len() meanings and initialization are unchanged. Filled nonzero construction initializes exactly length bytes in one payload acquisition. Alignment is a lower bound, never an exact allocator placement. |
| Ownership/lifetime | Return the existing Move buffer. The allocation's requested alignment travels with its owner through move, return, replacement, containers and borrowed helpers. Ordinary replacement transfers the new owner's alignment; it does not retrofit the old one. bytes() retains existing generation, lifetime and read/write authority. No stronger LLVM slice-load alignment or foreign-pointer authority is inferred. |
| Mutation | Put, append, append_filled and growing read_line preserve the allocation's requested alignment; growth may move the address and invalidates views under existing rules. Bounded reader/file/socket/HTTP/random fills preserve the allocation and alignment. Short/EOF/error behavior, initialized length and read-window capacity are unchanged. |
| Allocation/Drop | One BufferStorage implementation owns pointer, initialized length, allocation capacity and alignment. It uses checked size/Layout formation and matching allocation/deallocation layouts. Growth uses checked std::alloc::realloc with the original Layout and unchanged alignment, preserving the initialized prefix. Null retains the old owner; success publishes the returned provenance/capacity before any observer, even when the address is unchanged. Zero-capacity growth uses alloc. Existing caller failure policy remains. Hidden allocator capacity never enlarges the public read window. No implicit copy is added to construction, move or replacement. |
| Existing producers | Decoders, HTTP/compression/crypto and other Vec-producing operations transfer their allocation to the same storage owner at alignment 1, with no copy. They promise no explicit higher alignment. Existing nonempty Vec capacity determines its exact deallocation Layout. |
| Purity | Constructors remain Pure allocation operations. Runtime alignment admission adds no native I/O, global state, descriptor or retained input. |
| Syntax relationship | align(N) continues to describe struct/fixed-array storage. A buffer binding holds a movable handle, so aligning its binding slot would not select payload alignment. Constructor arguments select the allocation directly and work inside expressions and helper returns without hidden reallocation or syntax-only initializer exceptions. No raw.alloc change or second aligned-buffer type is introduced. |
| Native ABI | Replace existing compiler-private signatures with `ptr align_rt_buffer_new(i64 capacity, i64 alignment)` and `ptr align_rt_buffer_filled(i64 length, i8 value, i64 alignment)`. No compatibility symbols. RuntimeKey identities/counts and fresh-owned-result effects stay; signatures, scalar C ABI attributes and runtime fingerprint change together. Native alignment checks are identical to source calls. |
| IR and persistence | BufferNew gains a required i64 alignment child/operand; omitted source arguments become constant 1 before HIR publication. Validate exact Buffer/i64/u8 equations before lowering/LLVM. All traversals, substitution, effect/provenance and source serialization preserve/evaluate the child. No new enum variant. Generic interfaces persist source templates; no interface encoding change because interfaces persist source templates rather than BufferNew bodies. Compiler/runtime build identities invalidate prior artifacts through existing content fingerprints. |
| Optimization | Existing stack-buffer promotion remains eligible only for constant alignment 1. Other constant/dynamic alignment constructors stay on their explicit allocation path. Promotion must never elide validation, manufacture a payload alignment or change the public read window. No new aligned-load optimization is selected. |
| Prerequisites/boundary | Uses shipped buffer ownership, capacity and bounded pread. No deferred interprocedural authority work, fallible-allocation redesign, custom allocator, static aligned-buffer type or consumer adoption is required. Recoverable allocation failure stays a separate design. |

## Implementation closure matrix

| Axis | Implementation and exact acceptance owner |
| --- | --- |
| All supported alignments and storage transitions | `align_runtime::buffer_storage::tests`: all powers 1..2^29 as empty sentinels and real allocations/growth at 1, 2, 8, 64, 4096 and 16384; independent address-modulo, byte and capacity observations after reserve/grow/resize/clear/set_len/Vec transfer. Instrument exact allocated/deallocated pointer, size and alignment with scoped test hooks, including failed reservation. No speculative large resident allocation. |
| Invalid native admission and precedence | `align_runtime::tests::aligned_buffer_invalid_alignment_precedes_every_allocation`: isolated bounded children for zero, negative, non-power-of-two, too-large and i64 endpoints; cross new/filled, zero/negative/huge size and next-allocation failpoint. Exact admission diagnostic precedes header/payload acquisition; a valid allocation failpoint control proves the witness active. |
| Construction/put/append/self-append/fill/line/read/EOF/error | Parameterized native owner spans ordinary/filled/zero/failed-hint construction, every growth route, alias-safe copy, bounded pread, line growth and reader replacement. Retain existing m9_io, buffer_pread_into, native buffer-storage/line tests and consumer suites for the shared storage. |
| Source formation and malformed producers | New `align_driver --test aligned_buffer` covers both optional argument forms, wrong arities/types and no special const-only syntax. Checked-HIR and MIR owners mutate wrong alignment/result/size/fill equations and missing SSA values; rejection precedes LLVM. |
| Evaluation/control paths | `aligned_buffer` crosses dynamic arguments with ordered side effects and terminating first/second/third operands. Existing lower-required guard must prevent allocation after return/? termination. Returned owners cross if/match/else/?/map_err, loop joins, replacement and early exits using existing cleanup. |
| Move-in/out/nulling/Drop/borrowed view | `aligned_buffer` whole/per-unit native probes observe actual payload addresses/bytes across move/return/replacement/read/growth; 54 constructed handles, including repeated success and early-return paths, are counted against exactly one non-null free each before process exit. Existing stale-view/moved-owner negative programs remain refused for aligned constructors. No new authority escape. |
| Imports/generics/interface/cache | Imported helper taking dynamic alignment and returning Buffer runs whole/per-unit, with generic wrapper replay. Cold/warm/change/restore cached compilation observes selected alignment. Source templates and replay must retain explicit arguments. |
| Runtime provenance/concurrency | Storage keeps its writable raw pointer from acquisition/exclusive growth. Shared bytes() publications obtain no exclusive reference; preserve existing concurrent-read and writable-alias owners. Native allocation address and exact Layout remain owned until Drop. |
| Optimization/ABI | Stack-buffer qualification tests admit default/explicit-1 and refuse other/dynamic alignments; alignment argument termination stays reached. Runtime ABI registry/export and unsigned-byte attributes cover changed signatures on Linux/macOS. apps/kv/pkg/kv.align and the pkg_kv_control/parser/faults/alloc intercepted declarations compile against the one new ABI; run those focused consumer owners. |
| Resource evidence | Scoped native allocation records prove no payload for zero, one for filled construction, no allocation for move/bounded read, bounded geometric growth and exact matching frees. These are allocation contract checks, not RSS/latency benchmarks. |
| Platform | Native and focused whole/per-unit owners run locally on macOS and Linux before push; full platform CI remains the final guard. Final batch runs exactly `cargo build --release --workspace`. |

One capability joins the storage owner, constructor operand and all existing
consumers. It may exceed 1,000 handwritten lines: a dormant raw-storage rewrite
followed by a dormant IR operand would repeat the same ownership, native ABI
and platform proof. Shipping the useful constructor family together lowers
integration risk. The public contract and allocation strategy require a fresh
independent adversarial plan review before implementation, then one final
committed full-diff review and local gates under the repository workflow.

## Author ledger-to-prose pass

Every public argument, default, evaluation/admission order, terminal versus
best-effort outcome, payload lifetime and allocation has one row above. There
is no exchanged byte format, text input, process-global state or producer
inspection table. Alignments are not type identity or new nominal/structural
fingerprints; they are runtime values retained by the allocation. Existing
source/interface/runtime identity owns cache separation. No performance
benchmark is required because no latency or throughput improvement is promised.

Required agreeing sources: draft.md, docs/language-spec.md,
docs/design-notes.md, Settled docs/open-questions.md, checked-HIR ledger 19,
runtime ABI ledger 20, core-design/string.md and its ja mirror. Capability status is recorded in HANDOFF/roadmap. Requests 33/127 receive
the exact shipped disposition and PR; Request 35 remains partially deferred.
The external register edit stays uncommitted. Source examples and the exact
optional argument calls are syntax-checked by aligned_buffer.

## Independent plan review

The fresh inspection-only adversarial review was CLEAN. Constructor arguments
preserve the settled align(N) storage meaning. The native pkg.kv declaration
and intercepted fixture owners are included in the ABI matrix. Request 33's
historical recoverable-allocation criterion remains expressly deferred; this
capability closes its payload-alignment requirement and Request 127.

A scoped follow-up review accepted allocator realloc for growth, preserving
existing in-place opportunities. Failed realloc retains the old owner; successful
returned provenance is published before test instrumentation can unwind.

## Author implementation closure

The ledger-to-diff pass maps admission, storage layout, failure retention and
resource obligations to BufferStorage::new/grow/Drop, the native constructors,
`aligned_storage_growth_and_adoption_match_every_allocation_layout` and the two
native aligned_buffer owners. Allocation observation pins one constructor
payload, no empty payload and no move/bounded-read payload events; each acquired
layout has its exact retirement. Existing writable-alias/shared-publication and
unwind owners remain active.

The source owner `aligned_buffer_whole_unit_imports_growth_and_control` delegates
54 counted generated acquisitions/frees to the real runtime on both compilation
paths. It covers termination before construction, owned cleanup on `?`, loops,
returns, branch selection and replacement. Type/stale-view/move negatives,
`buffer_alignment_hir_requires_exact_operand_and_result`,
`buffer_alignment_mir_requires_exact_operands_and_result`, stack qualification
and generic cache edit/restore close the formation, traversal and persistence
rows. The ABI registry/golden/export audit and four pkg.kv owners close the only
native source consumer. No deferred K1/plan61 proof is assumed.
