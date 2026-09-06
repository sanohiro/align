# `std.fs` private temporary-directory lifecycle

Status: **IMPLEMENTED for align-llm Request 56; release pending.**

This document is the authoritative public-contract ledger and implementation closure matrix for:

```text
fs.create_private_temp_dir(prefix: str) -> Result<string, Error>
fs.remove_empty_dir(path: str) -> Result<(), Error>
```

The pair supplies one application-owned staging root for consumers that must copy verified bytes
before passing a pathname to a native library. It is not a general temporary-file framework,
recursive removal API, directory handle, or sandbox.

The capability exceeds roughly 1,000 changed hand-written lines because its two public operations
form one strict lifecycle boundary across the HIR, replay validator, MIR producer/status paths,
LLVM ABI inventory, Linux/macOS runtime, bilingual contract, and adversarial owner matrix. Splitting
that dormant producer-to-cleanup chain would leave no useful stable consumer and would duplicate
the owned-string, descriptor-cleanup, ABI, whole/per-unit, and platform proof. One atomic capability
therefore has lower integration risk than separately mergeable constructor and remover PRs.

## 1. Public-contract ledger

| Surface | Exact inputs and defaults | Result and deterministic errors | Ownership, lifetime, allocation, and cleanup | Compiler/runtime owner | Identity, prerequisite, and acceptance |
| --- | --- | --- | --- | --- | --- |
| `fs.create_private_temp_dir(prefix: str)` | Exactly one positional `str`; no path, suffix, mode, option, default, or environment input. `prefix` is 1..=64 ASCII bytes: the first is `A-Z`, `a-z`, or `0-9`; every later byte is alphanumeric, `_`, or `-`. Invalid UTF-8, null-positive, embedded NUL, empty, overlong, or other bytes are `Invalid` before platform-root lookup, randomness, allocation, or filesystem mutation. | Linux starts from fixed `/tmp`; macOS starts from `confstr(_CS_DARWIN_USER_TEMP_DIR)`, accepting its required terminal slash. The platform-owned spelling is canonicalized before strict validation so macOS `/var` compatibility symlinks and the terminal slash cannot make the returned path unusable by retained-root APIs. The canonical root must be absolute and strict. The leaf is exactly `prefix`, `-`, and 32 lowercase hexadecimal digits from 128 fresh OS-CSPRNG bits. One `mkdirat(..., 0700)` atomically claims the absent leaf. `EEXIST` draws a fresh suffix and retries, at most 128 attempts; exhaustion retains native `EEXIST` as `Error.Code`. Missing/denied/invalid and other native failures use the fixed mapping. Success returns the canonical absolute path. | The prefix is borrowed only for the call. Root and name storage, the output allocation, random bytes, and retained directory descriptors have one owner each. Output storage is fully allocated before the first `mkdirat`, so terminal OOM cannot leave a directory. The output slot is canonical `{null,0}` until successful creation transfers the allocation. Every recoverable failure frees it and closes each descriptor exactly once. | One distinct HIR expression, MIR rvalue with owned-string out slot, runtime key, A08 ABI symbol, checked-HIR/replay path, and LLVM lowering. The native runtime canonicalizes only the platform-owned root, then uses retained no-follow traversal, fallible OS randomness, and descriptor-relative `mkdirat`. | The existing nominal `string` identity and Drop path remain unchanged. The returned absolute string names the atomically claimed directory and is directly consumable by retained-root APIs. Linux and macOS owners cover the real platform spelling, sequential/concurrent uniqueness, forced first collision, mode, allocation-before-mutation, failure cleanup, whole/per-unit/imported/generic formation, and composition with the retained-root file constructors. |
| `fs.remove_empty_dir(path: str)` | Exactly one positional `str`; no recursive, force, missing-ok, root, identity, or environment option. The lifecycle input is an absolute strict path with nonempty components and no `.`, `..`, empty component, or trailing slash. Other shapes are `Invalid` before filesystem access. | Retain and revalidate every ancestor without following symlinks, observe the final entry without following it, require and open the same directory identity, then issue one descriptor-relative `unlinkat(..., AT_REMOVEDIR)` while the parent and final descriptors remain live. It succeeds only for the empty directory named at that syscall. Initial absence is `NotFound`; a symlink or non-directory is `Invalid`; denied is `Denied`; nonempty retains native `ENOTEMPTY`/`EEXIST` as `Error.Code`; other failures use the fixed mapping. A disappearance or type/identity substitution detected through final descriptor revalidation is `Invalid`; the final syscall limitation below applies afterward. | The path is borrowed only for the call. Private path/component copies and every retained descriptor close exactly once. No path or resource is returned. Failure never recursively removes a child, follows a symlink, removes a regular/special file, or retries after a semantic rejection. | One distinct HIR expression, MIR status rvalue, runtime key, A04 ABI symbol, checked-HIR/replay path, and LLVM status-to-`Result<(), Error>` lowering. Native traversal reuses the retained-root directory helpers; final removal is `unlinkat` with `AT_REMOVEDIR`. | The API is the explicit cleanup companion for paths returned by `create_private_temp_dir`; it does not authenticate arbitrary path provenance. Barrier owners cover replacements through final descriptor identity revalidation and final-syscall rejection of non-directory/nonempty replacements, without claiming descriptor-bound deletion afterward. align-llm owns child removal order and poisoned-tree retention after an unload-unsafe native side effect. |

Both operations are Impure. They add no global registry, cleanup-at-exit hook, ambient environment
override, current-directory mutation, subprocess, recursive walk, or public mode/descriptor/inode
surface. No benchmark is required because the contract makes safety, ownership, and bounded-work
promises rather than a latency or throughput promise.

## 2. Creation algorithm and bounds

The native constructor performs this sequence:

1. reject a null output slot, otherwise store canonical `{null, 0}`;
2. validate the complete prefix and its exact grammar;
3. obtain the platform-owned temporary root without reading `TMPDIR`, `TMP`, `TEMP`, XDG state,
   cwd, configuration files, or an application path;
4. canonicalize that platform-owned root, trim the macOS provider's terminal slash as part of that
   operation, validate the canonical result as an absolute strict path, retain `/`, and walk every
   canonical component with no-follow observation/open/identity revalidation;
5. checked-compute and allocate the exact returned absolute-path buffer before directory mutation;
6. draw 128 OS-CSPRNG bits, encode the exact leaf on a fixed stack buffer, and copy the candidate
   absolute path into the still-private output allocation;
7. call `mkdirat` on the retained root descriptor with mode `0700`;
8. on `EEXIST`, repeat steps 6–7 with fresh randomness up to the 128-attempt bound; and
9. on success, publish the already-complete owned string and close the root descriptor.

Linux randomness is a bounded `getrandom` loop that retries only `EINTR`, maps a negative return
through the fixed native-error mapping, and maps an impossible zero-length progress return to the
stable `Error.Invalid` rather than consulting stale `errno`. macOS uses `getentropy` for the
16-byte request and maps failure through the fixed native-error mapping. This fallible constructor does not reuse the aborting
`crypto.random` policy: failure to name a staging directory is an ordinary observable filesystem
construction failure. The existing cryptographic APIs remain unchanged.

`mkdirat` is the sole claim operation. Mode `0700` combined with the process umask can only remove
permissions, never grant group or other access. The runtime does not call `chmod`, reuse an existing
directory, create parents, resolve a caller path, or perform a check-then-create sequence.

## 3. Removal, races, and threat boundary

`remove_empty_dir` validates an absolute path and walks its parent using the same no-follow,
device/inode-revalidating directory operations as the retained-root constructors. At the final
parent it performs `fstatat(..., AT_SYMLINK_NOFOLLOW)`, requires a directory, opens that directory
with `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`, and requires the opened descriptor to have the same
device/inode identity. Only then does it call `unlinkat(parent, leaf, AT_REMOVEDIR)`.

The kernel's one removal call is the linearization point for the final name, directory type, and
emptiness. A symlink, file, FIFO, socket, device, or nonempty-directory substitution at that point
is not removed. A replacement before final open or descriptor revalidation is rejected as
`Invalid`. Holding the parent descriptor prevents ancestor retargeting; holding the final
descriptor preserves the observed object's lifetime through the call.

POSIX supplies no portable Linux/macOS operation that unlinks an already-open directory *by that
descriptor*. Therefore an empty-directory substitution after the last identity revalidation but
before `unlinkat` is indistinguishable to this path-only signature and may be the empty directory
removed at the syscall linearization point. For the intended path returned by
`create_private_temp_dir`, the platform temporary root plus mode `0700` excludes other users and
accidental application sharing; a hostile process with the same OS identity was explicitly outside
Request 56's threat boundary. The API makes no stronger claim for arbitrary paths in a shared,
non-sticky parent. Claiming descriptor-bound deletion would require a different public capability
than the requested `path: str` surface.

This limitation is normative and must remain visible in `draft.md`, the language digest, and both
`std.fs` design mirrors. A test hook may pause before final descriptor revalidation and before the
single `unlinkat`; it must not be represented as a kernel guarantee beyond those exact points.

## 4. Compiler, runtime, ABI, and identity

The semantic and MIR records are distinct:

```text
CreatePrivateTempDir { prefix: Expr }
RemoveEmptyDir { path: Expr }

FsCreatePrivateTempDir { prefix: Operand, out: Slot }
FsRemoveEmptyDir { path: Operand }
```

`check_method_call` recognizes both spellings only under `import std.fs`, validates exact arity,
checks the one `str`-coercible operand, borrows owned strings without moving them, and marks both
Impure. The first returns `Result<string, Error>` and the second `Result<(), Error>`. Every HIR
visitor, effect/escape/move walk, generic replay, checked-HIR validator, MIR printer/fingerprint,
producer validator, and LLVM lowering classifies both explicitly.

The runtime adds two keys and symbols using existing ABI shapes:

```text
FsCreatePrivateTempDir
align_rt_fs_create_private_temp_dir(
    prefix_ptr: ptr, prefix_len: i64, out_path: ptr,
) -> i32

FsRemoveEmptyDir
align_rt_fs_remove_empty_dir(path_ptr: ptr, path_len: i64) -> i32
```

The constructor uses A08 and the existing owned-string out-slot/result CFG. LLVM zeros the out slot
before the call; status zero moves the string into `Ok`, while failure keeps the slot canonical and
constructs `Err`. Removal uses A04 and the existing status-to-unit-result CFG. The exact ABI
declaration golden, runtime key/symbol bijection, native export inventory, rt-LTO inventory, and
whole/per-unit declarations update atomically. The registry moves from 356 keyed / 374 base / 385
maximum-probe rows to 358 / 376 / 387; no ABI shape or optional probe is added.

## 5. Implementation closure matrix

One parameterized owner may close multiple cells when its witness directly discriminates the
missing operation. Implementation review begins only after every applicable row points to code and
passing evidence.

| Axis | Implementation invariant | Exact owner evidence |
| --- | --- | --- |
| Formation and effects | Imported exact names, one argument, source-order `str` coercion, Impure results, exact `Result<string, Error>` / `Result<(), Error>`, no aliases or options. | `m9_fs::private_temp_directory_formation_and_per_unit_generic_paths`, whole-program and imported generic checks. |
| Prefix and path validation | Prefix grammar and removal's absolute strict grammar reject every malformed and multi-invalid shape before root/random/filesystem work. | Runtime direct-ABI table plus driver diagnostics; failpoints prove no root/random/create call for an invalid prefix and no traversal for an invalid removal path. |
| Platform root and randomness | Linux `/tmp`; macOS `_CS_DARWIN_USER_TEMP_DIR`; no environment lookup. Canonicalize only that platform-owned spelling before strict no-follow traversal and returning the root, including macOS's real terminal-slash and `/var`-compatibility form. Exactly 128 random bits and lowercase hex; `EINTR` retry, zero-progress `Invalid`, negative native-error mapping, and 128-attempt collision cap. | Platform runtime owner with injected root/random provider, real macOS spelling, first-name collision, terminal collision, short/zero/error random returns, retained-root composition, and source audit forbidding environment reads. |
| Atomic creation and permissions | One `mkdirat` claims absence with `0700`; occupied file/dir/symlink is unchanged; sequential and concurrent results differ; no parents or reuse. | Linux/macOS runtime concurrent/collision owner, `fstat` mode owner, and pre-existing-type matrix. |
| Creation ownership and cleanup | Output allocation precedes mutation; failure publishes canonical empty output and leaves no created directory; every buffer/fd closes exactly once. | Allocation/fd counters and failpoint sweep before and after each acquisition; direct null-output owner. |
| Retained removal traversal | Every ancestor and final directory is observed/opened no-follow and device/inode revalidated; initial missing/type/permission and post-observation races preserve precedence. | Parameterized root/intermediate/final symlink/type matrix and barrier replacements before directory open/revalidation. |
| Empty-only removal | Exactly one `unlinkat(..., AT_REMOVEDIR)` is the deletion point; empty succeeds, nonempty/file/symlink/FIFO/socket/device/missing/denied remain and return the fixed mapped error. | Direct runtime type/error matrix with before/after identity checks and syscall counter. |
| Removal race boundary | Parent and final descriptors remain live through unlink; replacement through final revalidation rejects. The documented same-identity post-revalidation empty replacement is not overstated as descriptor-bound deletion. | Barrier-controlled ancestor/final replacement owners plus a contract assertion naming the final-syscall linearization limitation. |
| Owned result lifecycle | The returned `string` moves through direct return, binding, replacement, `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, and Drop without double-free or leak. | Existing generic owned-string matrix plus a private-temp whole/per-unit control-flow fixture and allocation parity. |
| HIR and replay | Both records validate operand/result/effect exactly; malformed nodes fail closed; clone/depth/replay and compiler identity are exhaustive. | `align_mir::validate_hir_tests::hir_body_validator_native`, variant tripwire, imported generic replay, and edit/revert identity owner. |
| MIR, LLVM, and ABI | Distinct MIR spellings/keys use A08/A04 with exact argument/out-slot order and existing Result CFGs. | `m9_fs::private_temp_mir_and_llvm_abi_are_distinct`, declaration golden, registry/export parity, and source-export inventory. |
| Composition and platform | Created root accepts `create_exclusive_beneath`; completed child reopens through `open_beneath_single_link`; child removal then root removal succeeds on Linux and macOS. | Whole-program executable owner on both required platforms; align-llm later owns the complete bundle lifecycle. |

## 6. Non-goals

There is no caller-selected root, recursive `mkdir`, recursive delete, missing-ok mode, returned
directory descriptor, automatic child cleanup, exit hook, quarantine manager, file listing,
metadata API, Windows fallback, filesystem classifier, durability promise, or protection against a
hostile process with the same OS identity. `fs.remove` remains file-only and unchanged. Request 53's
general directory creation/listing/type predicates remain separate.

## 7. Pre-implementation review closure

The independent review of candidate `020c4663` found one P1 and two P2 issues. They were closed as
one boundary correction before implementation:

| Finding | Class-wide correction |
|---|---|
| P1: raw macOS `_CS_DARWIN_USER_TEMP_DIR` carries a terminal slash rejected by strict grammar. | Canonicalize only the platform-owned root before strict validation/traversal, cover the real terminal-slash spelling, and return the canonical path. The same correction closes the adjacent macOS `/var` compatibility-symlink case so retained-root consumers do not reject the constructor's own output. |
| P2: the ledger promised every post-observation substitution was `Invalid` despite the documented final-syscall race. | Scope `Invalid` and unchanged-replacement evidence through final descriptor revalidation and to the kernel's rejection of non-directory/nonempty final substitutions; retain the explicit path-only empty-directory limitation after that point. |
| P2: zero-progress `getrandom` has no errno to map. | Assign the exact stable result `Error.Invalid`; only a negative result reads and maps native errno. |

No HIR/MIR/ABI or owned-string strategy finding was reported. The targeted finding-closure check
reviews only these corrected cells; it is not a second full-diff review.

## 8. Implementation review closure

The one independent full-diff review of implementation candidate `1d0d33b9` reported no P0 or P1
finding. Four P2 findings were closed in one class-wide correction commit; the unchanged diff was
not sent through repeated full review.

| Finding | Finding-to-fix closure |
|---|---|
| Output overlap returned `Invalid` without restoring the promised canonical empty result. | Zero the valid output slot before returning from the overlap branch and initialize its direct owner with stale nonzero fields so both fields are observed cleared. |
| macOS `confstr` failure/truncation consulted stale `errno` and did not retry a grown size. | Assign stable `Invalid` to a zero return, retry when the provider reports a larger requirement, and cover canonical root spellings through the extracted platform-root validator. |
| Random-progress, occupied-type, allocation-order, removal-type/error, and injected cleanup cells lacked direct runtime owners. | Add short-read/EINTR/zero/error random owners, file/directory/symlink collision owners, allocation-before-mutation/free parity, missing/denied/FIFO/socket/device removal, and final-observe/open/revalidate descriptor failpoint coverage. |
| The active ABI status, exact A04/A08 membership table, and machine-gate inventory retained pre-Request-55/56 totals. | Record both new symbols in their reused shapes and update the ledger plus `test-runtime-abi-exports.sh` current assertions to 358 keyed, 376 base, 383 alloc-count, 380 par-map, and 387 maximum records. Historical capability snapshots remain historical. |
