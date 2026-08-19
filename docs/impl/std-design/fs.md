This file is the implementation-facing design for the `std.fs` extension below.
The accepted public-contract ledger is
[`../27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md).

# std.fs — exclusive publication (Request 14)

> 🌐 **English** · [Japanese](./ja/fs.md)

> **Status:** IMPLEMENTED 2026-08-19 (design PR #859, merged as
> `a21eb8416f2088df68026f10c63a38cd0bd65538`; implementation PR #861, merged as
> `3c2edd2f399c9e2c9551b4227c61b36d6a041e20`). The align-llm adoption gate is
> pending.

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

## Path and ABI rules

Both operations borrow path views only for the call. A path must be non-empty,
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

Both operations are `Impure` because they mutate directory state. They use the
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

## Platform boundary and non-goals

The accepted v1 adoption floor is a local ext4/tmpfs publication directory on
Linux and a local APFS publication directory on macOS. The runtime does not
classify filesystem types. NFS, FUSE, overlay, other remote or unqualified
filesystems, Windows, and a portable emulation are outside this capability.
The adoption fixture records its controlled filesystem environment before
testing; an unqualified environment is excluded by the consumer gate, not
silently classified by `std.fs`.

There is no transaction, journal, recovery daemon, process-global lock,
temporary-name generator, directory-relative capability, sandbox, parent
symlink policy, source regular-file proof for arbitrary paths, replacement or
exchange operation, or durability guarantee.

## Implementation and acceptance boundary

The implementation must add distinct semantic/HIR, checked-HIR, replay, MIR,
LLVM, runtime-key, ABI declaration, and native-runtime paths. It must preserve
whole-program/per-unit identity and the existing writer nominal type. The
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
