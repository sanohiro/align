# `std.fs` retained-root single-link regular-file open

Status: **DESIGN ACCEPTED for align-llm Request 55. Implementation has not begun.**

This document is the authoritative public-contract ledger and implementation closure matrix for:

```text
fs.open_beneath_single_link(root: str, relative: str) -> Result<reader, Error>
```

The operation is the hard-link-rejecting sibling of `fs.open_beneath`. It exists for callers that
must reject a regular inode reachable through more than one directory entry before consuming its
bytes. It does not change `fs.open_beneath`, expose metadata, or promise that external filesystem
state cannot change after the constructor returns.

## 1. Public-contract ledger

| Surface | Exact inputs and defaults | Result and deterministic errors | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner | Identity, prerequisite, and acceptance |
| --- | --- | --- | --- | --- | --- |
| `fs.open_beneath_single_link(root: str, relative: str)` | Exactly two positional `str` values. There are no options, defaults, flags, ambient root, or environment input. Root and relative grammar, UTF-8/NUL/length checks, and written component order are exactly `fs.open_beneath`'s. | The complete `fs.open_beneath` validation and error sequence runs first. Its existing `fstat` of the opened final descriptor must have reported `st_nlink == 1`; that predicate is applied only after the descriptor is regular, identity-revalidated, and returned to blocking mode. Zero or more than one link is `Error.Invalid`. Missing is `NotFound`, permission is `Denied`, unsafe grammar/symlink/type/identity is `Invalid`, and every other native failure keeps the fixed errno mapping. No reader or byte is published on failure. | Both paths are borrowed only for the call. The operation reuses the existing owned path copies, O(1) retained-directory traversal, final descriptor owner, and existing Move `reader`. Every temporary allocation and descriptor closes exactly once on every recoverable failure; success transfers exactly the checked final descriptor to the reader. Actual allocation failure remains terminal OOM. | One distinct semantic/HIR kind, MIR rvalue, runtime key, A12 ABI symbol, checked-HIR/replay path, and existing two-path LLVM lowering. Native runtime reuses the retained-root traversal, regular-file opener, and its opened-descriptor stat record, then applies a final link-count predicate without another syscall. | The existing reader nominal identity and interface format remain unchanged. The constructor, runtime key, and MIR spelling enter compiler/cache identity. Linux and macOS owners cover ordinary files, both names of a multiply linked inode, later success after unlinking the extra name, same-opened-inode race control, and whole/per-unit/imported/generic parity. align-llm owns post-release adoption in its GPU bundle verifier. |

The public result contains only the existing `reader`. The descriptor, device, inode, mode, size,
and link count are never exposed. The operation is Impure because it observes filesystem state. It
adds no lock, retry, path reopening, directory enumeration, subprocess, current-directory mutation,
or process-global state.

## 2. Exact validation and race semantics

The constructor preserves this order:

1. reject a null output slot, otherwise clear it to null;
2. validate, copy, and parse the complete root;
3. validate, copy, and parse the complete relative path;
4. retain the starting directory and traverse root components in written order;
5. traverse relative parent components in written order;
6. observe the final entry without following it and require a regular file;
7. open it read-only, nonblocking, no-follow, and close-on-exec;
8. `fstat` the opened descriptor once, retaining its type, device/inode identity, and link count,
   then verify type plus device/inode identity against the observation;
9. restore blocking mode through the existing `F_GETFL`/`F_SETFL` path; and
10. require the retained opened-descriptor `st_nlink == 1` before transferring that descriptor to
    the existing reader constructor.

Steps 1–9 are the unchanged `fs.open_beneath` algorithm and retain its error precedence. Step 10 is
the only additional rejection point. It consumes the stat record already produced by step 8 and
therefore adds no syscall or native-error case; any link count other than exactly one is
`Error.Invalid`. The original `fs.open_beneath` never applies this predicate and continues to accept
multiply linked regular files intentionally.

The retained `fstat` record is descriptor-based. A pathname `stat`, `read_dir`, reopening the path,
or checking both names would lose the acquired inode identity and is forbidden. One barrier pauses after final
open but before descriptor `fstat`, then removes the last name or creates an alias to prove zero and
greater-than-one observations reject. A second barrier pauses after the single-link predicate,
replaces the public final name, and proves subsequent reads still come from the descriptor whose
link count was checked. Another process may add or remove a hard link after
the descriptor `fstat`; the API certifies the opened inode at that observation point and does not
claim a persistent filesystem lease. Byte mutation through another descriptor remains outside the existing
immutable-input/single-writer precondition.

No artifact bytes are read until the caller invokes `reader.read`. A rejected descriptor is closed
before return, the output slot stays null, and no reader owner is constructed. Root, intermediate,
final, and replacement symlink behavior; directories, FIFOs, sockets, devices, missing/denied paths;
malformed paths; and native error mapping remain identical to `fs.open_beneath` because all of those
checks precede the new predicate.

## 3. Compiler, runtime, ABI, and identity

The semantic and MIR records are distinct from the existing constructor:

```text
ReaderOpenBeneathSingleLink { root: Expr, relative: Expr }
ReaderOpenBeneathSingleLink { root: Operand, relative: Operand, out: Slot }
```

`check_method_call` recognizes the spelling only under `import std.fs`, validates exact arity and
the two `str`-coercible inputs in source order, borrows owned strings without moving them, returns
`Result<reader, Error>`, and marks the operation Impure. Every HIR visitor, effect/escape/move walk,
generic replay, checked-HIR validator, MIR printer/fingerprint, producer validator, and LLVM lowering
must classify the new record explicitly.

The runtime adds one key and symbol using existing ABI shape A12:

```text
IoReaderOpenBeneathSingleLink

align_rt_io_reader_open_beneath_single_link(
    root_ptr: ptr, root_len: i64,
    relative_ptr: ptr, relative_len: i64,
    out_reader: ptr,
) -> i32
```

LLVM reuses the two-path handle call lowering: root is evaluated/unpacked before relative, the output
slot is initialized to null, and the existing status-to-`Result` CFG owns the reader transfer and
cleanup. The exact ABI declaration, runtime key/symbol bijection, native export inventory, rt-LTO
inventory, and whole/per-unit declarations update atomically with implementation. No ABI shape,
reader layout, public type, or serialized interface field changes.

## 4. Implementation closure matrix

One parameterized owner may close multiple cells when it fails for the old defect. Implementation
review begins only after every applicable row points to code and a passing owner.

| Axis | Implementation invariant | Exact owner evidence |
| --- | --- | --- |
| Formation and effects | Exact imported name, two arguments, source-order `str` coercion, Impure result, borrowed owned-string operands, and no mode flag or alias on `open_beneath`. | `m9_fs` direct/imported/generic formation; missing import, wrong arity/type; owned-string retention. |
| Root/relative validation | Output slot, complete lexical validation, grammar, traversal order, and native error mapping are byte-for-byte shared with `open_beneath`. | Parameterized runtime matrix runs both constructors over malformed root/relative pairs and root forms `.`, `/`, relative, and absolute. |
| Root/intermediate/final type | No symlink following; directory identity revalidation; final regular-file and device/inode revalidation all precede link count. | Shared symlink, dangling, non-directory, directory, FIFO, socket, device, missing, denied, and failpoint owners compare both constructors. |
| Link-count predicate | The existing opened-final-descriptor `fstat` supplies the link count; after all existing open checks, exactly one is accepted and zero or greater than one maps to `Invalid`. No second syscall or path metadata substitutes. | Single-link success; two names both reject; unlinking either extra name makes a later call through the remaining name succeed; an after-open/before-`fstat` barrier produces deterministic zero and greater-than-one descriptor observations. |
| Same-inode race | The link predicate and returned reader refer to the same opened inode, even if the public final name is replaced after the check. | Barrier after the link check, final-name replacement, descriptor identity/readback assertion, and no reopened-path read. |
| Ownership and cleanup | Root, intermediate, final, and result-construction owners close exactly once; failure publishes neither reader nor bytes. Existing reader move/Drop identity is unchanged. | Descriptor/allocation counters at each existing failpoint plus link-count rejection; normal, `if`, `match`, `else`, `?`, `map_err`, loop, return, replacement, and Drop paths. |
| HIR and replay | Every new HIR producer/consumer validates operand types, exact result, effect, and operation identity; malformed nodes fail closed. | Variant-sweep tripwire; checked-HIR mutation of name/operand/type/effect/output; replay and generic interface owners. |
| MIR, producer, LLVM, and ABI | Distinct MIR spelling and runtime key use A12 with exact root-relative-output order and existing reader `Result` CFG. | MIR print/fingerprint, producer certification, LLVM call/declaration, runtime ABI golden, key/export reverse parity, and malformed MIR owner. |
| Whole/per-unit and cache | Whole-program and per-unit behavior/declarations match; definition edit/substitution changes identity and exact revert restores it. | Direct/imported/generic run in both modes plus checked interface/cache edit-revert owner. |
| Platform | Linux x86_64/ARM64 and macOS Apple Silicon use the same descriptor contract and native `st_nlink`; unsupported targets retain the existing fail-closed disposition. | Linux and macOS runtime/driver owners, including ordinary and hard-linked files under every accepted root form. |
| Consumer boundary | align-llm opens `manifest.json` and every declared GPU artifact through this constructor before any backend registry invocation. | After release, `gmake gpu-bundle-smoke` accepts canonical inputs and rejects hard-linked manifest/plugin while retaining all symlink refusals. Consumer code remains align-llm-owned. |

No benchmark is required: the request makes a safety, identity, and cleanup promise, not a latency or
throughput promise.

## 5. Non-goals

This capability does not expose `stat`, `st_nlink`, file descriptors, device/inode identity, mode,
or size. It does not reject hard-linked directories, enumerate aliases, remove an alias, lock an
inode, prevent later link creation, hash bytes, verify immutability, or alter `fs.open_beneath`.
There is no application-private native escape hatch and no path-based fallback.
