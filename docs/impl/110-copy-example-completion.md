# File-copy example destination and completion

The runnable `file_copy.align` and `io_copy.align` examples currently open their
destination with truncating `fs.create`. Passing the source itself, a hard link
or a symlink to it destroys the input before copying. Both examples also allow
the final buffered write to fail only during best-effort Drop after reporting
success.

Use the existing `fs.create_exclusive` writer constructor from plan 27 in both
examples. The output must be a new directory entry; every occupied destination
is refused through native EEXIST without modification. Opening the input still
precedes destination creation. Complete an explicit `w.flush()?` before returning
success or printing a transfer count. This is delivery to the existing writer,
not durable synchronization or atomic publication. A late read/write/flush error
may leave a partial newly created output; the examples do not remove it.

`file_copy` remains byte-exact with one visible 64 KiB caller buffer. `io_copy`
continues to demonstrate nonconsuming copy by appending one newline and printing
the count of source bytes. No library, compiler, API, ABI, allocation policy or
native filesystem implementation changes.

Plan148 uses buffer.try_new(65536)? for file_copy's fixed window before the
first read. Constructor errors propagate unchanged; the already-created
destination remains empty and the input is unchanged. io_copy uses the existing
native copy operation and creates no source-level read window.

| Closure axis | Owner |
| --- | --- |
| Actual example sources and ordinary completion | `file_copy_examples` compiles both checked-in files. Empty, short binary and multi-window payloads keep exact bytes; `io_copy` alone appends its documented newline and prints the source length. |
| Destination admission | The same owner crosses an ordinary occupied output, identical source path, hard link, source symlink, dangling symlink and directory. Source bytes, occupied destination bytes and link identities survive refusal. |
| Input and argument failure | Invalid arity and missing input create no destination. Existing library path/permission/native error owners remain applicable. |
| Buffered completion failure | A child-only zero RLIMIT_FSIZE with SIGXFSZ ignored produces EFBIG on a fresh output. Both actual examples must exit through the ordinary error boundary, publish no success count and preserve the input. The partial output remains caller-visible. Restoring omitted flush must fail this owner. |
| Fixture lifecycle | One exclusive ArtifactStage owns source, executable and data paths. A fresh process group and one deadline own each child from spawn through capture and cleanup; pipe reads are nonblocking and capped. Limits and signal disposition change only in the child. |

Preserve baseline failure evidence before changing the examples. Verify the
actual-source owner on Linux and macOS, then run one independent full-diff
review and the normal final-commit gate. No benchmark or broad design review is
needed for this correction to example behavior.
