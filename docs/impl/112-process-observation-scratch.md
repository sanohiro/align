# Reuse process observation input within one scan

Linux process.table and child.group_members read /proc/PID/stat once per
candidate. Plan105 removed temporary parser token storage; each input read
still builds a fresh Vec. A success-path probe of the existing read helper
over 32 actual /proc/self/stat files measured five Rust allocation operations
per read (one allocation and four reallocations, 307–308 input bytes). File
opening and path construction were outside that measurement.

Each scan owns one private ObservationScratch. Linux stores a Vec<u8>; macOS
stores no input buffer and retains its existing native record path. The Linux
read helper clears and reuses that Vec, returning a slice borrowed from it.
parse_stat converts the slice into numeric Copy snapshot fields before the
next read. No observation borrows the input, no buffer escapes the call, and
normal Rust Drop releases storage on success or any early error. No global or
thread-local cache is introduced.

| Closure axis | Implementation and acceptance |
| --- | --- |
| Construction, reuse and returned data | One default scratch before each table/group scan loop, passed into observe. observation_bytes clears at entry and returns only the current read. observation_input_reuse_and_failures crosses long, short, empty and error-then-success reads. Snapshot remains Copy; Rust borrowing prevents reuse while the input view is live. |
| Native read and errors | Preserve Read::take(65537).read_to_end, its Interrupted retry, vanished ENOENT/ESRCH → None, other native status propagation, and read-error precedence before the 65536-byte admission check. The input owner injects partial data, interruptions and terminal errors. The cap owner checks exact/over limit and proves no read beyond the sentinel. |
| Parsing, platform selection and public callers | parse_stat, macOS observation, PID collection/count/order and publish are unchanged. Existing process_table native owners and m11_process_live::retained_file_output_and_group_observation cover table/group use, malformed fields and whole/per-unit source checking. Each scan uses an independent owner; no cross-call capacity retention. |
| Error cleanup and output publication | Existing File/read ownership closes the file before parse. Scratch and collected rows drop on read/parse/publish error. Existing null output initialization and success-only publication stay unchanged. No pointer, FFI representation, Align move/nulling/replacement, generic, interface/cache or compiler control-flow path changes. |
| Allocation claim | observation_input_reuse_allocations counts the actual helper with a positive allocation witness, warms storage once, then requires no Rust allocations for repeated bounded inputs. Linux observation_native_reuse_allocations counts actual repeated observe calls with reusable input; only unchanged path construction may allocate. Restoring a fresh Vec must fail the owners. Compare native Linux counts before/after; native macOS common owners remain applicable. |

This is a private storage refinement of plans49/50/105, not a new API or safety
strategy. Author-side matrix extraction and one independent full-diff review
close it. Existing parser/ABI owners are reused. No throughput, RSS, whole-table
zero-allocation or GPU-resource claim is made; PID, pathname, row and returned
Align-array storage remain. No normative language or library mirror changes
are required.

Linux ARM64's actual observe owner measured six allocation/reallocation
operations on the first read and one on each of 32 subsequent reads. Restoring
fresh input storage measured six on every read and failed both allocation
owners. The remaining operation builds the procfs pathname. The injected
reader owners also pass on native macOS, whose production observation keeps
using its native stack records.
