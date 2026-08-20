This file is the implementation-facing design for the `std.fs` extensions below.
The public-contract ledgers are
[`../27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md) and
[`../29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md).

# std.fs — explicit trusted filesystem boundaries

> 🌐 **English** · [Japanese](./ja/fs.md)

> **Status:** Request 14 IMPLEMENTED 2026-08-19 (design PR #859, merged as
> `a21eb8416f2088df68026f10c63a38cd0bd65538`; implementation PR #861, merged as
> `3c2edd2f399c9e2c9551b4227c61b36d6a041e20`). The align-llm adoption gate is
> pending. Request 18 retained-root regular-file access is IMPLEMENTED; its align-llm adoption gate
> is pending.

## Overview

This is a narrow extension to the existing M9 filesystem surface. It supplies
the two native primitives required to publish a result and an evidence sidecar
without replacing a competing directory entry:

```text
fs.create_exclusive(path: str) -> Result<writer, Error>
fs.rename_no_replace(source: str, destination: str) -> Result<(), Error>
```

The operations are independent and impure. They are not a pair transaction,
do not add a new writer type, and do not change the existing `writer` Move or
`Drop` contract.

Request 18 adds a separate two-operation boundary for regular files below one retained root:

```text
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
```

These operations reject root, intermediate, and final symlinks and traverse from retained directory
descriptors. They add no directory-handle value, metadata API, canonical path, sandbox, or
process-global root.

## Public contract

### `create_exclusive`

`create_exclusive` performs one native exclusive open equivalent to
`O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC|O_NOFOLLOW` on the accepted Unix targets.
The final component is not followed. If any final entry already exists—a
regular file, directory, symlink, FIFO, or device—the operation returns
`Error.Code(native EEXIST)` and does not open, truncate, replace, or remove the
entry. Parents use ordinary OS pathname resolution; no `realpath`, parent walk,
or intermediate-symlink rejection is added.

Success returns the existing owned `writer`. It owns one descriptor and the
existing writer buffer, and `Drop` flushes best effort and closes the
descriptor. `Drop` never removes the file. A write or flush failure may leave a
partial regular file; callers that need cleanup must call the existing explicit
remove operation.

### `rename_no_replace`

`rename_no_replace` performs exactly one native no-replace directory-entry
rename: Linux uses `renameat2(AT_FDCWD, ..., RENAME_NOREPLACE)` and macOS uses
`renameatx_np(AT_FDCWD, ..., RENAME_EXCL)`. The destination must be absent.
Every occupied destination, including a regular file, directory, symlink, FIFO,
or device, produces `Error.Code(native EEXIST)` and remains untouched. The
source is moved as an entry: a source symlink or special file is not opened or
preflighted, and a source directory follows the native rename rules. C6f2
supplies the narrower trusted-path and single-writer precondition under which
its source is a regular file created by `create_exclusive`.

Missing source, cross-device, unsupported-volume, missing-parent, permission,
length, and other native failures use the fixed std errno table. There is no
ordinary replacing-rename fallback, `link` plus remove emulation, subprocess,
preflight existence check, cross-device copy, `fsync`, or crash-durability
promise. A successful rename removes the source name and gives the destination
name to the same directory entry; open descriptors remain governed by the OS.

### `open_beneath`

`open_beneath` accepts one root path and one strict relative path. The root may be absolute,
relative under the existing current-directory rule, exactly `.`, or exactly `/`; every other root
component is non-empty and is neither `.` nor `..`. The relative path is non-empty, has no leading
or trailing slash, and contains no empty, `.` or `..` component.

The runtime validates both complete lexical inputs before opening a directory. It then retains the
starting directory and walks every root and relative parent component with descriptor-relative
no-follow operations. Each observed and opened component must be the same directory identity. At
the final parent it observes the final entry without following it, requires a regular file, opens
it read-only/nonblocking/no-follow, rechecks the descriptor's type and identity, and only then
publishes the existing owned `reader`. The constructor reads no artifact byte. A missing component
is `NotFound`, permission is `Denied`, and unsafe grammar, a symlink, non-directory intermediate,
non-regular final, or identity/type change is `Invalid`.

After success, later reads use the retained file descriptor; renaming or replacing its public path
does not retarget the reader. The API does not prevent byte mutation through another descriptor.
Callers that need immutable input retain that explicit single-writer precondition.

### `create_exclusive_beneath`

`create_exclusive_beneath` uses the same root/relative grammar and retained directory walk. At the
retained final parent it performs one native exclusive create with close-on-exec and final
no-follow flags. Every occupied final entry returns native EEXIST through the existing `Error.Code`
mapping and is untouched. Success returns the existing owned `writer`; partial writes, flush, Drop,
and explicit cleanup are exactly `create_exclusive`'s rules.

The operation creates no parent, temporary name, transaction, rename, rollback, or durability
state. It is the one-file retained-parent constructor; Request 14 remains the owner of no-replace
rename and C6f2 pair publication.

## Path and ABI rules

The Request 14 operations borrow path views only for the call. A path must be non-empty,
valid UTF-8, NUL-free, and represented by a valid readable immutable byte range
for the call. Relative paths are resolved against the current directory exactly
as existing `std.fs` paths are. The runtime performs checkable length/null,
UTF-8, empty, and interior-NUL validation before any native side effect. A
checked `len + 1` capacity overflow is `Error.Invalid`. Actual allocation
failure uses Align's locked immediate-abort OOM policy; it is not a new
recoverable filesystem error.

`create_exclusive` uses the existing writer-constructor ABI shape:

```text
align_rt_io_writer_create_exclusive(
    path_ptr: ptr, path_len: i64, out_writer: ptr
) -> i32
```

The runtime checks `out_writer` for null first and writes the slot to null
before later validation. The caller-owned slot must be a valid writable
`*mut *mut Writer`; a foreign caller that violates that precondition is outside
the recoverable ABI contract. No writer is published on a recoverable failure.

`rename_no_replace` uses the existing four-path/status ABI shape:

```text
align_rt_fs_rename_no_replace(
    source_ptr: ptr, source_len: i64,
    destination_ptr: ptr, destination_len: i64
) -> i32
```

Source validation and its ephemeral NUL-terminated copy complete before
destination validation/allocation. Neither operation retains a path after the
native call. The compiler gives the two operations distinct HIR/MIR kinds and
runtime keys; neither is a mode bit on `fs.create` or an ordinary rename.

The retained-root operations each borrow two path views and use the A12 ABI shape:

```text
align_rt_io_reader_open_beneath(
    root_ptr: ptr, root_len: i64,
    relative_ptr: ptr, relative_len: i64,
    out_reader: ptr,
) -> i32

align_rt_io_writer_create_exclusive_beneath(
    root_ptr: ptr, root_len: i64,
    relative_ptr: ptr, relative_len: i64,
    out_writer: ptr,
) -> i32
```

Output-slot validation is first, then complete root validation/copy/grammar, complete relative
validation/copy/grammar, root traversal, relative-parent traversal, and the final operation. Invalid
root grammar therefore wins over every relative-view error. Both slots are null on recoverable failure. Checked copy-size overflow is
`Error.Invalid`; actual OOM is terminal. Private full-path copies become NUL-delimited component
storage only after complete grammar validation; caller bytes are unchanged. At most two traversal
directory descriptors are live, and all path/component owners end with the call.

## Pair-publication consumer

The primitives do not promise two-file atomicity. The C6f2 consumer owns the
following visible sequence after its trusted-path and single-writer
precondition is established:

```text
create_exclusive(result_tmp)
write + flush + Drop(result_tmp)
create_exclusive(evidence_tmp)
write + flush + Drop(evidence_tmp)
recheck result_final absent
recheck evidence_final absent
rename_no_replace(result_tmp, result_final)
rename_no_replace(evidence_tmp, evidence_final)
```

The rechecks are diagnostic only; the no-replace renames remain the race
boundary. Publication is result then evidence. If a clean staging or
finalization step fails, C6f2 reports `OUTPUT_WRITE` after removing its own
residue. If an owned cleanup or required recheck fails, it reports
`OUTPUT_PAIR_CLEANUP_FAILED` with only the exact surviving evaluator-owned
paths. It never removes a competing final destination. If the first rename
succeeds and the second fails, the first final remains until explicit consumer
cleanup; an interruption may leave zero or one final and temporary residue.

## Errors, effects, and ownership

The Request 14 operations are `Impure` because they mutate directory state. They use the
existing errno table: `ENOENT` → `Error.NotFound`, `EACCES`/`EPERM` →
`Error.Denied`, `EINVAL` → `Error.Invalid`, and every other native code,
including `EEXIST` and `EXDEV`, → `Error.Code(errno)`. No `AlreadyExists`
variant is added. The pair-level `OUTPUT_*` statuses belong to C6f2 and do not
alter the primitive error model.

The path operands are borrowed `str` views and are never moved or retained.
`create_exclusive` returns the existing `writer` Move value, so normal writes,
flushes, `?`, `map_err`, branch and loop joins, returns, early exits, and Drop
must use the existing writer ownership path. No implicit rollback or delete is
performed after a partial write.

The retained-root operations are also `Impure`. They use the same fixed error model while mapping
an unsafe grammar, symlink/non-directory traversal component, non-regular input, or identity change
to `Error.Invalid`. Their two path operands are borrowed, and their successful reader/writer uses
the unchanged existing Move/Drop path. A same-final open/create pair has no hidden exclusion or
snapshot: open returns `NotFound` if it observes absence, but may acquire the newly created regular
inode while its writer is live. Consumers requiring immutable input must reject that overlap.

## Platform boundary and non-goals

The accepted v1 adoption floor is a controlled local ext4/tmpfs filesystem on
Linux and a controlled local APFS filesystem on macOS. The runtime does not
classify filesystem types. NFS, FUSE, overlay, other remote or unqualified
filesystems, Windows, and a portable emulation are outside this capability.
The adoption fixture records its controlled filesystem environment before
testing; an unqualified environment is excluded by the consumer gate, not
silently classified by `std.fs`.

There is no transaction, journal, recovery daemon, process-global lock,
temporary-name generator, public directory-handle capability, sandbox,
replacement or exchange operation, or durability guarantee. Request 14's path-only operations keep
ordinary parent resolution; Request 18's two retained-root constructors supply only the explicit
no-symlink regular-file boundary described above.

## Implementation and acceptance boundary

The implementation must add distinct semantic/HIR, checked-HIR, replay, MIR,
LLVM, runtime-key, ABI declaration, and native-runtime paths. It must preserve
whole-program/per-unit identity and the existing reader/writer nominal types. The
planned ABI rows are A08 for the constructor and A09 for the two-path rename;
the runtime ABI golden and key↔symbol/export parity are updated atomically with
the implementation.

The owner evidence is split by boundary:

- `crates/align_driver/tests/m9_fs.rs` owns formation, imports, execution,
  readback, control flow, and type diagnostics;
- `crates/align_runtime` owns malformed ABI views, native flags, errno mapping,
  partial writes, Drop, fd cleanup, and platform controls;
- the runtime ABI declaration golden owns exact symbols, shapes, and parity;
- existing generic, interface, cache, whole/per-unit, and cleanup owners are
  rerun only where the new operations reach them; and
- align-llm's `c6f2-request14-adoption` owns the complete pair race, cleanup,
  interruption, accepted-filesystem, and forbidden-workaround matrix.

No benchmark is required: this contract promises atomic no-replace behavior and
explicit ownership, not throughput.

The full closure matrix, acceptance table, and review-finding dispositions are
in [`27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md).

Request 18 uses the same cross-stage rule: distinct `ReaderOpenBeneath` and
`CreateExclusiveBeneath` nodes, complete visitor/validator/replay/MIR closure, exact A12 runtime
rows and export parity, existing handle Drop, whole/per-unit/cache parity, Linux and macOS
descriptor-walk owners, and align-llm's real `c6d-request18-adoption` consumer. Its complete matrix
is [`29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md). No benchmark is required because
the new contract is safety and ownership, not throughput.
