# Reuse the buffered reader's copy window

Status: **IMPLEMENTED 2026-10-05 — native and language owners qualify Linux/macOS.**

This selects a bounded refinement of plan12 §3.6 and §7.1. A buffered reader
already reserves the same 64KiB window that the portable io.copy pump separately
allocates. Reuse that existing storage rather than add a second transfer
allocation. Preserve the portable read/write sequence. Kernel transfer dispatch,
larger native stack frames and thread-local buffer pools are not selected.

## Exact capability ledger

| Surface | Contract |
| --- | --- |
| Public signature and inputs | Existing `io.copy(r: reader, w: writer) -> Result<i64, Error>`, borrowing both owners. No argument, default, option, environment input or language/library operation changes. |
| Validation and selection | Null reader or writer returns the existing Invalid status before inspection/allocation/I/O. Reuse only a buffered reader whose existing lookahead capacity is at least READ_LINE_CHUNK. All other readers retain the existing fallible fixed scratch allocation. Pin READ_LINE_CHUNK == BUF_WRITER_CAP at compile time. |
| Window formation | Drain pending lookahead first, at most 64KiB per write. When empty, use the existing Reader::refill, which reads at most the same 64KiB and retries EINTR with the same read-error mapping. Expose only its initialized returned prefix. Never reserve or resize eligible lookahead storage during copy. |
| Consumption and lifetime | Advance the reader's logical cursor before attempting the corresponding write, as the reference reader_read does. Borrow the window only until the existing writer_write call returns; it retains no input. Refill only after that borrow ends. Reader/writer ownership, fd closing, later use, Drop and write buffering remain unchanged. |
| Errors and result | Preserve lookahead-before-fd, read-before-write and first-error precedence, negative native status encoding, EOF, partial-write/EINTR handling, buffered output flush policy and saturating byte count. Eligible readers make no new scratch reservation, removing that reservation's allocation-refusal cause. Unbuffered reservation failure keeps Invalid before I/O. No writer poison or new post-error rule. |
| Allocation and resource claim | Eligible copies add zero transfer allocations and retain the reader's existing payload capacity/address. They remove one separate 64KiB requested scratch payload. Reader construction/lookahead, writer buffers, native provider activity and fixture allocations are outside that claim. Ineligible copies still acquire one fixed transfer payload. No added large stack buffer, general zero-copy, RSS, compiler-speed or throughput claim. |
| Owners and identity | align_runtime owns private window selection and the existing byte pump. No MIR/LLVM/native ABI symbol/signature, resource nominal, interface field, cache-key component or persisted format changes. Existing compiler/runtime producer identities apply. |
| Prerequisites and acceptance | Buffered reader, fixed portable copy and writer_write are shipped. Existing m12_read_line lookahead/copy and m9_io copy owners remain execution oracles. Native parameterized window/error/cursor owners and feature-enabled allocation counters qualify Linux/macOS. A local resource probe records actual baseline/candidate allocations, capacity and pointer observations; timing is not an acceptance gate. |
| Source agreement | This internal ledger; plan12 §3.6 shared-read authority and §7.1 implementation disposition; HANDOFF at capability completion. No new public contract requires specification or language-mirror edits. |

## Implementation closure matrix

| Axis | Closure owner |
| --- | --- |
| Construction and selection | `buffered_io_copy_windows_and_cursors` crosses unbuffered, initialized buffered, empty reserved lookahead and a smaller private lookahead capacity; existing null controls precede selection. |
| Move-in/out, return, replacement and Drop | No new native or language owner. Existing borrowed copy/nonconsumption and reader/writer Drop owners; the private byte window cannot escape the synchronous writer call. |
| Initialized bytes and pointer provenance | `buffered_io_copy_windows_and_cursors` crosses empty, 1B, 64KiB boundaries, multiple windows, binary NUL/non-UTF-8 and prior line lookahead. Observe the exact existing reader payload pointer and stable capacity; uninitialized tails never form a byte slice. |
| Error and cursor state | `buffered_io_copy_error_cursor_parity`: read refusal, simultaneous read/write refusal, write refusal and prior lookahead followed by write refusal. Cross an empty buffered writer at exactly 64KiB and 64KiB plus a tail, and a prefilled writer whose overflow flush fails. Compare native status, logical/physical reader cursor, next read and writer buffered/post-error state against the portable pump. Exactly 64KiB is retained without I/O; overflow consumes the next reader chunk before flush, and failed flush clears the previous accumulator. Existing partial-write/EINTR writer authority and Reader::refill are reused. |
| if/match/else/?/map_err, branch/loop joins and early exit | Language HIR/MIR and ownership paths do not change. Existing driver execution owners exercise Result propagation and subsequent handle use. Native EOF/error exits end all window borrows before cleanup. |
| Whole/per-unit, generic, interface and ABI | No changed compiler path, formation rule or interface record. Existing native ABI and driver owners remain applicable; no new runtime key or serialized shape. |
| Allocation parity | `buffered_io_copy_allocation_parity` uses feature-enabled thread-local whole-Rust counters: one scratch acquisition on the ineligible control and zero on eligible copies. The local ignored `buffered_io_copy_storage_probe` records baseline/candidate allocation counts and actual existing lookahead capacity/pointer. Pending lookahead and refill-only inputs are distinct cells. No allocations are counted by expected source shape alone. |
| Test lifecycle and platform | Reuse exclusive FileFixtureDir and immediate reader/writer guards. No new test binary, shared harness or generated-child owner. Run native owner and existing relevant driver targets on macOS and Linux. |

Keep one common writer/error/count loop. Do not duplicate writer policy in the
selected path. A private read-window helper may join scratch and lookahead
producers; its returned slice must borrow their actual storage. The buffer used
by the scratch control remains the existing Buffer owner. No forged Vec over
foreign storage or extending initialized length beyond the returned count.

Author matrix pass: detection uses existing private Reader fields after the null
gate; geometry has a compile-time owner. The slice lifetime couples the selected
reader/scratch borrow to the synchronous write, and all refill/read/write error
exits end that borrow. Whole/per-unit and control-flow cells reuse unchanged
compiler paths. Native fixtures use the existing exclusive directory and handle
guards. The feature-enabled resource control also proves the counter is active.
No other native text/view boundary or connection-global state is introduced.

Independent boundary review found one P2: the error row did not assign buffered
writer accumulator states. The row now owns exact-capacity deferred success,
overflow failure and prefilled-flush failure against the portable reference,
including both owners' subsequent state. No storage/lifetime/ABI finding or
strategy change was required. The finding was resolved before implementation;
the native parity owner now closes every assigned accumulator state.

## Qualification

The three `buffered_io_copy_*` native correctness/allocation owners pass on macOS
ARM64 and Linux ARM64 with feature `alloc-count`. The existing m12_read_line
lookahead/copy owner and eight m9_io copy owners pass on both platforms; the
unrelated large-input RSS measurement is not a gate for this storage claim.

The local ignored `buffered_io_copy_storage_probe` runs the frozen portable pump
and production copy over prepared reader/writer owners. Refill-only and prior-line
lookahead cases both measure one baseline allocation versus zero candidate
allocations. Baseline requested scratch payload is 65536 bytes; candidate scratch
is absent. Each owner's retained lookahead has capacity 65536 and keeps its exact
pointer throughout copy. Addresses across independently constructed fixtures need
not match. Output bytes agree; no timing, allocator-overhead or RSS claim follows.

```text
scripts/cargo.sh test -p align_runtime --lib --features alloc-count buffered_io_copy
scripts/cargo.sh test -p align_runtime --lib --features alloc-count buffered_io_copy_storage_probe -- --ignored --nocapture --test-threads=1
scripts/cargo.sh build -p align_runtime
scripts/cargo.sh test -p align_driver --test m12_read_line io_copy_after_read_line
scripts/cargo.sh test -p align_driver --test m9_io io_copy_ -- --skip io_copy_rss_stays_bounded_on_large_input
```

Evidence is preserved outside the repository in align-buffered-copy-evidence.
The Linux image identity is a7110d573bbd, with Rust 1.96.1 and LLVM 22.1.8.

## Kernel-transfer discriminator

At baseline 36524f2a, an ordinary Align program reads a two-window file whose
first 64KiB contains byte 1 and second contains byte 2, then copies to stdout
bound to a read-only regular-file descriptor. io.copy reports an error; its
subsequent one-byte read observes 2. A direct Linux copy_file_range attempt with
the same read-only destination reports EBADF without advancing its input, so
the subsequent byte is 1. The destination remains unchanged in both cases.

The Linux ARM64 probe uses private temporary ownership and bounded child
execution. It discriminates one error-state difference, not throughput or the
impossibility of every future dispatch. Evidence is preserved outside the
repository in align-copy-transfer-evidence. Simply replacing the portable pump
does not satisfy the existing exact-reference guardrail; kernel transfer remains
deferred. Reusing the already-reserved lookahead retains the reference sequence.
