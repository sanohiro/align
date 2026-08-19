# `std.fs` exclusive creation and no-replace publication

Status: **PROPOSED DESIGN; implementation not authorized.**

This document is the public-contract ledger and implementation plan for
align-llm Request 14. It adds the two filesystem operations needed by C6f2 to
publish a result/evidence pair without truncating, replacing, or deleting a
competing artifact:

```text
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
```

The names are part of the proposed contract. A rename is permitted only when
the final destination entry is absent; a create is permitted only when the
final entry is absent. Neither operation uses a check-then-operation sequence,
an ordinary replacing rename, a shell command, or a hidden cleanup helper.

The implementation starts only after this design has received one fresh,
independent adversarial review, the review findings have been closed in this
ledger, and the design commit has merged. The implementation is one strict
producer-to-consumer capability: semantic formation, checked HIR, MIR,
runtime ABI, native operation, ownership cleanup, platform owners, and the
align-llm adoption fixture must land against this contract. A dormant native
operation without its race and cleanup proof is not a useful boundary.

## 1. Public-contract ledger

| Surface | Exact inputs and defaults | Result and errors | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner | Artifact/cache identity and prerequisite acceptance |
| --- | --- | --- | --- | --- | --- |
| `fs.create_exclusive(path: str)` | Exactly one positional `str`; no option object, default, environment input, current-directory override, or retained path. The view is consumed only during the call. An empty path, a path with an interior NUL, an invalid ABI length/pointer, or a non-UTF-8 ABI view is rejected before the native open. Relative paths use the process current directory exactly as the existing `std.fs` paths do; no normalization, `realpath`, parent walk, or intermediate-symlink rejection is added. | `Ok(writer)` when one new regular file entry is atomically created and opened for writing. `Err(Error.Code(native EEXIST))` when any final entry already exists, including a regular file, directory, symlink, FIFO, or device; the existing entry is not opened, truncated, replaced, or removed. `ENOENT`, `EACCES`/`EPERM`, `EINVAL`, `ENAMETOOLONG`, `ENOSPC`, `EMFILE`, and every other native failure use the one existing errno table. No failure returns a writer. | The result carries the existing owned `writer` Move type. The writer owns one file descriptor and the existing 64 KiB buffered writer allocation; its path view is not retained. `write`, `flush`, `?`, `map_err`, branch joins, early returns, and Drop use the existing writer ownership and close path. Drop flushes best effort and closes the descriptor; it does not remove the file. A write or flush failure may leave a partial regular file, which remains visible for explicit `fs.remove`. No implicit rollback or delete is performed. | `align_sema` forms a distinct `CreateExclusive` HIR expression and returns `Result<writer, Error>`. Checked-HIR validation, replay, generic rechecking, MIR, LLVM lowering, and ownership/drop analysis treat it as a writer-producing filesystem operation, not as truncating `fs.create`. The runtime adds one dedicated constructor entry point using exclusive native-open flags. | The new builtin name and runtime key participate in the compiler/runtime capability and ABI inventories. No new language type, interface field, interface format, or cache identity kind is introduced; the existing writer nominal identity remains authoritative. The owner is the `m9_fs` filesystem target plus runtime FFI/Drop tests and the align-llm `c6f2-request14-adoption` fixture. |
| `fs.rename_no_replace(source: str, destination: str)` | Exactly two positional `str` views; no defaults or ambient configuration. Validate `source` completely, then `destination`, before invoking the native operation. Both are borrowed for the call only. Empty, embedded-NUL, invalid ABI length/pointer, and non-UTF-8 ABI inputs fail before any directory mutation. Paths are passed as written: relative paths resolve against the current directory, intermediate symlinks follow normal OS pathname resolution, and the final source/destination components are not rewritten. | `Ok(())` only when one native no-replace rename atomically moves the source directory entry to an absent destination. If the destination exists, including as a regular file, directory, symlink, FIFO, or device, return `Err(Error.Code(native EEXIST))`; neither entry changes. If the source is absent, return `NotFound`; for cross-filesystem, unsupported-volume, parent, permission, length, or other failures use the fixed errno mapping and do not emulate the operation. The operation never replaces or deletes a destination. | Both path views are borrowed and no path or file owner is returned. The source entry changes name on success: the source name is absent and the destination name refers to the same entry. Open descriptors remain governed by the OS; the API does not close a writer, flush a writer, remove a temporary, or roll back a prior publication. On an accepted local filesystem, an operation error leaves source and destination entries unchanged. Durability after a crash is not promised because this API does not call `fsync`/`fdatasync`. | `align_sema` forms a distinct `RenameNoReplace` HIR expression with `Result<(), Error>`. The checked-HIR validator, replay, generic rechecking, MIR, LLVM lowering, runtime-key registry, and ABI golden all carry the two-path operation explicitly. The runtime selects the platform no-replace primitive and never falls back to ordinary `rename`, `link`+`remove`, an existence check, or a subprocess. | The new operation and its exact ABI shape participate in compiler/runtime capability and cache inventories. No new type or interface field is introduced. The owner is the `m9_fs` race/platform target, runtime native-call tests, ABI declaration golden, and the align-llm pair-publication adoption fixture. |

The error category for “already exists” deliberately remains `Error.Code` with
the platform's native `EEXIST` value. Adding an `AlreadyExists` variant or a
per-operation exception to the fixed errno table would create a second error
model and make the standard library's existing error contract inconsistent.
The category is stable (`Code`), while the numeric payload is target-native as
it is for every other unmapped errno. Adoption code treats the error as a
failed publication and does not depend on a cross-platform numeric constant.

The two operations have no hidden process-global lock, retry loop, existence
cache, temporary-name generator, cleanup thread, or durability promise.
Independent calls are independent native operations and may run concurrently.

## 2. Semantic contract

### 2.1 Path validation and error precedence

The runtime applies the following order to every operation:

1. Validate the ABI view length and pointer without dereferencing an invalid
   range. A negative length, a length that cannot fit the target address space,
   a positive length with a null pointer, or invalid UTF-8 yields
   `Error.Invalid`.
2. Reject an empty path and scan the complete byte view for an interior NUL.
   These are `Error.Invalid`, before any OS call or directory mutation.
3. For `rename_no_replace`, validate `source` fully before validating
   `destination`. If both are invalid, the source error wins. No native call is
   made until both views pass.
4. Invoke exactly one native operation. Native errors are mapped by the
   existing `ENOENT -> NotFound`, `EACCES`/`EPERM -> Denied`, `EINVAL ->
   Invalid`, otherwise `Code(errno)` table.

The language type checker already guarantees a valid `str` for ordinary Align
programs. The runtime repeats the ABI checks because the runtime boundary must
fail closed for malformed generated or foreign calls. The path may contain
ordinary UTF-8 whitespace, `..`, repeated separators, and non-ASCII names.
There is no lexical normalization and no implicit conversion to an owned
string. OS component and total-path limits are reported by the native call;
they are not guessed by a second platform-specific length table.

The operation does not stat or `exists`-check either final entry before its
native operation. Such a check would be advisory and would create the exact
race this capability exists to remove.

### 2.2 Exclusive creation

`create_exclusive` uses the equivalent of:

```text
write-only + create + exclusive + close-on-exec + no-follow-final-component
```

The exact native flag spelling is platform-specific, but `O_CREAT|O_EXCL` is
the semantic requirement. The final component is never followed. A competing
`create_exclusive` call naming the same absent path has exactly one successful
open; all losing calls fail with native `EEXIST`. An existing symlink fails as
an existing entry even when its target is absent. Existing regular files are
never truncated, and existing special files are never opened for writing.

The newly created entry is a regular file with the platform's normal creation
mode subject to the process umask. The contract makes no stronger permission,
owner, timestamp, or durability promise. Parent directories must already
exist and be searchable/writable as required by the OS. Symlinks in parent
components follow normal pathname resolution; rejecting those would require a
different directory-capability API and is outside this request.

The returned writer is the existing buffered writer. C6f2 must make write and
flush errors visible before the writer's scope ends when it needs an exact
staged byte sequence. Drop is still the final leak-safety boundary, not an
atomic-commit boundary: a failed write, failed flush, interruption, or normal
Drop can leave a partial staging file, and the caller must explicitly remove
it when cleanup is required.

### 2.3 No-replace rename

`rename_no_replace` is a single atomic directory-entry operation on one
filesystem:

```text
source entry --[native no-replace rename]--> absent destination entry
```

The final source component is operated on as an entry, not opened or followed.
Consequently:

- a regular source file is moved without copying its bytes;
- a source symlink, if supplied, is moved as a symlink and its target is not
  read or changed;
- a source special file is moved as a directory entry and is not opened; and
- a source directory is subject to the native rename rules, although the C6f2
  publication contract supplies only regular files created by
  `create_exclusive`.

This source-entry rule is intentional. A pre-rename `stat` followed by a
type-check would reintroduce a source race and could not make “regular file” an
atomic property of a path. The operation's safety promise is no replacement of
the destination, not a general trust or confinement boundary for an arbitrary
source path. C6f2 obtains that stronger provenance from its own exclusive
staging step and its fixed temporary-path policy.

The final destination is never followed, removed, or replaced. Any existing
destination type, including a symlink or special file, is an occupied entry
and produces `Error.Code(native EEXIST)`. A destination directory is likewise
an occupied entry; no recursive operation is attempted. If the source
disappears before the native operation, the result is `NotFound` and the
destination is untouched. If source and destination are on different
filesystems, the result is the native cross-device error (`Error.Code(native
EXDEV)` on the accepted Unix targets) and both names remain as they were.

No-replace support is a capability of the underlying filesystem, not a promise
that all mounted filesystems implement a portable emulation. A filesystem that
rejects the native flag returns its native unsupported/invalid error, with no
ordinary-rename fallback. The v1 acceptance floor is local ext4/tmpfs on Linux
and a local APFS volume on macOS; NFS, FUSE, overlay layers, and other remote or
non-qualified filesystems are outside the atomic-failure guarantee and are not
accepted by the C6f2 gate. The adoption preflight records the filesystem class
for each publication directory and fails before staging when it is outside the
declared floor.

The native basis is the Linux [`renameat2(...,
RENAME_NOREPLACE)`](https://man7.org/linux/man-pages/man2/renameat2.2.html)
operation and the macOS `renameatx_np(..., RENAME_EXCL)` operation. macOS's
[`rename(2)` reference](https://man.freebsd.org/cgi/man.cgi?manpath=macOS+10.13.6&query=rename&sektion=2)
documents the `RENAME_EXCL` behavior, and Apple's
[`volumeSupportsExclusiveRenaming`](https://developer.apple.com/documentation/foundation/urlresourcevalues/volumesupportsexclusiverenaming)
property exposes volume support. The POSIX/open flag basis is
[`O_CREAT|O_EXCL`](https://pubs.opengroup.org/onlinepubs/9799919799/functions/open.html):
the existence check and creation are one atomic operation and a final symlink
is not accepted. The implementation uses `AT_FDCWD` for the path-based surface
and does not expose directory descriptors or native flags to Align.

### 2.4 Publication pair and interruption

The two operations are primitives; they do not pretend that two renames form a
transaction. The C6f2 adoption owner uses the following visible sequence:

```text
create_exclusive(result_tmp)
write + flush + Drop(result_tmp)
create_exclusive(evidence_tmp)
write + flush + Drop(evidence_tmp)
rename_no_replace(result_tmp, result_final)
rename_no_replace(evidence_tmp, evidence_final)
```

The exact C6f2 implementation may stage the two writers in separate lexical
scopes, but it must close each writer before its rename and must preserve the
result-then-evidence order. A competing creator can win either final target;
the corresponding no-replace rename fails without deleting the winner.

If the second rename fails after the first succeeds, the first final artifact
remains. The library does not hide a delete-before-retry or silently roll back
the first publication. The caller explicitly removes the already-published
first target and any remaining temporary file in a fixed cleanup order. The
primary publication error wins over cleanup errors; cleanup is attempted for
all known paths, and no cleanup error is allowed to overwrite the primary
diagnosis. A process interruption may leave zero or one final artifact and any
still-staged temporary entry; the next invocation's explicit cleanup owns that
residue. There is no background recovery and no claim of pair atomicity.

The adoption owner must prove the following state table:

| Failure point | Required visible state before explicit cleanup | Required cleanup |
| --- | --- | --- |
| First exclusive create fails | No result writer; no library-created result temp is assumed | Return the first error; do not attempt evidence staging. |
| Result write/flush fails | Result temp may contain a partial prefix; no final result | Close/drop the writer, remove result temp explicitly, retain the primary write error. |
| Evidence create or write/flush fails | Result temp is complete; evidence temp may be absent or partial; no finals | Remove both temporary paths explicitly; retain the primary evidence error. |
| Result rename fails | Result temp remains; evidence temp remains; no finals | Remove both temporary paths; retain the primary result-publication error. |
| Evidence rename fails after result succeeds | Result final exists; evidence temp remains; evidence final absent or occupied by a competitor | Remove the result final and evidence temp explicitly; never remove a competing evidence final; retain the primary evidence-publication error. |
| Cleanup itself fails | The failed cleanup entry remains visible and is reported only as secondary evidence | Continue the fixed cleanup sequence and surface the original primary error. |

The table is an adoption contract, not hidden behavior in either `std.fs`
operation. It prevents a later client from smuggling in `write_file`,
delete-before-rename, or a check-then-create workaround.

## 3. Compiler, runtime, and ABI boundary

### 3.1 HIR and semantic formation

Add two distinct HIR expressions rather than a mode bit on an existing node:

```text
CreateExclusive { path: Expr }       // Result<writer, Error>
RenameNoReplace { source: Expr, destination: Expr } // Result<(), Error>
```

`check_method_call` requires `import std.fs` and dispatches exact names only.
Arity and path-type diagnostics happen before construction. An owned `string`
argument uses the existing non-consuming `str` borrow conversion; the operation
never moves a caller's path string. The two nodes are registered in every
visitor, replay clone, HIR-depth calculation, purity/effect table, escape and
ownership pass, checked-HIR validator, MIR lowering, MIR printer, and generic
body recheck. A missing visitor arm is a compile-time tripwire or a checked-HIR
rejection, never a silent fallthrough to `fs.create` or ordinary `rename`.

`CreateExclusive` is Impure because it changes filesystem state. `RenameNoReplace`
is Impure for the same reason. The path operands are borrowed views and carry no
return region. The writer result carries the existing static owned-handle
region and Drop plan.

### 3.2 Runtime ABI

The proposed runtime symbols are:

```text
align_rt_io_writer_create_exclusive(path_ptr: ptr, path_len: i64, out_writer: ptr) -> i32
align_rt_fs_rename_no_replace(
    source_ptr: ptr, source_len: i64,
    destination_ptr: ptr, destination_len: i64,
) -> i32
```

The first uses the existing writer-constructor ABI shape A08. The second uses
the existing four-argument path/status shape A09. Both return the existing
status encoding; zero is success and nonzero is reconstructed by MIR into the
builtin `Error`. The first leaves `out_writer` null on every failure. The
second has no output pointer and no ownership transfer.

The ABI ledger must add two `RuntimeKey` entries, two runtime ABI declarations,
two runtime-source symbol definitions, and the exact declaration golden. The
new declarations must not be grouped under the truncating constructor or the
ordinary replacing rename. No new runtime global, lock, allocator, or
descriptor table is introduced.

The Unix implementation is isolated behind small target-specific helpers:

```text
Linux:  OpenOptionsExt custom flags for O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC;
        renameat2(AT_FDCWD, source, AT_FDCWD, destination, RENAME_NOREPLACE)
macOS:  OpenOptionsExt custom flags for O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC;
        renameatx_np(AT_FDCWD, source, AT_FDCWD, destination, RENAME_EXCL)
```

The helper owns all raw FFI declarations and converts only the returned errno
to the existing status table. It retries no rename, does no preflight stat,
and performs no cleanup. A successful exclusive open transfers the descriptor
into the existing `Writer`; any constructor allocation failure closes the fd
before returning its terminal allocation behavior, preserving exactly-once
ownership.

The runtime must not use `std::fs::rename`, because its documented semantics
replace the destination. It must not emulate no-replace with `exists`,
`hard_link`+`remove`, a temporary lock, or a subprocess. A platform without a
native primitive remains unsupported for this capability until a separately
reviewed platform boundary exists.

### 3.3 Interfaces, caches, and separate compilation

The public result types are existing nominal builtins (`writer`, `Unit`, and
`Error`). No interface format field or type descriptor is added. Generic bodies
that contain either operation are reparsed and rechecked by the importing unit,
as for the other builtin calls. The operation kind and runtime key still enter
the MIR fingerprint and the compiler/runtime capability manifest so changing
the operation, status shape, or runtime symbol cannot reuse an incompatible
object or cache entry.

Whole-program and per-unit compilation must emit the same HIR/MIR operation,
runtime declaration, status reconstruction, writer Drop, and interface-visible
generic source. A program that does not use either operation must not acquire a
new runtime dependency beyond the existing filesystem/runtime archive shape.

## 4. Implementation closure matrix

The following matrix is the implementation gate. One parameterized owner may
close multiple cells; a row is not a promise of one new test function.

| Axis | Boundary and invariant | Owner evidence required before implementation PR review |
| --- | --- | --- |
| Public formation | Exact names, imports, arity, `str` coercion, result types, and Impure classification are stable. No alias or truncating fallback. | `m9_fs` positive programs for both operations, missing-import/arity/type negatives, owned-string path borrow, and unused/import replay checks. |
| Validation order | Invalid ABI length/pointer/UTF-8, empty path, and NUL fail before side effects; rename validates source before destination. | Runtime malformed-view matrix plus Align diagnostics with two invalid operands and exact primary error. |
| Exclusive open | One native atomic create; no preflight, no truncation, no final symlink follow, no special-file open. | Existing-file, directory, symlink-to-existing, dangling-symlink, FIFO/device controls; two competing creators with exactly one success and no changed loser target. |
| Writer construction | Success owns one fd and one writer Drop path; constructor failure never leaks a fd or publishes a writer. | Runtime fd-count/Drop owner, `Result<writer>` move/branch/`?`/`map_err`/return/loop joins, and successful write/readback. |
| Partial writes | Write/flush error can leave a partial staging file but never hides it or deletes it. Drop closes exactly once; explicit flush makes errors visible. | Injected write/flush failure, partial-byte assertion, Drop close counter, and explicit cleanup owner. |
| Native no-replace | Exactly one `renameat2(RENAME_NOREPLACE)` or `renameatx_np(RENAME_EXCL)` call; no ordinary rename or delete fallback. | Runtime syscall shim/counter or target-specific native owner proving flags, plus ABI symbol/fixture mutation that fails if the wrong key or fallback is used. |
| Destination race | Existing regular, directory, symlink, FIFO, and device all produce native EEXIST and remain unchanged. Two competing renames to one destination yield one winner and one EEXIST loser. | Barrier-synchronized child-process race owner on Linux and macOS accepted filesystems; byte/inode/state assertions for winner and loser. |
| Source states | Missing source, source symlink, source special entry, source directory, same path, and source replaced before call follow the documented entry semantics; no source preflight creates a TOCTOU claim. | Rust runtime matrix and Align adoption controls; C6f2 uses only a complete regular file from `create_exclusive`. |
| Filesystem boundary | Same-filesystem success; cross-device returns EXDEV; unsupported volume returns native failure; parent and length errors preserve entries. | Linux ext4/tmpfs and macOS APFS qualification, linked-worktree path owner, cross-device fixture, missing-parent, source-disappearance, and unsupported-volume disposition. |
| Symlink boundary | Final source is moved as an entry, final destination is never followed, and parent symlink behavior is ordinary OS resolution. | Source/destination symlink controls, including dangling links and a parent symlink, with no target mutation. |
| Pair cleanup | Result-then-evidence order; second-publication failure leaves first final visible until explicit cleanup; primary error precedence is stable. | `c6f2-request14-adoption` state table across every create/write/flush/rename/cleanup failure and interruption checkpoint; no `write_file`, delete-before-rename, or helper escape. |
| Process interruption | No pair atomicity is claimed; staged/final residue is explicit and recoverable by caller cleanup. | Kill-at-each-checkpoint owner, restart cleanup owner, no temporary artifact after successful cleanup, and no deletion of competing finals. |
| HIR/checked HIR | Every producer and validator sees both distinct operations and rejects forged/stale nodes. | HIR mutation sweep, checked-HIR malformed-node tests, replay/generic/whole/per-unit parity. |
| MIR/LLVM/ABI | Operation kind, path operands, status result, runtime key, declaration shape, and writer Drop are identical across lowering modes. | MIR print/fingerprint assertions, LLVM ABI golden, direct/indirect/generic/per-unit compile-and-run parity. |
| Error mapping | One fixed errno table; EEXIST/EXDEV remain native `Error.Code`; no new error variant or operation-local remap. | Per-platform native-code assertions and `Error` reconstruction tests, including unsupported-volume `EINVAL`/`EOPNOTSUPP`. |
| Concurrency | Calls use per-call state and no process-global lock; independent calls do not share staging or cleanup. | Concurrent independent create/rename calls, repeated cycles, and fd/resource leak loop. |
| Documentation and adoption | The language guide, spec, ABI ledger, design source, handoff, and request register agree; adoption uses only shipped operations. | Author ledger-to-prose pass, required English/Japanese mirror update after contract acceptance, and final align-llm `make ci`. |

No benchmark is required. The contract promises atomicity, no replacement, and
explicit cleanup, not a throughput or latency number. A local measurement may
be useful during implementation but is not a correctness gate.

## 5. Acceptance and test boundary

The implementation PR must extend the existing filesystem owner rather than
create a second general filesystem test matrix:

- `crates/align_driver/tests/m9_fs.rs` owns source formation, compilation,
  execution, readback, import, type, and control-flow behavior;
- `crates/align_runtime` unit owners exercise malformed ABI views, native flag
  calls, errno mapping, partial writes, Drop, and fd cleanup;
- the runtime ABI declaration golden owns symbol, shape, and declaration
  parity;
- existing generic, interface, whole-program, per-unit, cache, and cleanup
  owners are rerun only where the changed operation reaches their boundary; and
- the sibling align-llm `c6f2-request14-adoption` owner proves the real result
  then evidence pair, race matrix, cleanup state table, filesystem qualification,
  and absence of forbidden workarounds.

The accepted platform floor is Linux x86_64/ARM64 on a local ext4 or tmpfs
publication directory and macOS Apple Silicon on a local APFS publication
directory. The owner records the actual filesystem before testing. A linked
Git worktree path is a required ordinary-path case, not a special compiler
path. The gate does not claim atomic no-replace semantics for NFS, FUSE,
overlay, or other unqualified filesystems; those environments must fail the
adoption preflight before publication.

The implementation must use the repository's normal local verification policy:
the owner target first, then the bounded gate and Clippy for the code PR,
followed by one fresh full-diff implementation review and the normal
preflight/open/CI/merge sequence. The `align-self-review` skill applies when
the Rust implementation begins; it does not apply to this documentation-only
design review.

## 6. Non-goals and deferrals

- No transactional pair publish, journal, manifest, recovery daemon, or
  process-global publication lock.
- No `AlreadyExists` error variant, errno constants in the Align language, or
  per-operation error translation.
- No atomic replacement operation, exchange operation, delete-before-rename,
  recursive directory publication, or shell/subprocess implementation.
- No directory-relative capability handles, path sandbox, parent-symlink
  rejection, `openat2`/`RESOLVE_BENEATH` contract, or full path confinement.
- No source regular-file proof for arbitrary path arguments. The operation
  moves the source entry as the OS does; C6f2's regular-file provenance comes
  from `create_exclusive` and its staging policy.
- No `fsync`, `fdatasync`, directory sync, crash-durability guarantee, or
  cross-device copy fallback.
- No Windows or unsupported remote-filesystem implementation in this slice.
  Adding one requires a separate native capability review and acceptance
  floor update.
- No new writer type, explicit `close`, path clone, hidden temporary name, or
  automatic partial-file removal.

## 7. Documentation and lifecycle

During the design review, this file is the authoritative public-contract
ledger. Once the design is accepted, propagate the one settled contract to:

- `draft.md` §18.2 and `docs/language-spec.md`'s `std.fs` surface;
- `docs/guide/13-std-os.md` and its Japanese mirror;
- a focused `docs/impl/std-design/fs.md` and synchronized `ja/fs.md`;
- `docs/impl/20-runtime-abi-ledger.md`, the relevant native-boundary and
  test-policy records, and `docs/open-questions.md` Settled;
- `HANDOFF.md` at the capability boundary; and
- `/home/hiro/prj/align-llm/docs/align-requests.md`, with the sibling edit
  deliberately left uncommitted.

The design propagation must not claim that the implementation or adoption has
shipped. After the implementation merges, update status prose once at the
capability boundary, then run the exact align-llm batch command required by
the repository instructions:

```text
cargo build --release --workspace
```

Request 14 is complete only after the Align merge, the pinned
`c6f2-request14-adoption` owner, and the sibling `make ci` pass.

## 8. Author-side ledger consistency pass

The author pass for this proposed design records the following closure before
independent review:

- both public signatures, exact arity, path input behavior, results, error
  mapping, ownership, allocation, and owners appear only in §1;
- empty, NUL, malformed ABI, length, parent, existing-entry, symlink,
  special-file, source-disappearance, cross-device, unsupported-volume, and
  same-path states have one deterministic validation/native precedence;
- the source-entry rule and destination no-replace rule are not conflated, and
  the deliberate absence of a source-type preflight is repeated in the
  contract, matrix, adoption boundary, and non-goals;
- the writer's existing Drop/flush/partial-file behavior is explicit on every
  `Result` and control-flow exit, with no hidden remove;
- the pair sequence, interruption residue, first-final cleanup, primary-error
  precedence, and no-pair-atomicity claim are identical in §2 and the matrix;
- Linux/macOS primitive selection, native support failure, accepted filesystem
  floor, and excluded remote filesystems are recorded without an emulation
  fallback;
- HIR, checked HIR, MIR, LLVM, runtime keys, ABI declarations, replay,
  generic, per-unit, whole-program, and cache identity each have an explicit
  owner and no missing visitor path is implied;
- no later milestone, benchmark promise, CLI/build input, global state, or
  undocumented helper is consumed; and
- the listed language/spec/guide/ABI/mirror/register sources are the complete
  propagation set after design acceptance.

The ledger is intentionally not marked accepted yet. The independent design
review must challenge the native platform boundary, source-entry decision,
error identity, interruption cleanup, and the proposed PR capability boundary
before implementation work begins.

## 9. Independent design-review questions

The fresh adversarial review must answer these questions against this exact
document and its base:

1. Does the proposed native operation provide the promised atomic no-replace
   destination transition on every accepted filesystem, and does the plan fail
   closed where it does not?
2. Is the source symlink/special-entry rule honest under pathname races, or does
   any prose accidentally claim a source type guarantee that a path-based
   rename cannot provide?
3. Are `EEXIST`, `EXDEV`, unsupported-volume errors, and malformed path inputs
   consistent with the one existing Align error model and deterministic error
   precedence?
4. Does every writer, rename, interruption, and pair-publication state have an
   explicit owner, cleanup result, and no-hidden-side-effect rule?
5. Is this one useful capability boundary, or does it accidentally combine a
   separate concern that should be deferred or split before implementation?

The review is inspection-only. It must not run builds, tests, benchmarks, or
network work. Findings that alter a public surface are fixed in this ledger
first and propagated together before implementation authorization.
