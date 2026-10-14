# Read-only regular files for positional I/O (media AM-R03)

Media loads Safetensors through bounded positional windows. Existing
fs.open_regular admits one descriptor as reader; its second open_ro acquires
a different descriptor for positional reads. The request is valid: a kind check
on the first descriptor cannot certify the second. Add one explicit constructor
of the existing File owner, reusing the existing regular-admission state machine.
AM-R07 external writable views remains a separate capability. No benchmark or
application-throughput promise is made.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Source | `fs.open_ro_regular(path: str) -> Result<file, Error>`, requires import std.fs and exactly one positional argument. Existing implicit shared borrow of owned string; no default, generic argument, optional flag or ambient application configuration. Impure; ordinary current-directory/absolute resolution and symlinks, as existing constructors. Linux and macOS are the supported hosts. |
| Admission | One O_RDONLY\|O_NONBLOCK\|O_CLOEXEC open; fstat that descriptor must report regular; F_GETFL then F_SETFL clear O_NONBLOCK before publication. Nonregular is Invalid, including directories/devices/FIFOs; no FIFO writer is needed. No pathname pre-stat, reopen, create, truncate, data write or mmap. A pathname replacement may select either object at open, but only the selected regular descriptor is returned. No sandbox, immutable-content or remote-filesystem interruptibility promise. |
| Errors/order | Source import/arity/expected result and path typing retain existing file-constructor rules. Native ABI numerical/output/input checks below precede any read/write; then UTF-8 and embedded NUL; then open/stat/getflags/setflags in that order. Empty path uses native mapping. Each descriptor operation retries only EINTR. Missing maps NotFound, EACCES/EPERM Denied, invalid text/nonregular Invalid, other failures use the existing fixed errno table. The primary error wins; fd Drop performs one close, never retries, ignores close errors. |
| Ownership/lifetime/allocation | Path is borrowed for the completed call only. One transient NUL path copy and one existing File shell allocation; no input retention, reader buffer, descriptor duplication, hidden content copy or new owner type. Private acquired fd is immediately RAII-owned; all failures close it once and leave no File. Only success transfers it to the existing Move File. Heap allocation failure retains terminal policy. Existing File return/consumption, replacement, Drop, receiver and thread restrictions apply. |
| File methods | Existing pread/pread_into/len/sync/pwrite contracts are unchanged. The same admitted descriptor supplies live metadata and positional reads; short counts and EOF remain ordinary results. Read-only pwrite remains Denied, including empty writes, according to the existing negative-offset/mode-check order. Buffer initialization/bounds and no-allocation read rules remain those of each existing method. |
| HIR/MIR | Extend existing FileOpenRo with `regular_only: bool`, false for open_ro and true for open_ro_regular. Preserve exact Str child, Result(File, canonical Error), effect, eager termination and path-owner cleanup. MIR retains exact i32 status, private File output slot, source Str operand and the Boolean. Lowering/replay/print/hash/producer consumers preserve it. Changing bool selects another valid constructor, not malformed input. No new IR variant or representation. |
| Runtime | New key IoFileOpenRoRegular / symbol align_rt_io_file_open_ro_regular, existing A08 `i32(ptr path, i64 byte_len, ptr out_file)`. Rust signature `unsafe extern "C" fn(*const u8, i64, *mut *mut RwFile) -> i32`. Existing File Drop accepts the result. HostState / ArgMem Unstated, conservative uncurated params/escapes [0,2], matching IoFileOpenRo. No signature/tag/layout ambiguity or global native state. |
| ABI validation | Caller supplies live immutable input bytes and one live writable output pointer slot. Check nonnull/aligned output and address+pointer-size, signed length and isize bound including NUL scratch, positive-null input and input address+length, then input/output disjointness before dereference or output write. Invalid numerical/overlap input returns Invalid and leaves output untouched. Then clear output to null, validate full UTF-8 then embedded NUL, then acquire. Zero input permits null. Success alone writes a nonnull File. Pointer-range checks do not authenticate arbitrary native addresses. |
| Artifacts | Existing File identity/layout and interface format stay unchanged. Private HIR/MIR body shape and native row are bound by compiler identity; generic source transport reparses the explicit constructor. No user wire/persisted format, canonical value encoding, reflection table, configuration file or later milestone is introduced. Runtime counts become 467 keyed, 485 base, 492 alloc-count, 489 par-map and 496 maximum exports; exact signature/effect/export owners qualify them. |
| Sources/acceptance | This ledger, draft.md, language-spec, design-notes, Settled/open-questions, std-design/fs English/Japanese, HIR ledger19 and runtime ABI ledger20. Plan58 owns shared admission; Plan84 owns File behavior. Native and source owners below; consumer source/pin/adoption and real media verification stay external. |

The unpublished descriptor exclusively owns its flags until restoration and
publication. There is no process/connection-global state and no second operation
can overlap it. Numerical ABI rejection precedes output zeroing; all subsequent
failures publish null. These rules apply equally to the existing reader publisher
when its common helper is extracted. Existing ordinary reader/open_ro policies
are unchanged; no compatibility alias or alternative owner model is added.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Source formation and HIR integrity | Same file constructor checker admits both names, stores exact discriminator, and validates all children. Parameterize existing file_constructors_hir malformed path/result/error owners and source arity/import/wrong-input/effect checks. Both bool values replay/lower without confusing policy. |
| MIR and ABI | Exact status/path/private File output certification shared by existing constructors. Extend file_constructors_mir mutations; compare emitted runtime symbol for both bool values. New key/declaration/effect/export count and compile-time native signature witnesses. |
| Native acquisition | Factor fs_regular's numerical/text/acquire path into one typed private publication helper for Reader and File; preserve acquire/syscall behavior. Parameterize regular_reader native matrix and ABI preflight over both publishers: readonly/empty/relative/dot/symlink/dangling; dir/device/FIFO bounded child; flags; all syscall failure/retry/close cells; forward/inverse pathname replacement; output sentinel/null and multi-invalid inputs. |
| Positional descriptor identity | For File publisher, read original bytes after pathname replacement through existing positional operation and observe unchanged descriptor offset. Check short/EOF, len and read-only pwrite denial; bounds/buffer guarantees reuse existing pread_into owner. Same fd remains usable after errors. |
| Moves/nulling/Drop/replacement | Driver constructor returns existing File through direct/imported helpers; owned path expires, reassignment and early error close exactly once. Reuse native fd-close witnesses and existing File move/use-after-move restrictions. No runtime File representation or Drop change. |
| Control flow | Extend existing file constructor/owned-temporary source owners for direct/fresh/if/match/loop/arena path inputs and eager exits, ?, else, map_err, receiver reuse, read success/error and skipped native invocation. Existing File join restrictions remain. |
| Generic/interface/native parity | Imported generic path helper and returned File, whole/per-unit/cache replay. Temporary compile/link/run fixtures remain exclusively owned and bounded. Same explicit constructor reaches the same native key; no consumer build modifications. |
| Platform and gate | Add driver cases to client_admission_composition, parameterize regular_reader-prefixed native owners so the existing scripts/test-client-admission.sh and Linux/macOS CI select both without a new job. Run the script locally plus ABI exports and focused HIR/MIR owners before push; CI is the final guard. |
| Allocation/performance | Native shell and path scratch only, no new payload allocation or read buffer. Existing buffer window allocation/storage owners remain authoritative. FIFO rejection is bounded correctness, not a speed benchmark. |

## Author consistency pass

Every new public promise above binds to exact source types, descriptor operations,
error/output precedence, ownership, native widths and required owners. The Boolean
has two complete valid meanings; it does not carry an unauthenticated source-name
claim. All other File operations use kernel mode and the owned descriptor as before.
No earlier milestone consumes a future capability. Normative prose mirrors this ledger. The initial independent adversarial review
was CLEAN before implementation, covering Plans58/84, native admission/publication,
compiler identity and all proposed acceptance owners. The final author closure,
self-review, committed-code review and local preflight qualify the implementation.

## Author closure

The only native policy path is fs_regular::acquire; open_with validates ranges and
text once, then selects a typed Reader or File publication. The new File ABI uses
A08 with a compile-time Rust signature witness and one registry/effect/golden row.
The FileOpenRo discriminator is preserved through source formation, replay,
serde-backed body identity, MIR printing/lowering and exact native-key selection.
It adds no variant, type representation, drop path or alternate permission state.

The extracted ledger obligations (exact types and output slot, every native
operation/cleanup edge, validation before access, and nonregular rejection) map
to file_constructors_hir, file_constructors_mir_gate, regular_reader_abi_preflight
and regular_reader_native_matrix. Both publishers run the same native matrix;
File additionally checks original-byte pread/pread_into after pathname replacement,
short/EOF preservation, live len, read-only denial including empty writes and
unchanged cursor. Native close witnesses and the existing File Drop implementation
cover fd cleanup. The driver regular_file_positional_lifecycle_and_replay owner
executes returned/replaced/discarded Files with fresh, if/match/loop/arena owned
path arguments, early exit, ?, else and map_err in whole/per-unit and cold/hit modes.
The open_handle_path_owner owner fixes path Drop after native access; source
negatives pin import/arity/type, impurity and use after move. No performance claim.

Sibling usage inspection finds media Safetensors reopening after reader admission;
llm gguf and voice util/assets use ordinary read-only positional windows. The
ordinary constructors and all existing read-window semantics remain unchanged.
Consumer source, compiler pin, real-model execution and adoption remain external.
