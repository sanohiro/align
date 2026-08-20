# `std.fs` retained-root regular-file access

Status: **DESIGN CANDIDATE for align-llm Request 18. Implementation has not started.**

This document is the authoritative public-contract ledger and implementation plan for two
descriptor-relative filesystem constructors:

```text
fs.open_beneath(root: str, relative: str) -> Result<reader, Error>
fs.create_exclusive_beneath(root: str, relative: str) -> Result<writer, Error>
```

The capability exists for callers that need to prove that a regular input or newly created output
is reached below one retained directory without following a root, intermediate, or final symlink.
It does not add general metadata reflection, a directory-handle value, a sandbox, a path
canonicalizer, or application-specific lifecycle behavior.

Implementation begins only after this ledger receives one fresh independent adversarial review,
all design findings are resolved here first, and the design commit merges. The implementation is
one consumer-complete compiler/runtime boundary: both constructors share one trusted traversal
algorithm, existing reader/writer ownership, checked HIR, MIR/LLVM lowering, runtime ABI, and the
real align-llm adoption owner. Splitting the constructors from that shared proof would either leave
a dormant operation or duplicate the component/race/cleanup matrix. The expected implementation
may exceed 1,000 hand-written lines for that reason; the larger boundary removes duplicated safety
proof and lowers integration risk.

## 1. Public-contract ledger

| Surface | Exact inputs and defaults | Result and deterministic errors | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner | Identity, prerequisite, and acceptance |
| --- | --- | --- | --- | --- | --- |
| `fs.open_beneath(root: str, relative: str)` | Exactly two positional `str` values and no options, defaults, environment input, process-global root, or retained path. `root` names an existing directory. It may be absolute, relative to the existing process current directory, the exact `.` root, or `/`; every other component is non-empty and not `.` or `..`, with no trailing slash. `relative` is non-empty and relative, has no leading/trailing slash, and has no empty, `.` or `..` component. `/` is the only separator on the accepted Unix targets; backslash is an ordinary filename byte. | `Ok(reader)` only after the final entry is opened from the retained parent and proven to be the same regular file observed immediately before open. `NotFound` for the first missing root/parent/final component; `Denied` for the first permission failure; `Invalid` for invalid ABI text/grammar, a symlink component, a non-directory root/intermediate, a non-regular final, or a final identity/type change between observation and open; all other native failures use the fixed errno table. Output-slot validation wins, then complete root grammar, complete relative grammar, root traversal in written order, relative parents in written order, and the final entry. No later error replaces the first. | Both paths are borrowed only for the call. Per-call NUL-terminated copies and directory descriptors are explicitly released on every recoverable path. Checked length/capacity overflow is `Invalid`; actual allocation failure uses the locked immediate-abort OOM policy. Success transfers exactly one existing Move `reader`; its descriptor, read, `?`, `map_err`, branch/loop/return, and Drop rules are unchanged. No byte is read by the constructor. | A distinct `ReaderOpenBeneath` HIR/MIR operation, checked-HIR/replay/visitor closure, one runtime key/symbol, and a shared retained-directory runtime helper. The final reader uses the existing reader constructor and cleanup. | Builtin spelling and runtime key enter the compiler/runtime capability and MIR/cache identity. No type/interface format changes. The implementation is blocked on the accepted design; align-llm then owns `c6d-request18-adoption` plus its C6d lifecycle owner. |
| `fs.create_exclusive_beneath(root: str, relative: str)` | The same two-argument root/relative grammar and validation order. The root and every parent must already exist as real directories; the final component must be absent. There is no mode, umask override, temporary-name policy, overwrite switch, current-directory mutation, or implicit parent creation. | `Ok(writer)` only after one native exclusive create relative to the retained final parent. Any occupied final entry—regular, directory, symlink, FIFO, socket, or device—returns the existing native EEXIST-backed `Error.Code` and remains unchanged. Root/parent grammar, missing, denied, symlink, non-directory, and other errors match `open_beneath`. The output slot is null on every recoverable failure. | Paths and traversal storage are per-call borrows/owners. Success returns the existing Move `writer`; its 64 KiB buffer, write/flush/`?`/`map_err`/branch/loop/return/Drop, partial-file, and explicit cleanup rules are exactly Request 14's. Drop never removes the new file. No hidden rollback, retry, or delete occurs. | A distinct `CreateExclusiveBeneath` HIR/MIR operation and runtime key/symbol reuse the same retained-directory helper and existing writer construction. The final native call is `openat`-style `O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC|O_NOFOLLOW`, never check-then-create. | Same identity rules; no new writer type or interface field. The real-client owner acquires this writer as CLI output preflight before decoded request-field validation, then writes one bounded canonical result or explicitly cleans a failed partial output. Request 14's pair-publication adoption remains separate. |

Both operations are Impure because they inspect external directory state and may open/create a file.
They do not mutate process current directory, install a process-global root, cache a descriptor,
launch a process, inspect an environment variable, enumerate a directory, normalize a path, or
infer a deployment root.

### 1.1 Encoding, bounds, and ABI preconditions

Ordinary Align callers already supply valid UTF-8 `str` views. The runtime boundary also defines
the checkable foreign/generated-call contract. For each view, a negative or unrepresentable length,
a positive length with a null pointer, invalid UTF-8, empty bytes where not explicitly allowed, or
an interior NUL is `Error.Invalid`. A non-null pointer must name a readable immutable range for the
declared length; a dangling non-null address is outside the recoverable ABI contract. The non-null
output pointer must name a writable handle slot; violating that precondition is likewise outside the
contract.

The exact validation order is:

1. reject a null output slot, then write a non-null slot to null;
2. validate and copy the complete root view;
3. validate and copy the complete relative view;
4. parse the complete root and relative component grammars without a filesystem call;
5. open and retain the starting directory and walk root components in written order;
6. walk relative parent components in written order; and
7. perform the operation-specific final observation/open or exclusive create.

Thus two malformed text inputs report the root error; a valid root with two invalid relative
components reports the earlier component; and no directory descriptor or final operation exists
until both complete lexical inputs are valid. UTF-8 byte sequences are preserved exactly. There is
no Unicode normalization, case folding, separator rewriting, lossy filename conversion, or trailing
slash removal.

Each complete owned path copy checks `len + 1` before allocation, appends one NUL, and lives through
the call. After full grammar validation, the runtime replaces separator bytes only in those private
copies with NUL component delimiters; caller bytes are never changed. The walk retains at most the
current directory descriptor plus the next descriptor and closes the previous descriptor after the
next one is safely acquired. Memory and live traversal-fd usage are `O(total path bytes)` and `O(1)`
respectively. Actual OOM remains terminal, and no performance threshold is promised.

### 1.2 Root and relative grammar

The accepted roots are:

```text
.
/
name[/name...]
/name[/name...]
```

Every `name` is one or more UTF-8 bytes other than `/`, is not `.` or `..`, and contains no NUL.
`root` has no trailing slash except when it is exactly `/`. A relative root begins from a retained
descriptor for the process current directory; an absolute root begins from a retained descriptor
for `/`. The exact `.` root means the retained current directory and performs no component walk.
The operation never calls `chdir`.

`relative` has exactly this form:

```text
name[/name...]
```

It is always interpreted from the retained root descriptor, never from process cwd. Absolute
`relative`, empty text, repeated separators, `.`/`..`, and a trailing separator are invalid. These
restrictions make containment structural rather than a pathname-prefix inference.

The public API deliberately does not return a canonical path. A canonical string would not retain
the directory objects that supplied its proof and would invite a second path-based open. The
reader/writer itself is the retained capability returned to the caller.

## 2. Filesystem and race semantics

### 2.1 Retained-directory traversal

The implementation uses one target-owned helper for both constructors:

1. open `.` or `/` as the starting directory with close-on-exec;
2. for every root component, observe it without following the final component, require a directory,
   open it relative to the retained parent with directory/no-follow/close-on-exec flags, verify the
   opened descriptor is the same directory identity and type, then release the previous descriptor;
3. repeat the same step for every relative parent component; and
4. hand the retained final-parent descriptor and final component to the selected constructor.

The observed/opened identity is the platform device/inode pair. A disappearance, replacement,
type change, or identity mismatch between observation and descriptor acquisition is `Invalid`; the
operation does not retry or restart from a public pathname. Once a component descriptor is retained,
renaming or replacing its public ancestor cannot redirect later component operations.

Linux may use `openat2` only where its flags implement the same contract. The portable accepted
Linux path remains descriptor-relative `fstatat(AT_SYMLINK_NOFOLLOW)` plus
`openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` and descriptor identity revalidation. macOS uses the
corresponding `fstatat`/`openat` sequence. No implementation depends on `/proc`, a shell, `realpath`,
pathname prefix comparison, `chdir`, or a process-global lock.

### 2.2 Regular input

At the retained parent, `open_beneath` first observes the final entry with no symlink follow and
requires a regular file. It then opens that name read-only, nonblocking, no-follow, and close-on-exec,
immediately verifies that the descriptor is regular and has the same device/inode identity, clears
nonblocking state when the platform applies it to ordinary file descriptions, and only then
constructs the existing `Reader`. A failed post-open type/identity check is `Invalid`; a failed
nonblocking-clear operation uses the fixed native-error mapping. Either failure closes the final
descriptor and retained parent before returning. No artifact byte is read during this sequence.

A FIFO is opened only after a no-follow observation classified it as regular, which is impossible
under the documented single-writer directory precondition; the nonblocking flag additionally keeps
a replacement race from waiting on a FIFO. A replacement that yields any special descriptor or
identity mismatch is closed and rejected. Opening a native device can itself have device-specific
effects, so callers requiring the stronger “no hostile concurrent replacement” property must uphold
the same single-writer/immutable-input precondition that C6 already declares. The API guarantees
that no special descriptor is published and no bytes are read from one; it does not claim to make
arbitrary hostile device opens side-effect-free after a concurrent replacement.

After success, path replacement cannot retarget the `Reader`: reads use the retained final fd.
Concurrent mutation through another fd remains ordinary external state and is not hidden by this
constructor.

### 2.3 Exclusive output

At the retained parent, `create_exclusive_beneath` performs one native exclusive create with no
final symlink follow. It does not preflight final absence with `exists` or metadata, and it does not
fall back to `fs.create`. Every occupied final name fails without opening or changing that entry.
After the fd is created, the existing Writer allocation/ownership path takes over. A terminal OOM
after native creation remains Align's immediate-abort policy; there is no recoverable path that
leaks an unowned descriptor.

The operation is one-file publication only. It adds no rename, pair transaction, durability,
temporary-name generation, cleanup, or recovery policy. Request 14 remains the owner of
`rename_no_replace` and the future C6f2 result/evidence pair.

### 2.4 Error table

| First failing state | Public result |
| --- | --- |
| malformed ABI slot/view, invalid UTF-8/NUL/length/capacity, invalid root/relative grammar | `Error.Invalid` |
| missing starting/root/parent/final input component | `Error.NotFound` |
| denied starting/root/parent/final access | `Error.Denied` |
| root/intermediate symlink, dangling link, or non-directory | `Error.Invalid` |
| final input symlink, directory, FIFO, socket, device, other special, or observed/opened identity mismatch | `Error.Invalid` |
| occupied final output of any type | `Error.Code(native EEXIST)` |
| every other native error | existing fixed errno mapping |

No `AlreadyExists`, `NotRegular`, `Symlink`, or path-specific error variant is added. The caller
maps these standard categories into its own persisted domain labels. This preserves Align's one
error model.

### 2.5 Overlap and global state

The complete operation-pair policy is:

| Pair | Policy |
| --- | --- |
| two `open_beneath` calls, same or different roots/files | supported independently |
| `open_beneath` plus `create_exclusive_beneath`, disjoint finals | supported independently |
| two `create_exclusive_beneath` calls, disjoint finals | supported independently |
| two exclusive creates of the same final | exactly one native winner; the loser receives EEXIST and does not change the winner |
| aggregate/focused test process plus another independent process | supported when their caller-owned paths are disjoint; same-output competition follows the row above |
| a hostile mutator replacing an input after its regular descriptor opens | the reader remains bound to the retained fd; byte mutation is outside the immutable-input precondition |

No pair changes cwd, environment, signal handlers, umask, process-global caches, locks, or retained
roots. A failed second operation has no state to restore beyond closing its own descriptors and
buffers.

## 3. Compiler, runtime, ABI, and identity

### 3.1 Semantic/HIR surface

Add two expression kinds:

```text
ReaderOpenBeneath { root: Expr, relative: Expr }            // Result<reader, Error>
CreateExclusiveBeneath { root: Expr, relative: Expr }       // Result<writer, Error>
```

`check_method_call` accepts the names only under `import std.fs`, validates exact arity and both
`str`-coercible inputs in source order, and constructs no node after either type error. Owned
`string` operands are borrowed for the call and remain caller-owned. Both operations are Impure.
The handle result has the existing static owned-handle region and Drop plan.

Every HIR visitor, effect/escape/move walk, replay clone, generic recheck, depth checker, checked-HIR
validator, malformed-node owner, and enum sweep must classify both variants. The validator rechecks
operand types, exact result type, effect, and operation identity before MIR publication.

### 3.2 MIR and LLVM

Add distinct MIR rvalues rather than a flag on the existing path-only constructors:

```text
ReaderOpenBeneath { root: Operand, relative: Operand, out: Slot }
WriterCreateExclusiveBeneath { root: Operand, relative: Operand, out: Slot }
```

Both use the existing open-handle `Result` CFG, status reconstruction, source operand lifetime, and
handle cleanup. MIR printing/fingerprints distinguish the operations from `fs.open`, `fs.create`,
and `fs.create_exclusive`.

The proposed runtime symbols are:

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

Both use existing ABI shape A12: `i32 @SYM(ptr, i64, ptr, i64, ptr)`, with no curated LLVM
attributes. LLVM evaluates root before relative, keeps both views live through the call, initializes
the handle slot to null through the runtime contract, and reconstructs the same builtin `Error`
status used by other filesystem constructors.

The runtime registry adds `IoReaderOpenBeneath` and `IoWriterCreateExclusiveBeneath`. The ABI row,
exact declaration golden, key/symbol bijection, runtime export parity, source-compatible extern
rules, whole/per-unit declarations, and rt-LTO inventories update atomically. No direct unregistered
extern is permitted.

### 3.3 Interface and cache identity

No public type, interface field, serialized descriptor, runtime layout, or handle ABI changes.
Generic source containing either builtin is rechecked in the importing unit through the normal
interface source path. The distinct HIR/MIR kinds and runtime keys enter compiler build and MIR
fingerprints; a definition edit, operation substitution, or runtime-symbol change misses stale
caches, while reverting the exact definition restores identity under the existing rules.

Whole-program and per-unit compilation must emit identical argument order, status reconstruction,
runtime declaration, handle transfer, and Drop. Programs not using these operations acquire no
call-site dependency beyond the ordinary linked runtime archive.

## 4. Implementation closure matrix

One parameterized owner may close several rows. Before implementation review, every applicable row
must point to its final diff and passing owner or be explicitly deferred here.

| Axis | Implementation invariant | Exact owner evidence |
| --- | --- | --- |
| Public formation | Exact names, import, two arguments, source-order type checking, `str` coercion, result types, and Impure effects; no aliases or flags on old operations. | `crates/align_driver/tests/m9_fs.rs` direct/imported positives, missing-import/arity/type negatives, owned-string borrow retention, generic and per-unit calls. |
| ABI validation order | Null output wins; root view/grammar precedes relative view/grammar; no descriptor before both complete lexical inputs; output stays null. | Runtime foreign-call matrix for null/negative/unrepresentable/UTF-8/empty/NUL/capacity cases and two-invalid precedence, with fd/allocation counters. |
| Root formation | `.`, `/`, relative, and absolute roots retain the correct starting descriptor; every other empty/dot/dotdot/trailing component fails. | Root grammar table plus working-directory sentinel proving no `chdir` or root cache. |
| Component traversal | Every root and relative parent component is observed/opened no-follow, directory/type/identity checked, and previous descriptors close exactly once. | Nested ordinary success; root/intermediate/final and dangling symlinks; non-directory intermediates; public ancestor rename/replacement at every barrier; fd-count loop. |
| Input formation | Final is observed no-follow, opened nonblocking/no-follow, revalidated as same regular inode, and only then published as Reader. | Empty/non-empty regular files; directory/FIFO/socket/device/symlink controls; missing/denied; observed-opened replacement matrix; no special byte read; exact readback. |
| Input ownership/exits | Reader owns one fd; paths/descriptors do not escape; all failure and caller control paths close exactly once. | normal, `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early return, replacement, move-out/return, and Drop owners. |
| Output formation | One retained-parent exclusive create with no final follow; occupied entries are untouched; no check-then-create. | regular/directory/symlink/dangling/FIFO/device occupancy; missing/denied parent; synchronized two-creator race; inode/byte preservation. |
| Output ownership/exits | Writer construction and partial writes reuse Request 14 exactly; no implicit remove. | writer write/flush/Drop/fd count, partial-write failpoint, explicit cleanup, all control-flow owners above. |
| HIR/checked HIR | All producers/consumers classify both nodes; malformed/stale fields fail closed. | variant-sweep compile tripwire; mutation of operation, operand, type, effect, and output slot; replay/generic parity. |
| MIR/LLVM/ABI | Distinct rvalues and exact A12 runtime rows, root-before-relative call order, existing status/handle CFG. | MIR print and fingerprint goldens; LLVM declaration/call/body assertions; runtime ABI golden; key/export reverse parity. |
| Interfaces/cache | No format addition; imported/generic whole/per-unit calls retain exact behavior and identity. | interface compile/run, definition edit/substitution/revert cache owner, corrupted interface/checked-HIR rejection. |
| Allocation/failure | Path/component owners and every intermediate fd balance on all recoverable failures; checked overflow is Invalid and actual OOM terminal. | allocation/fd counter matrix for zero/one/many components, each native failure injection, terminal OOM child, repeated cycles. |
| Concurrency | Per-call state only; independent calls overlap; same create has one winner; failed second calls restore nothing global. | same-process full pair matrix, aggregate-plus-focused process pair, two-process independent and competing output cases. |
| Platform | Linux x86_64/ARM64 and macOS Apple Silicon implement the same descriptor contract without `/proc`. | local ext4/tmpfs and APFS owners, linked-worktree path, target-specific syscall/flag assertions, unsupported-platform compile disposition. |
| Real client | C6d uses these operations before artifact decode/write, maps standard errors deterministically, and keeps verifier/lineage semantics separate. Lexical path rejection is `INVALID_PATH`; a referenced-input `NotFound` is `INPUT_NOT_FOUND`, `Invalid` (including a no-follow/type rejection) is `INPUT_TYPE`, and `Denied`/other read failures are `INPUT_READ`. Output-constructor failure remains the CLI filesystem `Result` boundary and emits no result artifact. | align-llm `c6d-request18-adoption`, then the complete `prompt-state-smoke`; no `exists`/ordinary `open`/`write_file`, subprocess, or app FFI on this boundary. |

The matrix covers formation, construction, move-in/out, source retention, Drop, replacement, return,
every relevant control-flow exit, generic monomorphization, interface serialization, whole/per-unit
compilation, runtime ownership, allocation parity, malformed input, and the complete same-process
operation pairing. No benchmark is required: this capability makes no speed, resource ceiling, or
latency claim beyond path-bounded memory and constant live descriptor count, which are
correctness owners.

## 5. Acceptance and delivery boundary

The implementation extends the existing filesystem owner rather than creating a second broad suite:

- `crates/align_driver/tests/m9_fs.rs` owns source formation, whole/per-unit execution, readback,
  control flow, path tables, and public diagnostics;
- `crates/align_runtime` owns ABI validation, component traversal, native flags, race barriers,
  errno mapping, allocation/fd cleanup, Reader/Writer transfer, and platform controls;
- `crates/align_mir` checked-HIR and variant sweeps own cross-stage completeness;
- `crates/align_codegen_llvm` runtime ABI registry/golden/export owners own the exact A12 rows;
- interface/cache owners run only for the new imported/generic operation paths; and
- align-llm owns the real C6d preflight/error/result consumer without redefining the primitive.

The accepted platform floor is Align's current Linux x86_64/ARM64 and macOS Apple Silicon CI on
local ext4/tmpfs and APFS. Windows, NFS/FUSE/remote-filesystem atomicity, hostile concurrent device
replacement, Unicode normalization, filesystem sandboxing, directory enumeration, canonical path
strings, general metadata, writable-directory probing without creation, and multi-file publication
are non-goals. A later consumer needing directory values or arbitrary metadata must present its own
contract rather than widening this regular-file capability speculatively.

Public-contract propagation is exact: this ledger owns the detailed record;
`docs/impl/std-design/fs.md` owns the English implementation-facing design and
`docs/impl/std-design/ja/fs.md` its synchronized Japanese mirror; `draft.md` and
`docs/language-spec.md` own the authoritative and condensed language surfaces;
`docs/design-notes.md` owns the rationale; `docs/open-questions.md` owns the settled decision;
`docs/impl/20-runtime-abi-ledger.md` owns the proposed rows and later shipped inventory;
`docs/impl/07-roadmap.md` and `HANDOFF.md` own capability status; and
`docs/guide/13-std-os.md` plus `docs/guide/ja/13-std-os.md` own the synchronized end-user summary.
The sibling `align-llm/docs/align-requests.md` remains the real-client request/adoption register.
No other language mirror or persisted format carries this surface.

Implementation follows the repository's cross-cutting gate: author matrix-to-diff pass, the
`align-self-review` skill for Rust under `crates/`, narrow owner tests, one fresh full-diff review,
one consolidated valid-finding repair, exact-head preflight, PR/CI, and merge. The sibling request
register is updated with the merged surface and evidence, but remains uncommitted there until its
align-llm adoption PR.
