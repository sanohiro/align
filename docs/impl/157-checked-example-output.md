# Checked stdout completion in executable examples

The retained-tree summary and io.copy count use builtin print, which deliberately
ignores stdout errors. The tree, digest, HTTP fetch, SSE watch and CSV summary
help paths do the same. Read-only stdout reproduces success without delivered
output in all seven paths. Use the existing std.io writer and propagate its
Result; this changes example completion, not builtin print or any library API.

| Obligation | Implementation and existing owner |
| --- | --- |
| Exact successful output | Keep tree labels/count order, the copy count, and the usage text plus print's additional LF. Compose the two numeric reports with an explicit template string, then use checked unbuffered writes. Existing tree_summary/file_copy_examples owners retain byte goldens; existing help owners retain flag/default checks. |
| Error delivery and order | Require the native write error's normal exit and exact error report with read-only stdout. Help still follows successful parsing and precedes numeric validation, filesystem/input/network access. Tree traversal and copy/flush still complete before reporting. Invalid input or traversal errors precede report-output failure. Extend the existing tree, copy, digest, HTTP/SSE and CSV owners with this cross-path control. |
| Existing side effects | A failed copy-count report leaves the completed destination, including io_copy's appended LF; input remains unchanged. An output failure may leave a stdout prefix. Tree admission/traversal errors still emit no summary. No rollback, atomic publication, durable-output or new resource promise. |
| Source modes and cleanup | Existing Move owners and ? cleanup apply to the named report/usage/writer. Whole/per-unit execution owns the complete tree output and CSV paths; existing actual-source owners cover the remaining examples. Reuse bounded fixture ownership and native errno constants; no new shared harness or test binary. |
| Documentation | Update the owning example plans and affected English/Japanese guide descriptions. The language spec and library design remain unchanged. Record this capability once in HANDOFF. |

This is a source composition fix using the settled writer contract. It changes
no compiler layer, FFI/ABI, source type, native allocation policy or persisted format.
There is no performance claim or benchmark. The focused executable owners,
one author obligation-to-diff pass, independent preflight review and normal
final-SHA verification close the change.

## Qualification

All 20 focused owners pass on macOS and Linux ARM64. Read-only stdout now
returns the native EBADF code and exact standard error report for all seven
previously successful refusal paths. Existing complete copy, recursive traversal,
CLI precedence, input/transport/error, source cleanup and CSV whole/per-unit
owners continue to pass. The new complete tree main owner runs in both modes.
Normal-output comparison against the retained baseline executables confirms
byte-for-byte identity for all seven paths, including the extra help LF.
