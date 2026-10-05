# Fixed storage for process observation paths

Plan112 reuses Linux stat input within each table/group scan. Its actual native
allocation owner still measures one Rust allocation per warmed observe call:
format!("/proc/{pid}/stat") constructs a temporary pathname. Replace only that
String with caller-local 22-byte storage and a borrowed Unix OsStr view.

The longest signed i32 spelling is -2147483648: six prefix bytes, eleven PID
bytes and five suffix bytes fit exactly. Format through the standard writer
for a mutable byte slice, then borrow only its written prefix. Use the same
decimal formatting as before, including zero and negative private probe inputs;
public candidate admission still requires positive PIDs. No UTF-8 validation,
CString, heap storage, descriptor ownership or returned path view is added.

| Closure axis | Implementation and acceptance |
| --- | --- |
| Formation and bounds | stat_path writes into one [u8;22] and returns an OsStr borrowed from it. stat_path_spelling_and_storage compares signed extrema, decimal-width boundaries and deterministic i32 bit patterns with independent format! output. Reused storage, exact length and untouched tail bytes are checked. |
| Native lifetime and errors | The observe stack buffer remains live through synchronous File::open; no path escapes. File open/read, disappeared ENOENT/ESRCH, other error mapping, the 65537-byte read sentinel and parser admission are unchanged. An impossible fixed-buffer formatting failure becomes Invalid before File::open. Existing process_table native owners retain those boundaries. |
| Ownership and callers | Table and group scans still own one input scratch and publish only complete results. File/Vec/rows Drop and numeric Copy snapshots are unchanged. There is no public move/nulling/replacement, FFI, ABI, IR, generic, interface/cache or compiler control-flow change. macOS keeps its native record path. Existing m11_process_live table/group source execution is reused. |
| Resource claim | The existing thread-local allocator plus a positive witness proves path formation allocates nothing. observation_native_reuse_allocations measures actual Linux observe calls: repeated same-PID reads within retained input capacity require zero Rust allocations. Restoring format! must fail this owner. Before/after counts concern Rust allocation operations, not syscalls, native C storage, whole-table allocations, throughput or RSS. |

This private storage change follows the existing native path and allocation
strategy. Author matrix/self-review and one fresh independent full-diff review
are sufficient; no new public contract, safety strategy or language mirror
change is required. Retain candidates, output arrays and native error behavior.

The native Linux ARM64 owner measured first-read allocation/reallocation counts
of 6 before and 5 after this change. Each of 32 subsequent reads of the same
live PID within retained input capacity measured 1 before and 0 after. The
remaining cold operations grow the input Vec. The path spelling/storage owner
also runs on native macOS; its production observer is unchanged.
