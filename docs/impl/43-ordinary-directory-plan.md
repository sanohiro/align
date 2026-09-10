# Ordinary directory operations

Status: exact F1/R53 contract, selected by plan 40. Retained byte paths and
raw enumeration remain the separate R64 capability. Existing read_dir is unchanged.

## Public contract ledger

| Surface | Contract |
| --- | --- |
| `fs.create_dir(path: str) -> Result<(), Error>` | Requires import std.fs, one required argument, Impure. Create exactly one directory. Unix permission request 0777 filtered by current process umask; do not read/change umask or create missing ancestors. An existing entry, including an existing directory or symlink, is an error. |
| `fs.is_dir(path: str) -> Result<bool, Error>` | Requires import std.fs, one required argument, Impure. Follow ordinary path/symlink resolution and perform one metadata observation. Successful directory observation returns true; successful other-kind observation returns false. Missing, denied, non-directory ancestor, symlink-loop and other query failures remain Error, never false. It proves neither write permission nor future identity. |
| Path domain | Borrow the str or auto-borrowed string only for the call, retain nothing. Relative paths resolve from the process cwd; absolute paths use their root. Ordinary OS path grammar, including dot/dot-dot and repeated/trailing separators, applies. Reject empty input, embedded NUL or invalid UTF-8 before filesystem I/O. No normalization, environment expansion or current-directory mutation. |
| Errors and ownership | Existing fixed errno mapping, including existing-entry Code(errno); no new Error variants. No owned output or region dependency. Native path marshalling may allocate O(path bytes); OOM follows existing hard-error policy. No filesystem observation cache, hidden recursion or retained authority. |

On existing 64-bit Linux/macOS targets use the existing std::fs/native platform
boundary. Native API pointer validation precedes path validation; complete path
validation precedes the single filesystem operation. Query and creation are
separate observations: no race-free check-then-create or writability promise.

## Native/compiler closure

Two one-child HIR records, FsCreateDir and FsIsDir, each
hold one checked str expression. Their result types are exact Result<Unit,Error>
and Result<Bool,Error>. Receiver/path evaluation and any early exit occur once
before native invocation. Both are Impure and nonretaining.

MIR FsCreateDir has one str operand and returns i32 status. RuntimeKey FsCreateDir
maps to align_rt_fs_create_dir, A04 i32(ptr,i64). MIR FsIsDir has one str operand
and an exclusive compiler-owned bool scratch; returns i32 status. RuntimeKey
FsIsDir maps to align_rt_fs_is_dir, A08 i32(ptr,i64,ptr). The scratch is one i8
byte, initialized false before observation; only success publishes bool 0/1.
Error status uses existing Result construction and never fabricates a bool.
Both shapes have no extra LLVM attributes.

Malformed native path pointers (null with positive length, negative length,
isize overflow, address extent overflow) reject as Invalid before dereference.
For is_dir first validate the writable nonnull one-byte output address/extent;
then validate path length/address extent and reject scratch/path overlap without
writing. Only after proving disjointness initialize output false, validate path
contents, and invoke the OS. Invalid output, invalid path address/extent, and
overlap leave the output untouched; later content/OS failures leave it false. Dangling/undersized pointers remain ABI
precondition violations. Complete path validation rejects zero length, NUL and
invalid UTF-8 before marshalling/allocation/OS access. No native owner escapes.

| Closure axis | Exact owner |
| --- | --- |
| Formation, imports, result and path types | align_driver m9_fs::ordinary_directory_formation; borrow string arguments, invalid arity/types/import. |
| Actual behavior and errors | m9_fs::ordinary_directory_operations: create, existing dir/file/symlink, missing ancestor, regular file false, directory true, followed symlink, missing/broken link/loop errors, NUL/empty refusal. Explicit non-directory-ancestor errors and OS trailing-separator behavior. Permission refusal in an isolated unprivileged child where available; no root-dependent assertion. |
| Permissions | m9_fs::ordinary_directory_umask: isolated child sets a known umask, creates in a controlled parent without default ACLs, verifies requested 0777 filtered mode and verifies the call preserved umask. Native filesystem ACL policy can additionally restrict permissions; it is not overridden. |
| Control, purity and provenance | ordinary_directory_formation rejects impure parallel use; ordinary_directory_operations exercises if/match/else/?/map_err/loop and early return in path; whole/per-unit owned-string argument and arena transport. No new ownership class, collection carrier, source nulling or Drop behavior. |
| Native malformed input and output | align_runtime::fs_directory::tests::invalid_paths_before_io and query_out_contract: negative/null/overflow/NUL/UTF-8, overlap with untouched output; initialized false output on later content/OS errors. |
| HIR/MIR trust | align_mir::validate_hir_tests::ordinary_directory_records_reject_malformed_types; LLVM ordinary_directory_mir_gate: wrong operand/result/out shape rejects across public lowering/emission entrypoints. |
| ABI/cache | runtime declaration golden, ordered RuntimeKey inventory, scripts/test-runtime-abi-exports.sh. Existing no-new-type canonical/interface format and compiler build fingerprint; m9_fs whole/per-unit parity. |

No new type/scalar tags, native owner, persisted format or generic serialization
shape. One complete F1 capability delivers both independently useful general
operations. No benchmark: there is no throughput, latency or resource bound
beyond path-sized marshalling. Propagate the contract to draft.md, language-spec,
design-notes, Settled, fs design EN/JA, HIR/runtime ledgers and plan40. Record
status once and write the consumer request answer uncommitted.

Author ledger-to-prose pass: follows ordinary symlinks consistently; false is
only a successful non-directory observation, never failure or write denial.
Path grammar and all invalid-input precedence are explicit. Scratch is a bool,
not a pointer-sized host owner. No retained-F2 proof is claimed or consumed.

Independent review (2026-09-10): two P2 findings fixed before coding. Output/path overlap now rejects before any write and the matrix distinguishes untouched versus false error outputs. The isolated permission owner pins mode/umask preservation, non-directory ancestors and trailing separators.

## Implementation closure

Native operations share one checked pointer/length and strict text decoder;
create uses std::fs::create_dir and query uses followed metadata with the existing
errno mapper. Query checks output and disjoint path extent before writing false.
No result owner or native allocation survives a call. HIR/MIR records preserve
single path evaluation and early-exit guards; native certification checks str,
i32 and exclusive bool scratch independently of caller assertions. Child/replay,
effect and storage sweeps explicitly cover both variants.

Driver owners cover whole/per-unit behavior, string borrowing and arena calls,
all ordinary path/error cases, all control forms and skipped native invocation
after an early return. Isolated generated executables verify umask preservation
and mode 0700 under umask 0077; another child drops root effective identity if
necessary before checking both Denied paths. A C wrapper pins platform mode_t
conversion instead of assuming its width. Native malformed path/out tests and
all HIR/MIR entrypoint rejection owners close the remaining matrix cells.
Registry totals, declaration goldens and export verification advance together.
No new ownership, serialization shape, resource claim or benchmark is introduced.
