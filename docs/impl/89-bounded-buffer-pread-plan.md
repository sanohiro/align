# Bounded positional reads into an existing buffer (Request 38)

This capability reads directly into a caller-selected region of the existing
buffer allocation. It reuses File, Buffer, Result/Error and their established
exclusive byte-storage discipline. The source is a live bound File and the
buffer follows the existing native-fill bare mutable-local rule. K1 remains deferred.

## Public-contract ledger

| Surface | Exact contract and owner |
| --- | --- |
| Method | `f.pread_into(b: mut buffer, destination_offset: i64, length: i64, file_offset: i64) -> Result<i64, Error>`. Four positional arguments, no defaults. Impure and nonconsuming. File remains a bound local under its existing receiver rule; buffer is a bare mutable local, including an existing borrow-mut helper parameter. Direct fields, indexed places and temporaries retain the existing native-fill rejection; callers may pass supported places through ordinary exclusive helpers. Arguments evaluate once in source order. |
| Window | Negative destination offset, length or file offset returns Invalid. Destination start must be at most initialized `b.len()` (no uninitialized hole); requested length must fit `b.capacity() - destination_offset`. Reject a declared out-of-window range even at EOF or when length is zero. Validate null shells, signed scalar conversions/file offset, File fd, Buffer metadata, then the destination window, before the zero-length fast path, pointer arithmetic, native I/O or mutation. Every validation failure leaves bytes and all storage metadata unchanged. Integer conversion and range checks never wrap. |
| Successful read | One successful `pread` attempt, retrying EINTR like existing pread; return the actual count, including short reads and zero EOF. Read at most requested length at the absolute file offset without changing the descriptor cursor. Existing bytes before/after the actually written range survive. Initialized length becomes max(old length, destination offset + actual count); zero, EOF and error preserve old length. No implicit padding or zero fill. Use explicit `buffer.filled` when random write positions need a fully initialized window. |
| Empty/error behavior | A fully validated zero-length request returns zero without a syscall. Non-EINTR native failure uses the existing fixed errno table. No retry/fill loop for short counts, fallback, stat preflight or changed source path. Failed I/O publishes no new initialized bytes; any native modifications of already initialized bytes on failure are not rolled back. Native platform count limits may fail; callers explicitly choose bounded chunks. |
| Ownership/lifetime | File and buffer are borrowed for the call. No retained input or result view; scalar Result is Static. The existing entire-buffer generation invalidation applies even to zero/error paths. A live old byte view cannot be used after the call. Move, replacement, early exit and Drop remain their ordinary owner operations. No concurrent shared byte reader may overlap the exclusive mutation. |
| Allocation/provenance | No allocation, resize, reserve, clone, descriptor duplication or release. Capacity, payload address and ownership provenance remain unchanged. A contiguous successfully initialized extension alone raises Vec/Buffer length; partial reads never publish uninitialized spare bytes. Exclusive BufferStorage access refreshes the existing cached writable pointer. |
| Compiler/ABI | New checked `FilePreadInto` HIR/MIR records preserve all five operands in evaluation order. Runtime key `IoFilePreadInto` calls `align_rt_io_file_pread_into(*mut RwFile, *mut Buffer, i64 destination_offset, i64 length, i64 file_offset) -> i64`: nonnegative count, negative mapped error. Exact shell/payload validity, exclusive access and disjoint shell ownership are unsafe preconditions. Null shells, negative File fd, and invalid Buffer metadata return Invalid. Accepted Buffer metadata is `data.len() == b.len`, `b.len <= b.cap <= data.capacity()`, with `b.len` and `b.cap` representable as i64/isize. Nonnegative descriptor values retain native errno mapping without a descriptor-mode/stat preflight. Invalid Vec allocations, misaligned or dangling non-null shells/payloads remain unsafe precondition violations, not recoverable inputs. LLVM only lowers MIR and decodes the existing count/error convention. HostState, ArgMem Unstated, params [], escapes [0,1], release None, no fresh/divergence; no optimistic native attributes. |
| Artifact/platform | No new type layout, persisted public wire format, interface schema, CLI input or ambient configuration. Existing nominal imported identities, source-template rechecking, structural MIR hashing and compiler namespace cover the record. Linux x86_64/ARM64 and macOS are supported using native pread. Both whole-program and per-unit execution qualify imported helpers. |

## Implementation closure matrix

| Axis | Implementation and regression owner |
| --- | --- |
| Formation and malformed records | Sema checks file/buffer places, exact i64 arguments, arity and result domain; checked HIR independently authenticates result, receiver, writable buffer, all scalar operands and local identities before any lowerer. Forged-HIR owner rejects each metadata class in all four entrypoints; MIR validation independently checks operands. |
| Construction and allocation parity | Native parameterized initialized-prefix/window owner covers reserve-only and filled buffers, contiguous extension, overlap overwrite, short/EOF/zero, exact-fit and out-of-range requests. Assert unchanged payload address/capacity and exact initialized byte sequence after each operation. Pointer stability is an explicit resource promise, owned by this native invariant test rather than timing. |
| Move-in/out, source nulling, Drop, replacement and return | Existing borrowed-call machinery does not consume either owner. Native repeated call/free cycles and driver helper/return/replacement paths prove ordinary owner availability. No new cleanup path. |
| Loan and argument ordering | Sweep every FilePread analysis sibling, including source-visible mutation and finalization. Driver negatives cover later use of old bytes, mutation through shared buffer, unbound/temporary/wrong receivers and a later argument moving/replacing the already evaluated buffer/file. Errors must not become a typed valid producer. |
| Control and purity | Parameterized whole/per-unit programs cover `if`, `match`, `else`, `?`, `map_err`, branch/loop joins and early exit; calls are Impure. Integer scalar arguments may use imported generic helpers without losing concrete rechecking. |
| ABI and native malformed inputs | Exact key/cardinality/shape/effect inventories and native typed function assertion; null shells, negative/overflowing scalars, negative File fd, Vec/Buffer length mismatch, initialized length above published capacity and published capacity above allocation reject before the zero fast path, pointer arithmetic or syscall. The named native window owner crosses every malformed metadata class with zero and nonzero length. Zero length has no syscall. Isolated native error/short-count owner and independently forged HIR/MIR cover producer-to-consumer trust. |
| Acceptance | Two disjoint regions of one initialized allocation read back exactly; bounded read smaller than capacity never overreads the requested region; append-style reads start from an empty reserved window without publishing gaps. Local Linux and macOS native owners plus imported driver modes precede push. No throughput claim or benchmark gate. |
| Deferred | Reset/truncate, an in-memory offset write, unrestricted uninitialized holes, atomic snapshots, automatic all-count read loops and align-llm adoption. The direct bounded positional read closes Request 38's provider requirement without making those additional APIs prerequisites. |

Author consistency pass: the ledger is the authoritative record. Propagate its
signature, validation, initialized-length, allocation and loan rules through
draft.md, docs/language-spec.md, docs/design-notes.md, Settled open-questions,
fs English/ja design, checked-HIR ledger, runtime-ABI ledger, roadmap and HANDOFF
at the capability boundary. Extract each exact/range/unchanged promise into the
parameterized owner checklist before coding. Sweep the new IR operands through
all sibling analyses and cache/print/validation consumers, not only dispatch.

Keep one useful end-to-end capability: dormant HIR, ABI and native producer
seams have no stable consumer independently. If mechanical sweeps and exact
owners exceed roughly 1,000 hand-written changed lines, retaining one boundary
avoids repeated trust proofs and merge/integration risk across those seams.

## Named acceptance owners

| Contract axis | Exact planned owner |
| --- | --- |
| Native window/content/capacity/address matrix and native invalid scalars/shells | `align_runtime::file_pread_into_window_preserves_storage_and_initialized_prefix` |
| EINTR, short/error counts, no zero-length syscall and no implicit fill loop | `align_runtime::file_pread_into_native_attempt_transitions` |
| Whole/per-unit imported helpers, control, result, empty-reserved/filled destination and exact two-region bytes | `buffer_pread_into::bounded_positional_reads_in_imported_helpers` |
| Loan invalidation, mutability/receiver/arity/types and argument-order invalidation | `buffer_pread_into::bounded_positional_read_formation_and_loans` |
| All four checked-HIR entrypoints with independently forged result/file/buffer/place/each scalar operand | `file_pread_into_hir_rejects_forged_native_types` |
| Independent MIR typed producer gate before LLVM | `file_pread_into_mir_gate_requires_exact_native_operands` |
| Exhaustive sibling sweeps, key/effect/declaration/export inventories | Existing HIR variant tripwires, `storage_generation_expr_variant_sweep`, runtime ABI/key inventory owners and author extracted FilePread sibling inventory |

## Independent plan review closure

One fresh inspection-only review found one P2: the native malformed-metadata
promise had not enumerated its accepted relationships. The ledger now names
Vec/Buffer length equality, initialized/window/allocation ordering, representable
extents, negative File fd rejection, native error mapping for nonnegative fds,
and validation before zero-length return. The native malformed-input owner
crosses each detectable invariant with zero/nonzero requests; invalid pointers
and allocations remain unsafe preconditions. No capability strategy changed.

## Author source-order closure

The direct later-buffer-move owner exposed missing completed native-owner
reservations. File operations now register their already evaluated File and
output Buffer places through the existing owning-handle snapshot machinery and
validate them before the enclosing action and retire their completed snapshots
at its scalar boundary. Independent old views and other eager operands retain
their own reservations. The same root class is closed for
existing pread/pwrite, with parameterized later-operand moves and replacement
controls. This preserves the existing source-order/liveness contract and does
not change the borrow strategy, lifetime precision or K1 boundary.

## Independent code review closure

One fresh full-diff review found one P2: native mutable-local validation trusted
`Local.is_mut` without independently authenticating its parameter mode. A
coherent forged Borrow-mode record with a mutable local flag and a shared-mode
retention summary passed the checked-HIR body boundary. The shared source
predicate now requires ByValue, BorrowMut or an existing Out slice authority,
covering every native output sibling instead of special-casing this method.
`native_output_hir_rejects_shared_mode_with_mutable_local_flag` retains the
mutable flag and checks pread_into, pread, reader read/read_line and buffer
append/append_filled in all four lowering entrypoints. Its pre-fix witness fails;
the corrected authority also retains the existing owning, exclusive and Out
positive paths. No public contract, IR shape or ownership strategy changed.
