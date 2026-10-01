# Read-only random-access files (Request 21)

## Capability boundary

Request 21 supplies the recorded M12 A4 deferred-with-trigger capability. The
external register records bounded GGUF/transcript/KV reads whose writable-copy
workaround fails on read-only mounts; the non-mmap random-read trigger has fired.
This is Category A deferred work, not a reopening of a locked language rule.
One complete constructor-to-native capability ships in one PR. Existing file
representation, methods, ownership and cursor-free access remain authoritative.
K1 and consumer adoption are outside this work. No benchmark is required: this
capability makes no new performance or resource ceiling promise.

## Public-contract ledger

| Surface | Exact contract | Ownership, effects, allocation and acceptance |
| --- | --- | --- |
| `fs.open_ro(path: str) -> Result<file, Error>` | Requires `import std.fs` and exactly one argument; implicit shared borrow of owned string. Ordinary path resolution, including symlinks and relative paths. Opens an existing object with O_RDONLY and O_CLOEXEC, without create/truncate/extend; no new regular-file admission rule. UTF-8 and embedded-NUL validation precede filesystem work. Empty path keeps OS mapping. NotFound for missing, Denied for EACCES/EPERM, Invalid for EINVAL or invalid text; other native errors use the existing Code(errno) table. No defaults or ambient application configuration. | Impure, same Move file owning exactly one fd; no path retention, no input-byte copy beyond the native ephemeral NUL path conversion, one native handle-shell allocation. No buffer allocation or mmap. Caller supplies the existing buffer read window. Drop closes once. Existing single-threaded, no aggregate, bound receiver rules remain. Driver read-only permissions/error/window owner and runtime path/flag owner. |
| `f.pread(b: mut buffer, off: i64) -> Result<i64, Error>` / `f.len() -> Result<i64, Error>` | Existing actual count, short reads and zero EOF; negative offset abort; live descriptor metadata. Read-only and read/write handles select the same implementations. | Existing handle borrow, mutable bare-local buffer requirement and allocation rules unchanged. Byte parity and live-length driver/runtime owner. |
| `f.pwrite(data: bytes, off: i64) -> Result<i64, Error>` on a read-only file | Runtime Error.Denied, including empty data. Null native handle guard precedes negative-offset abort; negative offset precedes access-mode check. Before reading source bytes or attempting a write, query F_GETFL (retry EINTR); O_ACCMODE == O_RDONLY returns Denied. Query failures use the fixed native error table. Read/write handles then take the existing full-write/empty-data path. No static constructor-origin tracking or stored capability flag. | Same file type and layout; no new ownership/cursor/global state. One descriptor flag query per pwrite, no allocation. Read-only denial, empty denial and byte/length immutability owners; existing read/write owners retained. |
| Artifact and interface identity | New HIR/MIR operation participates in existing structural graph hashing and compiler-build identity. Imported generic templates use the existing source/reparse authority. No new format/version, nominal type or fingerprint policy. | Whole-program and per-unit imported-helper owners. Existing programs retain source semantics; recompilation uses the normal compiler identity. |

## Native and compiler ledger

| Boundary | Exact record |
| --- | --- |
| HIR | `FileOpenRo { path: Box<Expr> }`, exact path Str and Result(File, canonical Error enum); Impure. Every exhaustive effect, move, escape, storage-generation, finalization, depth and replay visitor walks the path or applies the existing file-constructor policy. |
| MIR | `FileOpenRo { path: Operand, out: Slot }`, exact i32 status and File output slot. Shared lower_open_handle evaluates path once, keeps temporary borrowed owner through the native call, releases it after the call, and uses the existing success-only handle load / canonical Error decoder. No path view escapes in the result. |
| Producer gate | The three file constructors share one native-owner contract: exact Str input, i32 result, private typed File output. Malformed types, missing/reused output slots and aliased producers reject before LLVM. Native output/input inventory and loop-fact opacity include the new sibling. |
| LLVM | Existing file dispatcher and fallible native declaration lookup, A08. Shared open-handle emission uses fallible slot access and text extraction; malformed MIR never indexes a missing slot or panics on a non-text operand. |
| Key/symbol/signature | `IoFileOpenRo`, `align_rt_io_file_open_ro`: A08 `i32(ptr path, i64 byte_len, ptr out_file)`. Output must be aligned writable pointer storage, disjoint from the live readable input span; raw handle provenance and exactly-once release use the existing File authority. |
| Native validation order | Null output returns Invalid before dereference. Valid output is initialized null; reject negative/unrepresentable/above-isize extent or positive-null path before any raw slice. UTF-8 then embedded-NUL validation precede open. Empty length needs no input dereference and retains OS mapping. Success alone publishes an owned handle. No filesystem call on invalid text; acquisition failure leaves output null. OOM follows the existing abort policy. |
| Runtime effects | HostState, ArgMem Unstated, same conservative uncurated params/escapes [0,2] as IoFileOpen; no new attribute strategy. New keyed count 450, base rows 468, alloc-count 475, par-map-probe 472, task-group-probe 468, maximum 479. Existing exact signature, export and effect owners cover all rows. |
| Overlap/global state | File operations remain structurally single-threaded. Constructor and pwrite change no process-global or connection-global native state, so no restoration or overlapping-operation protocol is introduced. |

## Implementation closure matrix

| Axis | Closure and owner |
| --- | --- |
| Formation / malformed input | Driver arity/import/path/Result diagnostics; forged HIR path/result/error/effect owner. Parameterized MIR file-constructor status/path/output/private-slot owner. |
| Construction / native ABI | Runtime path validation, output-null-on-failure, O_RDONLY and CLOEXEC owners; ABI golden, registry/effect invariants and native export script. |
| Move-in/out / source nulling / return | Existing File Move authority; imported helper returns Result<File, Error>, by-value helper consumes once, use-after-move rejected. No borrowed path retained. |
| Drop / replacement | Direct runtime close proof and existing cycle owner extended for open_ro; driver branch return, reassignment and error propagation use existing File Drop. Temporary path owners release after native call. |
| If / match / joins / else / try / map_err | Parameterized driver receiver/control cases exercise fresh/bound text inputs, successful handles and missing-path errors; canonical Error discrimination. Existing File join restriction is retained rather than widened. |
| Loops / early exits | Loop-produced text path, loop-local handle cleanup and eager terminating path; native call only after completed path formation. |
| Generics / interface / whole / per-unit | Imported generic constructor wrapper and File return/consume helper in both compilation modes; no raw HIR serialization change. |
| Provenance / allocation parity | Existing fd-owner shell and buffer implementation reused, no new memory family. Native private output slot and call-scoped path; bound path remains usable and fresh path dropped once. |
| Permission behavior / read parity | A mode-0444 input opens read-only and rejects open_rw on the same non-root test host; mode-000 input Denied; pwrite nonempty/empty Denied without content/length change; missing NotFound, short EOF, live len, writable read parity. Native flag assertion remains platform-independent evidence of no write access. |
| Explicit deferrals | No stat surface, new File type/flag, cursor, seek, buffering, copy_range, generic handle widening, consumer code or K1. |

## Author consistency pass

The public and native ledgers agree on byte-counted UTF-8, call-scoped lifetime,
null output on failure, canonical Error identities and exact signed ABI widths.
No persisted/wire format, reflection table, cache owner or milestone prerequisite
is added. Validation precedes filesystem side effects. Read-only pwrite has one
explicit order and uses kernel-owned descriptor mode, including zero-byte
operations, without constructor-origin inference. Required normative updates
are draft, language-spec, design-notes, Settled/open-questions, M12 roadmap,
std-design/fs and ja mirror, checked-HIR ledger and runtime ABI ledger. Existing
normative signatures serve as declarations; syntax is exercised by owner programs.
The final matrix-to-diff pass will bind every applicable row to implementation
and discriminating regression evidence before independent code review.

## Pre-implementation review closure

One fresh independent adversarial inspection of this ledger and the adjacent
runtime, sema, checked HIR, MIR producer, shared lowering and LLVM paths was
CLEAN. It confirmed descriptor-mode error/empty-data ordering, call-scoped path
cleanup, the three-constructor producer closure and conservative A08 metadata.
No additional strategy or ownership representation is introduced.

## Exact owner binding

- `m12_file_io::open_ro_permissions_windows_and_write_denial`: mode-0444/read-write
  refusal, mode-000 denial, missing/NUL errors, window/short/EOF reads and empty/full
  pwrite denial without mutation. `negative_offset_aborts` covers read/write and
  read-only pread, nonempty pwrite and empty pwrite.
- `m12_file_io::open_ro_control_and_imported_generic_parity`: helper return/move,
  shared/fresh/if/match/loop/arena/task-group paths, reassignment, else/try/map_err,
  early termination and writable read parity, whole-program and per-unit.
- `m12_file_io::open_ro_formation_and_move_diagnostics`: arity/import/path/result,
  temporary receiver, use-after-move, print and parallel capture restrictions.
- `owned_temporaries::open_handle_path_owner_releases_after_native_read`: all five
  path-only reader/writer/file constructor consumers release fresh path owners
  after native use and before Result formation; existing temporary suite retained.
- `file_open_ro_path_flags_read_write_and_drop_contract` and
  `file_open_ro_invalid_paths_leave_null_before_filesystem_work`: O_RDONLY/CLOEXEC,
  short/EOF, live fstat, denial, exact path extents/text and null failure output;
  existing file roundtrip/update/cycle/CLOEXEC owners retained and cycle owner
  extended to read-only acquisition/Drop.
- `file_constructors_hir_reject_forged_types_in_every_entrypoint` and
  `file_constructors_mir_gate_requires_exact_native_slots`: three constructors,
  exact input/result/native widths and private output identity.
- Existing `runtime_abi` declaration/effect/type matrix, exact runtime-key owner,
  sema/HIR cardinality tripwires and `scripts/test-runtime-abi-exports.sh` close
  the complete new keyed/export inventory.

## Author matrix-to-diff pass

Every applicable cell above is implemented by the existing File representation,
new exhaustive constructor sibling, shared typed native producer contract and
call-scoped path cleanup. The concrete owners in Exact owner binding close those
cells. The cleanup witness was mutation-verified: removing the shared after-call
cleanup fails the five-constructor owner; restoring it passes. The original 14
owned-temporary tests also passed. No new allocation/resource performance promise
requires a benchmark. The mechanical added-prose obligation extraction is retained
with the worktree Git logs; source English and Japanese contracts agree. Existing
M12 original-history wording remains marked historical, with the fired trigger and
current surface recorded separately. K1 and external adoption remain deferred.

## Code review finding closure

The fresh full-diff review found one P2 fixture-ownership class. Runtime flags,
the expanded native fd-cycle owner, driver permissions/control and the expanded
negative-offset owner now acquire mode-0700 directories with exclusive mkdir
and arm RAII cleanup immediately. They never remove or follow an existing entry.
Both native and driver collision/symlink negative controls prove pre-existing
markers survive failed acquisition. New native handle guards also close acquired
fds on assertion unwind. Shared legacy harness machinery remains outside this
local fix. The production contract, IR and safety strategy are unchanged; the
fix closes against these owner targets without a second full-diff review.
