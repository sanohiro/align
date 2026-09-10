# Incremental SHA-256

Status: implementation selected by the owner on 2026-09-10; exact contract
reviewed with the two inventory corrections recorded below. R29/H1 in plan 40 owns the
consumer selection. This ledger owns the capability below.

## Public contract

| Surface | Exact contract |
| --- | --- |
| `crypto.sha256_stream() -> crypto.digest` | Requires `import std.crypto`; no arguments/defaults. Creates an opaque Move owner of one EVP SHA-256 context and checked byte count. Constructor/provider/allocation failure aborts as the existing one-shot digest does. No partial owner escapes. |
| `d.update(data: bytes) -> ()` | Receiver is a bound digest local or a `borrow mut` parameter, evaluated before data exactly once. Shared-borrow receiver rejects; ordinary owned local can mutate its native state without a language `mut` binding, as other handle methods do. Input is exactly str, string auto-borrowed to str, or slice<u8>, with no retention. Buffer requires `.bytes()` and arrays require an explicit byte slice; direct owners reject. Arbitrary bytes/NUL and zero-length input are valid. Check cumulative length <= `2^61-1` before EVP; overflow aborts. EVP failure aborts, never a partial successful digest. |
| `d.finish() -> array<u8>` | Bound owned digest local/by-value parameter only; no arguments. Consumes it and nulls its source before the terminal native call. Releases the context and returns a standalone 32-byte owned array. Borrowed finish, reuse and double finish reject. No reset, clone, manual close or algorithm selector. |
| `crypto.digest` | Nominal builtin with no bare alias. One opaque pointer in native slots; recursively Move. Local, by-value/shared/exclusive helper parameters, return, ordinary record/sum/Option/Result carriers and existing owned-record arrays follow the common opaque-handle carrier model. Direct digest or tagged/sum-digest collection elements (fixed/dynamic arrays, slices, vectors/masks, array builders and pipelines), tuple/box storage, out parameters, global/constants, closure/task/parallel capture, native/layout(C) exposure, print/equality/order/hash reject. The permitted recursive carrier grammar is digest, ordinary natural-layout struct fields, sum payloads, Option/Result, and fixed/dynamic AoS arrays of existing heap-tree records (digest joins the admitted leaf set; record/Option edges remain the existing row restriction); direct digest collections are not part of that grammar. Shared and exclusive borrowed parameters may read/replace their ordinary carriers under existing borrow rules; only exclusive digest receivers may update. This is the signature-key carrier inventory with borrow mut deliberately admitted, not its key-only parameter restriction. No borrowed view into the context is exposed. |

All three operations are Impure. A context has no retained input or arena roots;
the output is not region-bound to any update. Owners in records must be moved to
a local or passed to an explicit borrowed helper for a method operation; no new
implicit field receiver form is introduced. General whole-owner replacement
continues to drop the previous initialized owner once. No new reference syntax,
user lifetime, error variant, persisted application format or CLI option.

Source argument validation checks method/arity, receiver and each admitted child
once, then ownership/mutability. Invalid children never publish typed HIR. A
terminating receiver/input performs no later call or consumption. The digest
receiver must remain valid through eager evaluation of the input.

## Native ABI and lifecycle

| Runtime key / symbol | Exact C/LLVM shape | Rules |
| --- | --- | --- |
| `CryptoDigestNew` / `align_rt_crypto_digest_new` | `ptr ()` | Allocates EVP context and shell; initializes SHA-256 before publishing. |
| `CryptoDigestUpdate` / `align_rt_crypto_digest_update` | `void (ptr, ptr, i64)` | Exclusive live shell then valid borrowed bytes. No input/shell overlap. Check pointer/length/overlap/total before forming a Rust slice or entering EVP. Null with zero length is valid; negative length or null with positive length aborts. |
| `CryptoDigestFinish` / `align_rt_crypto_digest_finish` | `{ptr,i64} (ptr)` | Consumes exactly one live shell. EVP final writes bounded stack storage and must report exactly 32 bytes; free context/shell and allocate normal owned-array output. Impossible provider results abort. |
| `CryptoDigestFree` / `align_rt_crypto_digest_free` | `void (ptr)` | Null is a no-op for moved slots; otherwise release context then shell exactly once. |

These are typed RuntimeKey records using existing ABI shapes. LLVM performs no
ownership decision. Drop plans for every recursive carrier select DigestFree;
crypto capability discovery includes digest leaves reachable only through Drop.
The provider stays the existing system OpenSSL dependency, using the default
SHA-256 EVP implementation just as one-shot hashing does. No change to one-shot
provider configuration or process-global OpenSSL state. Fatal abort/OOM has no
unwinding guarantee. Native callers must supply valid allocated shell pointers;
malformed compiler records are rejected before emission, not recovered by probing
arbitrary native addresses.

Initialize and update use `EVP_MD_CTX_new`, `EVP_sha256`,
`EVP_DigestInit_ex`, `EVP_DigestUpdate`, `EVP_DigestFinal_ex`, and
`EVP_MD_CTX_free`. Keep at most one fixed shell/context independent of total input
size; do not store chunks or promise zero OpenSSL allocations. No throughput or
exact RSS promise. A fixed-chunk increasing-total-input measurement owns this
storage claim; native counters also witness constructor/Drop/finalization.

## Compiler identity and validation

Add `Ty::CryptoDigest` and `Scalar::CryptoDigest` through the complete type-class
sweep. Canonical nominal type and scalar tags are respectively 73 and 51, whose
independent encode/decode goldens include malformed-tag neighbors. The exact
nominal spelling is `crypto.digest` and its import owner is `std.crypto` in both
source and reconstructed interfaces. No new interface field encoding: new builtin
membership is checked by the existing canonical table and compiler identity.
Canonical graph type bytes change for new types only; existing type bytes remain
unchanged. Compiler/runtime fingerprints invalidate affected artifact caches.

HIR adds New, Update(digest,data), Finish(digest) records with exact result types
Digest, Unit and dynamic u8 array. Native MIR operations mirror these records;
Finish lowering nulls the evaluated owned source through the existing move path.
All depth/replay/finalization, effects, liveness/escape, MoveCheck, producer proof,
HIR validation, MIR canonical/printing and LLVM operand/result checks participate.
Malformed shared Update and borrowed Finish reject independently in checked HIR;
new records cannot fall through a generic successful validation fallback.

## Implementation closure matrix

The following names are owners in `m11_crypto_stream` unless qualified.
Reuse their parameterization for multiple cells.

| Axis | Implementation / owner |
| --- | --- |
| Constructor/update/finish correctness | Runtime EVP context; `stream_vectors_and_partitions`: empty, abc, 55/56/63/64/65-byte boundaries, repeated empty and many updates, binary NUL input, one-shot parity. |
| Formation/invalid domains | Sema builtin/method gate and interface table; `stream_formation_and_carriers`: imports, arity, wrong data, shared update, borrowed finish, post-finish use, forbidden carriers. |
| Construction/move/Drop/replacement/return | Common recursive DropPlan and null_moved_source; `stream_owned_control_flow`: direct, helper, struct/Option/Result/sum, replacement and repeated cycles, exact live/free witnesses. |
| if/match/else/?/map_err/joins/loop/early exit | The same carrier/control owner crosses successful and early-exit paths; no new ownership exceptions. Returned digest retains no input. |
| Borrow/source expiry/eager operands | `stream_borrowed_inputs_and_interfaces`: data expires after update, exclusive helper, later operand moving/replacing receiver rejects, unrelated owner mutation succeeds. |
| Whole/per-unit/generic/cache | Both compiler modes in driver helpers; imported signatures and generic carrier; cold/hit/private edit/restore fixture owned by the existing frontend-cache target if needed. |
| Malformed HIR/MIR | `hir_digest_records_reject_malformed_types_and_borrowed_consumption` and `digest_mir_gate_preserves_owner_identity_and_authority`: receiver/result/input type, shared receiver, borrowed finish and forged owner producers rejected at every checked entrypoint. |
| Type/ABI/link closure | Variant tripwire, canonical nominal encode/decode goldens, runtime declaration/export structural owners; a drop-only imported helper retains crypto linking. |
| Native faults/allocation | Runtime tests: invalid bytes, overflow-before-EVP, null Drop, constructor/finalizer cleanup, empty pointer, known vectors and fixed-storage counter measurement. Fatal cases use isolated subprocess owners. |

Expected boundary is roughly 1,000–1,500 handwritten lines including the full
variant sweep and parameterized owners. One end-to-end PR avoids a dormant
owning type whose constructor exists without correct terminal/Drop/transport;
the four native operations and all compiler layers share exactly one ownership
proof. This is a reason to keep the capability together, not to skip gates.

Synchronize `std-design/crypto.md` and its Japanese mirror, `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, the crypto Settled entry, and
HIR/runtime ledgers. Record implementation status at capability completion;
align-llm adoption remains pending and its code is outside this repository task.

Author ledger pass: the operation/state product is New, live Update, consuming
Finish, null/non-null Drop; all borrowed/owned receiver states are explicit.
There is no runtime inspection record, options/default product, global lease,
text encoding boundary (input is bytes), or new public serialization format.
Native widths and private canonical tags are fixed above, with corresponding
independent owners. This plan has no executable new-API examples before source
support exists; driver fixtures will supply checked declarations and calls.

Independent plan review found two P2 inventory gaps. The corrected ledger fixes
exact byte-view inputs and the closed carrier grammar (including Out and
layout(C)); source and malformed-HIR owners must exercise that complete inventory.

## Author closure and local evidence

The type-class sweep covers Move/Drop, scalar layout, enum payloads, heap-tree
rows, import reconstruction, canonical codecs and native producer authority.
Digest Update participates in completed exclusive-place snapshots so later eager
input evaluation cannot move or replace its receiver. Digest Finish creates
fresh empty-content storage with individual allocation even inside an arena.
Both rules are exercised through whole-program and per-unit drivers; the same
parameterized carrier owner covers records, sums, Option/Result, fixed/dynamic
record arrays, generic and function-value forwarding, joins and early exits.
Negative source fixtures close the excluded storage and borrowed-terminal cases.
Malformed HIR is checked through every lowering entrypoint; malformed MIR is
rejected by the native producer gate.

Native owners cover five invalid-input/overflow subprocess cases, known vectors,
a million-byte fixed-chunk vector, null Drop, unfinished Drop, and injected
provider-failure cleanup before publication and after consumption. The test-only
live-context counter and fault control add no native exports. Fatal OOM remains
process abort with no unwinding promise; it is not a recoverable allocation API.

Local storage measurement (`digest_storage_measurement`, ignored outside an
explicit benchmark run): one 16-byte shell and one live context at 1, 16 and
64 MiB total input using a fixed 64 KiB chunk. Linux RSS was 10,708 / 11,796 /
11,800 KiB. This witnesses bounded retained state, not an exact RSS guarantee or
throughput threshold. The ABI golden adds exactly four declarations, using A47,
A73, A83 and A62; in particular New has no `nofree` attribute.
