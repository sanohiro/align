# Plan244: exclusive external views and synchronous file windows

AM-R07 needs direct synchronous positional reads into externally owned memory.
Plan240 ordinary function results remain read-only. No writable-return relation,
permission type, owner-free view or new ownership model is proposed. Producer
wrappers keep the write within an explicitly exclusive owner helper; they may
return counts or read-only views. Consumer GPU registration, transfer completion,
publication and performance qualification remain external.

## Public ledger

`resource.view_mut_from_raw(owner, ptr, len)` is an unsafe, privileged counterpart
of `resource.view_from_raw(owner_ref, ptr, len)`. It requires a bound mutable
owned resource Local, including an existing `borrow mut R` parameter, not a
resource_ref/shared parameter/temporary/partial Move field. This is the existing
exclusive whole-owner domain, not a new partial-owner exception. The result is
expected `Option<slice<FFIScalar>>` (existing integer/float elements; no str).
ptr is raw, len i64 elements, no defaults. All operands evaluate once left to
right. Bind the completed owner before later operands; a later ptr/len expression
may not move, replace or exclusively borrow that owner before the intrinsic.
Early exit before the action has existing cleanup and no new view/invalidation.
The action authenticates the owner's write authority and publishes a writable
view tied to its existing generation on Some. Formation does not write memory
or end that generation, just as local owned-buffer view formation does not.
Actual writes/exclusive calls still invalidate overlapping earlier observations
and resource dependencies under the existing temporal borrowing rules. Ordinary
read-only views returned by helpers never gain write authority; `borrow mut`/`out`
view calls prove actual local backing writable and disjoint from all peers.
Copies/slices from this producer share the selected resource root; two raw
pointers or two aliases from the same resource cannot prove disjointness. A new
view does not grant permission through a shared ResourceRef. Independent resource
owners may be disjoint only under the unsafe wrapper's unique backing obligation;
borrowed child resources retain parent alias dependencies. Empty views retain
owner lifetime and alias identity.


No memory allocation, copy, initialization, retention or Drop is performed.
The unsafe wrapper must prove the entire selected element range is initialized,
live, writable, correctly aligned and owned by this resource generation, with no
untracked foreign concurrent access; it also owns external transfer completion.
Do not allow an uninitialized memory slice merely because the next operation is
a writer: short read/error leaves the rest unchanged and readable. Native
construction returns None for negative/unrepresentable byte length, nonempty
null, misalignment, or pointer-end overflow, in that order without pointed-to
access; empty null is valid. Existing shared view must receive the same checked
numeric/address admission rules rather than drift to a weaker sibling. No
runtime range registry can prove foreign validity. Unsafe is still Impure.

`f.read_at(offset: i64, destination: slice<u8>) -> Result<i64, Error>` is an
Impure shared File borrow plus exclusive initialized byte-view access for one
synchronous positional read. Receiver is a bound File Local under existing
File rules, destination a bound writable byte view (or existing fixed-array view
coercion), including `borrow mut`/`out` helper parameters. Offset has no default;
choose a subwindow by binding a slice range before the call. The slice descriptor
and File survive Ok/Err. No descriptor rebind, allocation, resize, initialization,
copy or address retention. One ordinary pread, retry only EINTR as existing
File.pread_into, no cursor change. Return actual count, including short read and
zero EOF; only returned prefix changes, remainder unchanged. Admission failures/zero/EOF preserve bytes; native Err may
alter bytes inside the window and publishes no valid read count, while bytes
outside it and full initialization remain intact. There is no rollback promise. Zero valid length succeeds with zero
and performs no syscall. Existing pread_into remains the distinct owning Buffer
operation that can extend initialized length, not a second view spelling.
Explicit source calls invalidate prior byte-validation observations even on
zero/error paths, as current writers do. Later eager operands cannot invalidate
already completed owner/backing addresses. Native positional-read offset/range
validation and fixed errno mapping match the existing File implementation.

ABI: `i64 align_rt_io_file_read_at(ptr File, i64 offset, ptr destination,
i64 length)`. Like existing File operations, nonnegative values are actual counts
and negative values encode `-status`. There is no output counter or new error
encoding. Admission order: (1) reject null/misaligned File pointer and File-shell
address overflow; (2) reject negative offset or negative/unrepresentable byte
length (usize and isize); (3) reject nonempty null destination, destination-end
overflow or overlap with the File shell; (4) read live File fd and reject negative
fd; (5) return zero without syscall for zero length; otherwise call pread, retry
only EINTR and map the existing fixed errno table. Empty null is valid, and an
empty range overlaps no shell. Native caller supplies a live File and valid
initialized writable range, with no alias to the File shell. Actual offset+count
filesystem limits retain native pread errors; no speculative cursor/end change.
The implementation rejects a callback count beyond requested length and uses
checked integer conversion. Production pread only writes the successful prefix.
HostState conservative attributes, no capture/release or new representation.
Runtime key and native export inventories/golden change atomically. Compiler
identity invalidates cached bodies; interfaces carry existing modes/types, with
no trusted permission bit across ordinary results.


## Closure matrix and required owners

- Formation/admission: mutable owning Local vs borrowed-exclusive Local; shared,
  ResourceRef, temporary, partial field, wrong result/string/bool type reject.
  Privilege and unsafe checks survive imported/uncalled functions. Exact native
  numerical admission positive/zero/null/alignment/length/range overflow matrix.
- Owner transitions: move, replacement, into_raw, Drop, exclusive calls end the previous view; no new cleanup action. View does
  not keep an owned resource alive by allocation. Stack/caller lifetimes remain
  inferred, and returning a view of a local owner rejects.
- Access: local writable producer and its range aliases write/read correctly;
  shared old producers, ordinary returned views, readonly aggregates/captures and
  joined readonly/writable sources reject at safe writers. Overlapping peers
  and later-argument owner mutation reject, independent resources pass. Mutable
  descriptors returned/rebound through calls follow Plan240, never mint rights.
- Control: Some/None, if/match/else/?/map_err, loops/branch joins/early returns,
  generic helpers, interface replay and whole/per-unit native execution. A helper
  borrows the whole resource mutably and performs read_at internally, returning
  count/read-only data; no writable returned-view promise.
- Initialization/write: actual resource-owned initialized allocation with canary
  bytes before/after selected range; exact file content, short read, EOF, invalid
  offset, native error, zero length; untouched remainder and counts, no retained
  address/allocation, later independent native transfer/read and exactly-once Drop.
  No GPU hardware is needed for provider safety proof or claimed speed benefit.
- IR: every HIR consumer explicitly handles mutable formation/effects/roots;
  independent checked-HIR rederives owner mode and writable provenance. MIR
  authenticates exact resource owner WRITE/READ proof before publishing writable
  access even for discarded values; shared producer remains Shared. Actual
  native writer requires that proof for destination, independent of used result.
  Malformed used/discarded HIR/MIR matrices mutate owner type/mode/place, result,
  flags, range, count result and shared-to-writable laundering across calls.
- ABI/export/cache: exact signatures and effects for both default/optional exports;
  cold/hit whole/per-unit import consumers; no new persistent permission field.
- Required prose: draft, condensed spec, design notes, Settled, Plan17 resource
  section, std fs EN/JA, HIR/runtime ABI ledgers and exact declaration golden.

This is one useful producer-to-writer capability, not two dormant PRs. It may
exceed 1,000 handwritten changed lines because source lifetime, independent HIR,
MIR producer, native I/O, provenance/control owners and bilingual contracts must
agree. Splitting the writable producer from its native consumer duplicates the
same safety proof and leaves no AM-R07 consumer until the pieces recombine.

Author source mapping: existing File.PreadInto uses i64 count-or-negative-status,
retries EINTR, validates offset nonnegative, and retains the descriptor cursor.
Use the same lowering/error mapping; the new view operation does not mutate a
Buffer descriptor. `MoveCheck` source snapshots reserve completed native owners
and header values; `borrow_mut`/native byte-writer invalidation checks retained
resource dependencies and prior text observations. Unknown resource-derived
headers preserve exact fallback roots rather than mint independent allocation
identity. New writable MIR production must require owned/exclusive resource
provenance; the existing shared ResourceViewFromRaw remains Shared. The checker
must explicitly carry the new local producer through source root admission
instead of loosening unknown-origin admission globally. A parameterized source
and forged IR matrix closes every new authority path; unproved cells fail closed.

Author native references: Linux man-pages pread(2),
https://man7.org/linux/man-pages/man2/pread.2.html ; Apple read/pread(2),
https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/read.2.html .
Neither promises transactional destination rollback on arbitrary native Err.
No kernel allocation claim is made; no Align/user-space allocation occurs.

Consumer inspection: media safetensors.read_window reuses initialized Buffer
windows; gpu.align's resource.view_from_raw reads a native result then appends
into owned output. Voice pkg.gpu.values reads a resource f32 view and explicitly
copies it. LLM native_cuda_load.upload_weight publishes only each actual returned
prefix from pread_into. The new provider surface need not modify any consumer or
change these shared-read producers. External pinned resource and upload timing
remain consumer-owned.

## Accepted strategy corrections and named owners

An independent inspection accepted this single capability with four corrections:
native UTF-8 views must enter temporal validation; byte writes must not end owner
lifetimes; native errors are not transactional; File receivers remain Local.

`ResourceViewFromRaw(StrUtf8)` must publish a byte-validation observation over
the complete resource/parent backing in addition to lifetime dependencies.
Writing a sibling mutable byte view invalidates this observation even though
both views' owner generation survives. This applies equally to direct native
UTF-8 construction and `bytes.as_str()`, projection/copy/helper return/imported
and nested carrier publication. Independent resources remain independent;
borrowed children retain parent overlap dependencies. An unsafe wrapper still
must report the actual owner and cannot fabricate independent overlapping owners.

Element mutation uses `invalidate_collection_mutation_target` semantics: retain
the view descriptor, allocation and owner generation, invalidate overlapping
byte-validation observations. Formation itself changes neither memory nor
owner generation. Move, replacement, into_raw, Drop and exclusive whole-owner
calls use the existing generation-ending rules and reject live Drop-bearing
dependent resources. File.read_at must never use reallocating Buffer invalidation.

| Owner | Required matrix |
| --- | --- |
| driver `external_view_authority_matrix` | owned/exclusive producer positives; shared/ref/temporary/partial field/privilege/unsafe/type negatives; local copies/ranges, same and independent resources, overlapping peer calls; ordinary returns/captures/rebound outputs read-only |
| driver `external_view_lifetime_and_timing_matrix` | owner move/replacement/into_raw/Drop/exclusive calls and dependent-resource cleanup; later eager operand mutations; empty ranges; Some/None, branch/loop/early exit and generic/imported controls |
| driver `external_view_validation_matrix` | raw StrUtf8 versus bytes.as_str; same/independent/parent-child resource; local/helper/imported/carrier observations crossed with direct/native/helper writes; fresh revalidation positives and stale publication negatives |
| driver `external_file_windows_native` | initialized owned native canaries, real full/short/EOF/zero/invalid/native-error reads, unchanged outside bytes and cursor, repeated native read/transfer and exactly-once Drop; whole/per-unit/cache-hit execution |
| sema/MIR `external_view_hir_shape` | exact producer owner/mode/place/resource/view kind/unsafe authority and File read shape, independent replay and all public lowering entrypoints |
| codegen `external_view_mir_shape` | exact owned/exclusive source proof, Shared producer laundering, forged source/call/mode/result/validation flags; writer destination/count proof even when discarded |
| runtime `file_read_at_native_matrix` | scalar/pointer/range/overlap/negative-fd precedence; zero calls; short/EOF/EINTR/native-error injection; canaries/cursor/alloc-count and no retained address |
| native resource view admission owner | empty null and each scalar width, negative/oversize/byte-count/address overflow/alignment, no access on rejection; malformed MIR flags reject |
| existing resource/readonly/byte-operation owners | shared producer compatibility and existing lifetime/access contracts remain; ABI signature/export/golden owners cover all configurations |

The final author obligation-to-diff pass must map these names to the concrete
implemented tests before requesting the one full code review. A missing proof
cell stays rejected; no broad unknown-origin allowance may replace it.

Implementation representation: `ResourceViewKind::SliceMut(Scalar)` extends the existing complete resource-view HIR/MIR node; its owner type is Resource, while the shared Slice/StrUtf8 forms retain ResourceRef. All existing child walks remain explicit through that node. FileReadAt is one new HIR/MIR action.


## Author closure mapping

The driver `external_file_windows` target owns the source/IR/native matrix.
`external_view_authority_matrix`, `external_view_safe_writer_authority`,
`external_view_return_privilege_and_parent_matrix` and
`external_view_lifetime_and_timing_matrix` cover mutable/shared/ref/temporary/
partial-field/type/unsafe/privilege admission, aliases, ordinary returned views,
owner transitions, dependent cleanup, eager operands and control forms.
`external_view_validation_matrix`, `external_validation_carriers_and_writers`
and `external_parent_validation` cross direct native text, bytes.as_str,
helper/record/branch publication, same/independent/parent owners and element,
helper, fill and file writers, with fresh-validation positive controls.
`external_view_hir_shape`, `external_file_argument_and_hir_matrix`,
`external_view_mir_shape` and `external_inline_destination_authority` close
used/discarded independent HIR/MIR type, mode, place, flag and write-authority
mutations, including empty/nonnull inline arrays and fields. The HIR producer
matrix exercises all four public lowering entrypoints.

`external_file_windows_native` executes initialized real-resource windows,
canaries, full/short/EOF/zero/invalid reads, a second foreign read, generic
monomorphization, imported whole/per-unit code, cold/cache-hit ThinLTO and
exactly-once Drop. `external_native_view_admission` executes both native view
producers for every supported integer/float scalar, including empty-null,
nonempty-null, negative/oversize length, alignment and address overflow.
Runtime `file_read_at_native_matrix` and its admission companion own allocation,
cursor, deterministic pre-access validation, EINTR/error/count injection and
out-of-window canaries. Existing resource_ownership, call_boundary_views,
text_byte_provenance and binary_byte_write_effects retain the wider branch,
mutable-output and call/closure regression inventory. No GPU benchmark is
promised or run.

The local projected header stores an optional complete `external_backing` root
set only at an authenticated ResourceViewFromRaw producer. It is separate from
managed-generation `known`: native memory gains no fabricated allocation or
content proof. Exact copies/projections retain it, joins require both alternatives,
generation renaming maps it, and unresolved ordinary calls clear it. Byte overlap
uses this set, including parent dependencies. Resource/ref arguments count as
potential byte sources when a callee publishes validated text, so imported helper
results cannot hide native UTF-8 observations. Fixed-array slice producers derive
backing authority from the original inline slot's parameter mode, after their
existing element-initialization proof; ordinary function results still restrict
view access, and shared incoming/empty arrays remain shared.
