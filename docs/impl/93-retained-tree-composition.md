# Bounded retained-directory composition

This completes plan 51's filesystem assessment through an Align-owned executable
example. The existing native and language contracts remain plan 45 and plan 54.
No filesystem signature, ABI, owner, error model or allocation policy changes.

## Evidence and disposition

The multimodal reference uses a flat private artifact directory. Read-only
inspection of the external consumer's `prompt_tree_snapshot.align` and
`prompt_workspace_usage.align` also confirms that bounded recursive traversal
already composes retained directories, cursors, metadata and ordinary records.
Their manifests, hashes, deadlines and workspace policy remain consumer-owned.
No consumer source or adoption state is changed by this assessment.

`examples/tree_summary.align` supplies the general usage example: count entries by
kind and sum regular-file logical sizes under explicit entry and depth limits.
Use raw entry bytes directly; retain each child relative to its parent; never
open a regular file, symlink or special entry. Sorting, full-tree materialization,
recursive deletion, snapshot certification and inode deduplication are excluded.
The current cursor already copies each native basename directly into the final
owned array, without an intermediate byte vector. No cursor optimization is
selected without a measured remaining cost.

## Example contract and implementation checklist

The CLI uses the existing `std.cli`: `--root` defaults to `.`, `--max-depth` to
32, `--max-entries` to 8192, and `--help` to false. Help returns existing usage
after successful parsing, before numeric validation or filesystem access.
Otherwise validate depth in 0..64 and entries in 1..1048576 before opening the
root. Root spelling and errors use unchanged `fs.open_directory` admission.
The root counts as one entry and one directory at depth zero. A child directory
at the selected maximum depth is an error, including an empty child. Exactly
the selected entry limit succeeds; a further entry fails before its metadata
query. Native errors propagate. Invalid limits, negative regular-file sizes,
checked byte-sum overflow and observed/opened child identity disagreement are
`Error.Invalid`. No partial summary is printed on failure.

Success prints six label/value pairs in fixed order: entries, directories,
regular_files, symlinks, other, logical_bytes. Every count and sum is `i64`.
Hard-linked names count separately; bytes are logical size, not disk usage.
Concurrent mutation can change observations or cause errors. The result is not
a stable snapshot, a deadline guarantee or a sandbox against mount traversal.
Entry and depth limits bound application work and recursion, not syscall time.

| Invariant | Implementation | Owner acceptance |
| --- | --- | --- |
| Existing CLI validation and explicit defaults | main, named command/parsed locals | Actual example execution with defaults, overrides, help, malformed flags and invalid limits |
| Entry budget shared across recursion; root included | Copy Counts passed and returned by walk | Exact/one-below limit over a nested tree; empty root with limit one |
| Explicit recursion bound | remaining depth checked before child open | Depth zero and maximum-depth boundary, including an empty nested directory |
| Byte names and no-follow kind dispatch | cursor.next, metadata_at, open_dir | Linux raw names, Unicode names, regular files, directory and dangling/external symlinks; special entry counted without opening |
| Child identity check across separate observations | opened descriptor metadata compared with observed device/inode | Author inspection; existing plan 45 owns native admission/race injection |
| Logical size and checked aggregation | nonnegative size and checked_add | Known regular sizes and hard-linked names; overflow discriminated by a direct helper owner |
| Named owners close on recursion, return and ? | existing directory/cursor/name Move cleanup | Repeated successful and failed walks with native descriptor and requested-live-byte oracles |
| Existing whole/per-unit lowering | unchanged source checked in both modes | Example source formation and a native per-unit walk |

All per-entry owned names and native path copies are visible through the existing
operations. Traversal retains directory/cursor owners along the active path;
there is no whole-tree collection. No new speed, RSS or allocator-count promise
is made. Benchmarking is therefore not a correctness gate.

The four `align_driver --test tree_summary` owners passed on native macOS and
Linux ARM64. The Linux fixture creates actual non-UTF-8 names. Both platforms
exercise whole-program and per-unit execution, exact and exceeded recursive
budgets, empty-child depth rejection, CLI precedence, checked byte arithmetic,
and repeated success/error cleanup. The cleanup probe verifies a nonzero native
allocation count before accepting its requested-live-byte oracle.

## Continuation

Assess only demonstrated remaining CLI composition gaps next. The example's
single command already uses existing flags and needs no parser extension.
Subcommand dispatch in the multimodal reference remains ordinary application
control flow until repeated consumer work establishes a missing general surface.
