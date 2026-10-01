# Exclusive random-access file creation (Request 30)

## Capability boundary

Request 30 combines the shipped exclusive final-entry creation contract with
the existing positional File owner. Alignpack and KV publication currently use
an exists-then-create sequence that can truncate a competing creator's file.
One constructor-to-native capability removes that race without changing File's
representation, methods, ownership or cursor-free access. This is a standard
library capability, not a reopening of a locked language rule. K1, durability
and consumer adoption remain outside the boundary. No performance promise or
benchmark is introduced. The expected diff is below 1,000 hand-written lines.

## Public-contract ledger

| Surface | Exact contract | Ownership, effects, allocation and acceptance |
| --- | --- | --- |
| `fs.create_rw_exclusive(path: str) -> Result<file, Error>` | Requires `import std.fs` and exactly one positional argument, implicitly borrowing string. One native open uses O_RDWR\|O_CREAT\|O_EXCL\|O_CLOEXEC\|O_NOFOLLOW with mode 0644 subject to umask. An absent final entry creates one regular file; any occupied regular file, directory, live or dangling symlink, FIFO or device fails with Code(native EEXIST) without opening, truncating, replacing, following or removing it. Parents use ordinary resolution, including symlinks and relative paths; no parent creation, stat/exists preflight, normalization, retry or emulation. Missing parent is NotFound, permissions Denied, invalid inputs Invalid, other native errors Code(errno), through the fixed table. Empty, embedded-NUL and invalid UTF-8 reject before filesystem work. No defaults or new ambient configuration. | Impure, same Move File owning one descriptor and native shell. Path is call-scoped and not retained. One ephemeral NUL path copy plus the existing File shell; no buffer or mmap. Drop closes once and never removes the entry, including partial-write/error paths. Existing bound-receiver, aggregate and single-threaded restrictions remain. Driver exclusive-create owner and native occupied-entry/race/flag owners. |
| Existing File methods | Pread, pwrite, len and negative-offset semantics remain unchanged; new owner is read/write. A full pwrite can extend past EOF and pread returns actual/short/zero EOF counts. | Existing File borrow and buffer requirements, ownership, replacement, Return, try/map_err and Drop paths. Owner writes at planned offsets and reads exact bytes back. |
| Atomicity and environment | The operation claims only atomic final-entry acquisition on the accepted local ext4/tmpfs Linux and APFS macOS floor, consistent with plan 27. Exactly one competing exclusive creator of one absent path wins. No path stability after acquisition, parent confinement, multi-file transaction, automatic rollback, unlink, flush or crash durability. Native errors are returned without weakening flags. Process umask/current directory follow existing filesystem semantics and are never changed by the operation. | No global state change or restoration protocol. Barrier-controlled native competing-creator owner and existing-entry preservation controls. |
| Artifact/interface identity | New HIR/MIR operation participates in normal structural hashing and compiler-build identity; imported generic templates reparse from source. No new type graph, serialization field/version or fingerprint kind. | Imported generic File return/consume helper in whole-program and per-unit compilation. |

## Native and compiler ledger

| Boundary | Exact record |
| --- | --- |
| HIR | `FileCreateRwExclusive { path: Box<Expr> }`, exact Str path and Result(File, canonical Error enum), Impure. Enumerate every sibling File constructor visitor, checked gate and replay/depth path. |
| MIR | `FileCreateRwExclusive { path: Operand, out: Slot }`, exact i32 status and private File output. Shared lower_open_handle evaluates the path once, retains fresh borrowed owner until native call, drops it after use, then success-only loads handle or decodes canonical Error. |
| Producer/LLVM | Four constructors share the exact native-owner status, input and private output contract. Missing/reused outputs and forged path/result types reject before LLVM. Use existing fallible A08 file dispatcher, no new representation or codegen semantics. |
| Key/symbol/signature | `IoFileCreateExclusive`, `align_rt_io_file_create_exclusive`: A08 `i32(ptr path, i64 byte_len, ptr out_file)`. Output is aligned writable pointer storage disjoint from input; positive non-null input is live immutable readable memory. Success transfers exactly one File owner to exactly-once free. |
| Validation order | Null output returns Invalid before any input check; valid output is initialized null. Reject negative/unrepresentable/above-isize/empty extent, null-positive input and impossible len+1 capacity before reading bytes. UTF-8 and embedded-NUL validation precede ephemeral path allocation and the native open. Acquisition failure leaves output null; success alone publishes the Box handle. OOM follows locked abort policy. Move the shared exclusive-path helper's len+1 check before raw-slice construction; all prior exclusive-publication callers keep their public semantics. |
| Native acquisition | One `libc::open` with the exact flags and 0644 mode. Reuse the existing exclusive-path validation authority and errno table. New File shell is the existing fd-only RwFile; fd is never wrapped as writer or exposed on failure. |
| Runtime effects/inventory | HostState, ArgMem Unstated, uncurated params/escapes [0,2], no fresh/release/divergence, matching existing file constructors. Key count 451; base 469, alloc 476, par probe 473, task probe 469, maximum 480. Existing exact declaration/effects/export owners close the complete row inventory. |

## Implementation closure matrix

| Axis | Closure and owner |
| --- | --- |
| Formation/validation/malformed | Extend existing parameterized HIR and MIR four-constructor owners, strict arity/import/path/result/move diagnostics and exhaustive sema tripwires. |
| Construction/native side effects | `file_create_exclusive_occupied_entries_and_flags` and `file_create_exclusive_invalid_paths` preserve regular/directory/live+dangling symlink/FIFO entries and null outputs. `file_create_exclusive_race_has_one_winner` proves exactly one winner, loser Code(EEXIST), and final winner bytes; fd flags prove read/write and CLOEXEC. `file_create_exclusive_mode_respects_child_umask` runs isolated children with umask 000 and 077 and asserts exact modes 0644 and 0600; the parent umask never changes. Immediate child ownership and one bounded execution/cleanup deadline prevent leaked test processes. |
| Move-in/out/source nulling/return | Existing File representation; imported generic constructor returns Result<File>, by-value helper consumes once and repeated use rejects. No retained path root. |
| Drop/replacement/early failure | Native guarded handle acquisition plus bounded create/free cycle; driver reassignment/early-return/try/map_err/missing-parent failure paths use existing cleanup. Shared temporary path owner covers all six path-only constructors after native use, before Result formation. |
| If/match/else/joins/loop/early exits | Extend existing imported/control owner with exclusive constructor and unique targets. If/match/loop-generated paths evaluate once; eager terminating paths perform no open. File join restrictions remain excluded rather than widened. |
| Generics/interface/whole/per-unit | Imported generic exclusive constructor wrapper and File return/consume in both modes; no interface schema change. |
| Provenance/allocation parity | Existing fd owner, no new memory family, private native output, no native file buffer. Reuse ordinary File method and negative-offset owners. |
| Shared helper safety | Existing writer-exclusive/publication runtime owners plus new maximum-length reject-before-dereference witness; no process-global umask/environment modification by tests. |
| Explicit deferrals | No sync/durability, new File type, flags/options API, cursor, seek, buffering, confinement/retained directory, consumer code or K1. |

## Author consistency pass

The requested flags, occupied-entry semantics and Code(EEXIST) identity agree
with plan 27's existing exclusive publication strategy; the new mode is exactly
0644 subject to umask, without changing the writer's existing creation mode.
Path validation rejects empty inputs like create_exclusive, while open_ro's
ordinary empty-path mapping remains unchanged. Native pointer/extent and output
precedence are explicit before side effects. Exact i32/A08 status, File output,
nonretained path and conservative effect metadata agree across the records.
There is no persisted format, cache graph, reflection or milestone prerequisite.
Required sources are draft, language-spec, design-notes, Settled/open-questions,
M12 current roadmap, fs English/ja, checked-HIR and runtime ABI ledgers. Examples
are declarations here; concrete owner programs syntax-check all call forms.
Final matrix-to-diff binding precedes the one independent full-diff code review.

## Pre-implementation review closure

The fresh independent adversarial plan review found one P2 missing creation-mode
proof. The construction row now binds the exact child-scoped 000/077 umask owner
above. A 000 child discriminates accidental reuse of the writer's 0666 mode,
which ambient 022 would conceal; 077 proves normal masking. No parent-process
umask mutation is permitted. The remaining native/public strategy and sibling
constructor sweep had no actionable findings. This local acceptance-cell fix
preserves the reviewed contract and requires no second complete plan review.

## Exact owner binding and author matrix-to-diff pass

- `m12_file_io::create_rw_exclusive_preserves_existing_entries_and_reads_offsets`
  covers full positional writes, a past-EOF hole, short/EOF reads, occupied entry
  preservation and canonical missing/invalid errors in whole and per-unit mode.
- `m12_file_io::create_rw_exclusive_control_and_imported_generic_parity` covers
  imported generic return, by-value consumption, fresh/bound/if/match/loop/arena/
  task-group paths, replacement, else/try/map_err and eager early termination.
  The partial-output error owner confirms no implicit removal.
- `m12_file_io::open_ro_formation_and_move_diagnostics` now parameterizes the new
  constructor as well: exact arity, import, path/Result type, bound receiver,
  use-after-move, printing and Pure parallel/capture rejection in both modes.
- `owned_temporaries::open_handle_path_owner_releases_after_native_read` sweeps
  all six path-only constructors, retaining the path until native use and dropping
  it before Result formation. The existing 14 other temporary owners remain green.
- The four native `file_create_exclusive_*` owners close every occupied final
  kind, device rejection, exact output/extent/text order, flags, one-winner race
  with winner-byte identity, and child-scoped 000/077 mode masking. Native
  `file_no_fd_leak_across_cycles` now includes 128 exclusive acquisition/Drop cycles.
  macOS APFS owners and Linux tmpfs owners provide the stated local floor evidence.
- `file_constructors_hir_reject_forged_types_in_every_entrypoint` and
  `file_constructors_mir_gate_requires_exact_native_slots` parameterize all four
  File constructors and five malformed forms, retaining every checked entrypoint
  and exact private native output validation. Existing exhaustive HIR policies,
  key/ABI registry owners and the five-case native export script close the inventory.

Every applicable matrix row points to the explicit sibling implementation, the
shared File constructor producer/cleanup authority or the unchanged File method
and Drop implementation with the owners above. No interface representation,
type identity, memory family or performance/resource promise changes. The added
normative-prose extraction and Gate 1–6 author checks are retained outside Git.
The creation-mode and one-winner owners were mutation-verified: 0666 and missing
O_EXCL each fail their named owner; restored code passes. The public contract,
English/Japanese mirrors and exact 451/469/476/473/469/480 inventory agree.
