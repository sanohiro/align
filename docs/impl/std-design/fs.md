This file is the implementation-facing design for the `std.fs` extensions below.
The public-contract ledgers are
[`../27-fs-exclusive-publication-plan.md`](../27-fs-exclusive-publication-plan.md) and
[`../29-fs-retained-root-plan.md`](../29-fs-retained-root-plan.md). Request 55's single-link
extension is owned by [`../34-fs-single-link-plan.md`](../34-fs-single-link-plan.md). Request 56's
private temporary-directory lifecycle is owned by
[`../36-fs-private-temp-plan.md`](../36-fs-private-temp-plan.md).

# std.fs — explicit trusted filesystem boundaries

> 🌐 **English** · [Japanese](./ja/fs.md)

> **Status:** Request 14 IMPLEMENTED 2026-08-19 (design PR #859, merged as
> `a21eb8416f2088df68026f10c63a38cd0bd65538`; implementation PR #861, merged as
> `3c2edd2f399c9e2c9551b4227c61b36d6a041e20`). The align-llm adoption gate is
> pending. Request 18 retained-root regular-file access is IMPLEMENTED. Request 55 retained-root
> single-link open is IMPLEMENTED; align-llm adoption remains external. Request 56 private
> temporary-directory lifecycle is IMPLEMENTED; release and align-llm adoption remain pending.

## R65 sealed storage contract

[Plan 50](../50-r65-process-capability-handoff.md), designed and ready for implementation, owns
the explicit memory_kind, memory_writer and sealed_file declarations, native
seals, positional writable-out reads, ownership, ABI and acceptance. These are
Linux-only OS facilities with a defined unsupported-host error; ordinary native
Mac filesystem/process workflows do not depend on them. Materialization is
explicit and chunked, seal consumes the writer, and launch does not copy the
whole file. Executable admission and controlled inheritance belong to std.process.

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

Request 18 plus Request 55 add a separate three-operation boundary for regular files below one
retained root:

```text
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.open_beneath_single_link(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
```

These operations reject root, intermediate, and final symlinks and traverse from retained directory
descriptors. They add no directory-handle value, metadata API, canonical path, sandbox, or
process-global root.

Request 56's final pair creates one platform-selected private staging root and explicitly removes it only
after the caller has removed every known child. It is separate from general directory creation and
recursive cleanup.

```text
fs.create_private_temp_dir(prefix: str) -> Result<string, Error>
fs.remove_empty_dir(path: str) -> Result<(), Error>
```

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

### `open_beneath_single_link`

`open_beneath_single_link` is a distinct hard-link-rejecting constructor. It runs the complete
`open_beneath` grammar, retained-directory traversal, regular-file open, descriptor identity
revalidation, error mapping, and nonblocking-clear sequence unchanged. Immediately before reader
construction, it checks `st_nlink` from the existing `fstat` of that same opened descriptor.
Exactly `st_nlink == 1` succeeds; zero or more than one link is `Error.Invalid`.

The operation returns the same owned `reader` and exposes no descriptor or metadata. It reads no
artifact byte and publishes no reader on failure. Checking a pathname, enumerating a directory, or
reopening the final name is not equivalent because that would lose the opened descriptor identity.
A later external link-count or byte mutation is not prevented; the predicate certifies the opened
inode only at the final descriptor observation.

### `create_exclusive_beneath`

`create_exclusive_beneath` uses the same root/relative grammar and retained directory walk. At the
retained final parent it performs one native exclusive create with close-on-exec and final
no-follow flags. Every occupied final entry returns native EEXIST through the existing `Error.Code`
mapping and is untouched. Success returns the existing owned `writer`; partial writes, flush, Drop,
and explicit cleanup are exactly `create_exclusive`'s rules.

The operation creates no parent, temporary name, transaction, rename, rollback, or durability
state. It is the one-file retained-parent constructor; Request 14 remains the owner of no-replace
rename and C6f2 pair publication.

### `create_private_temp_dir`

`create_private_temp_dir` accepts one 1..=64-byte ASCII prefix. The first byte is alphanumeric;
remaining bytes are alphanumeric, `_`, or `-`. It reads no application path or environment
variable. Linux starts from `/tmp`; macOS starts from `confstr(_CS_DARWIN_USER_TEMP_DIR)`, including
its platform-provided terminal slash. Only that platform-owned spelling is canonicalized before
strict validation and retained no-follow traversal, so macOS's `/var` compatibility symlink does
not make the returned canonical absolute path unusable by retained-root APIs.

Each candidate leaf is the prefix, `-`, and 32 lowercase hexadecimal digits from 128 fresh
OS-CSPRNG bits. One `mkdirat` with mode `0700` atomically claims it; umask may narrow but never widen
the permissions. `EEXIST` draws a fresh suffix, with an exact 128-attempt bound. The operation never
reuses an occupant or creates a parent. It returns the owned absolute path. Its result allocation is
complete before the first create, so failure publishes no path and leaves no created directory.

### `remove_empty_dir`

`remove_empty_dir` accepts one absolute strict path with no empty, `.`, `..`, or trailing-slash
component. It retains and revalidates every ancestor and observes/opens the same final directory
without following symlinks. With parent and final descriptors still live, one
`unlinkat(..., AT_REMOVEDIR)` removes only the empty directory named at that syscall. It never
recurses or removes a symlink, file, special entry, or nonempty directory.

The native removal is the final namespace/type/emptiness linearization point. Linux and macOS do
not offer portable unlink-by-open-directory descriptor semantics: an empty-directory substitution
after final identity revalidation may be the name removed. For a path returned by the constructor,
the platform root and `0700` exclude other users and accidental application sharing; a hostile
same-OS-identity process is outside this capability. Arbitrary paths in shared non-sticky parents
receive no stronger promise.

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

align_rt_io_reader_open_beneath_single_link(
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

The private-directory lifecycle uses existing A08 and A04 shapes:

```text
align_rt_fs_create_private_temp_dir(
    prefix_ptr: ptr, prefix_len: i64, out_path: ptr,
) -> i32

align_rt_fs_remove_empty_dir(
    path_ptr: ptr, path_len: i64,
) -> i32
```

Output-slot validation is first, then complete root validation/copy/grammar, complete relative
validation/copy/grammar, root traversal, relative-parent traversal, and the final operation. Invalid
root grammar therefore wins over every relative-view error. Both slots are null on recoverable failure. Checked copy-size overflow is
`Error.Invalid`; actual OOM is terminal. Private full-path copies become NUL-delimited component
storage only after complete grammar validation; caller bytes are unchanged. At most two traversal
directory descriptors are live, and all path/component owners end with the call.

The constructor's A08 output is the existing owned-string slot. The runtime checks and zeros it
first, then allocates the exact output before any `mkdirat`. Removal's A04 input is absolute-only so
the constructor's output is consumed without a current-directory race. Both operations have
distinct HIR/MIR kinds and runtime keys.

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
to `Error.Invalid`; `open_beneath_single_link` additionally maps every descriptor link count other
than exactly one to `Error.Invalid`. Their two path operands are borrowed, and their successful reader/writer uses
the unchanged existing Move/Drop path. A same-final open/create pair has no hidden exclusion or
snapshot: open returns `NotFound` if it observes absence, but may acquire the newly created regular
inode while its writer is live. Consumers requiring immutable input must reject that overlap.

The private-directory operations are also `Impure`. Prefix/path operands are borrowed. The created
path is an ordinary owned `string`, so move, return, branch/loop joins, `?`, replacement, and Drop
use the existing string ownership path. Randomness failure and native creation/removal failures use
the fixed error mapping; only `EEXIST` creation collisions retry. Removal leaves nonempty and
mismatched entries observable for caller-owned cleanup. An interrupted random read retries, a
negative failure uses the fixed native-error mapping, and zero progress is `Invalid` rather than a
stale `errno` value.

## Platform boundary and non-goals

The accepted v1 adoption floor is a controlled local ext4/tmpfs filesystem on
Linux and a controlled local APFS filesystem on macOS. The runtime does not
classify filesystem types. NFS, FUSE, overlay, other remote or unqualified
filesystems, Windows, and a portable emulation are outside this capability.
The adoption fixture records its controlled filesystem environment before
testing; an unqualified environment is excluded by the consumer gate, not
silently classified by `std.fs`.

Request 14/18/55 add no transaction, journal, recovery daemon, process-global lock,
temporary-name generator, public directory-handle capability, sandbox,
replacement or exchange operation, or durability guarantee. Request 14's path-only operations keep
ordinary parent resolution; Request 18's two retained-root constructors supply only the explicit
no-symlink regular-file boundary described above.

Request 55 adds no metadata surface or persistent immutability promise. `open_beneath` remains
unchanged for callers that intentionally permit hard links.

Request 56 adds no environment-sensitive temp-root selection, caller root, directory handle,
recursive creation/removal, hidden cleanup, exit hook, quarantine registry, or same-identity hostile
process defense. General directory creation, listing, and type predicates remain Request 53.

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

Request 55 follows that established boundary with one distinct reader operation and runtime key,
the same A12 lowering, and a descriptor-only link-count predicate over the existing stat record.
Its complete contract and implementation closure matrix are in
[`34-fs-single-link-plan.md`](../34-fs-single-link-plan.md).

Request 56 adds one A08 owned-string constructor and one A04 unit-result remover, each with distinct
HIR/MIR/runtime identities and complete checked-HIR/whole/per-unit/export coverage. Its exact prefix,
platform-root, randomness, allocation-before-mutation, no-follow removal, race boundary, and
ownership matrix are in [`36-fs-private-temp-plan.md`](../36-fs-private-temp-plan.md).

### Ordinary directory creation and type observation

`fs.create_dir(path: str) -> Result<(), Error>` creates exactly one directory.
It requests Unix mode 0777 filtered by the existing process umask and native ACL
policy, without changing umask or creating missing ancestors. An existing entry
of any kind is an error, including an existing directory or symlink.

`fs.is_dir(path: str) -> Result<bool, Error>` follows ordinary path/symlink
resolution. A successful directory metadata observation returns true; a successful
other-kind observation returns false. Missing, denied, non-directory-ancestor,
broken-link, loop and other query failures remain errors. This observes type,
not writability, stable identity or future access.

Both require `import std.fs`, are Impure, take one required path and retain no
input. Owned strings auto-borrow as str. Reject empty paths, embedded NUL and
invalid UTF-8 before filesystem I/O. Relative paths use the current cwd; dot,
dot-dot, repeated and trailing separators have ordinary OS semantics. No path
normalization, environment expansion, recursion, caching or cwd mutation occurs.
Native path marshalling may allocate proportional to the explicit path; OOM keeps
the normal hard-error policy. Errors use the existing errno mapping, including
Code(EEXIST). No new owner, type tag or Error variant is added. The exact contract
and closure are in [ordinary directory operations](../43-ordinary-directory-plan.md).

### Retained directories and raw entry names

`import std.fs` exposes two opaque Move owners, `fs.directory` and `fs.dir_cursor`.
`fs.open_directory(path: str) -> Result<fs.directory, Error>` opens a retained,
no-follow directory. `directory.cursor() -> Result<fs.dir_cursor, Error>` creates
an independently positioned stream that survives directory Drop. Exclusive
`cursor.next() -> Result<Option<fs.dir_entry>, Error>` returns an owned raw basename,
then cached None at EOF or a cached Error after a native read failure. Only `.`
and `..` are omitted; there is no sorting, UTF-8 filtering or implicit metadata.
`fs.dir_entry` is the ordinary Move record `{ name: array<u8> }`.

All methods borrow a named local receiver for the call and retain no input.
An owned local cursor can advance internally; a borrowed helper needs `borrow mut`
for next. Temporary and field-path method receivers remain excluded. Ordinary
record/sum/Option/Result transport uses existing Move cleanup; direct opaque
collections, tuples, boxes, globals, captures, parallel and FFI transport remain
excluded. Drop closes owners without deleting directory entries.

Directory relative methods accept `bytes`: `metadata_at` returns `fs.metadata`,
`open_dir` returns `fs.directory`, `open_read` and `open_read_single_link` return
`reader`, and `create_new` returns `writer`, each inside `Result<_, Error>`.
`create_dir(path: bytes, mode: u32)`, `remove_file(path: bytes)`, and
`remove_dir(path: bytes)` return `Result<(), Error>`. Creation is exclusive and
one-level; create_new requests 0666 under normal umask/ACL rules. Failed publication
closes the new descriptor without rollback unlink. Removal is nonrecursive;
remove_file unlinks a symlink itself and remove_dir requires an empty directory.

Both root and relative paths reject NUL, empty/repeated/trailing components, `.`
and `..` components before I/O. Root alone additionally accepts `.` and `/`, an
optional leading slash, and requires UTF-8. Relative paths cannot be absolute and
accept arbitrary non-NUL bytes. Ancestors are admitted without following symlinks;
readers additionally require regular-file kind and descriptor identity revalidation.
The single-link form requires descriptor link count one before publication.

Directory, reader, writer and file each expose `metadata() -> Result<fs.metadata,
Error>` and `set_mode(mode: u32) -> Result<(), Error>` on the descriptor. Mode is
0..07777; invalid bits fail before I/O, and named creation checks path before mode.
Metadata does not change position or flush buffers. The ordinary Copy record has
fields, in declaration order: `kind: fs.entry_kind`, `device: u64`, `inode: u64`,
`links: u64`, `mode: u32`, `size: i64`, `modified_seconds: i64`,
`modified_nanoseconds: u32`, `changed_seconds: i64`, `changed_nanoseconds: u32`.
The Copy sum `fs.entry_kind` has payload-free Regular, Directory, Symlink, Other
variants in that order. Metadata conversion is checked, nanoseconds are below
1,000,000,000, and changed time is ctime rather than creation time. All operations
are Impure and use the existing Error model. Observations grant no snapshot,
writability, source immutability or identity-conditional deletion guarantee.
The exact validation precedence, ownership, platform/race limits and ABI are in
[the retained byte-tree ledger](../45-retained-byte-tree-plan.md).
