# Retained path component storage

## Consumer and unchanged contract

The bounded tree summary in plan 93 issues metadata and child-directory operations
with raw basenames. Each call currently builds one private NUL-delimited byte
copy plus a Rust `Vec<usize>` of component offsets. Even one basename requests
four offset slots on the supported 64-bit targets: 32 transient bytes and an
allocation beyond the native byte copy. Multi-component paths grow that vector.

Replace the offset vector with a borrowed double-ended iterator over the same
validated private byte copy. Plans 29, 36, 45 and 54 continue to own grammar,
validation precedence, errors, no-follow admission, races and cleanup. There is
no new signature, symbol, ABI, opaque owner, interface/cache format or language
rule. Only private Rust path storage and its native callers change.

## Representation and validation

Keep the existing checked positive length, address extent and len-plus-one
capacity checks; conditional UTF-8 validation and embedded-NUL rejection retain
their order. Allocate and fill the same private terminated byte vector before
component grammar validation. Validate every component before rewriting `/` to
NUL or performing filesystem I/O. No native call receives caller-owned bytes.

Store `component_start: usize`: the first component begins at zero for relative
paths, one for ordinary absolute roots, and the original input length for the
special roots `.` and `/`. The start is always within the byte vector, whose last
byte is the terminator. Components split that suffix inclusively at NUL, omit
terminator-only pieces, and yield native pointers into the unchanged owned vector.
Every yielded piece contains its terminator. The iterator allocates no component
table and performs no decoding or copying. The path owner remains live and
immutable through traversal and the final syscall.

Forward root traversal uses all components. Relative operations take the final
component with next_back before traversing the remaining parents. Empty relative
component sets remain Invalid before I/O; special roots retain zero traversal
components. `relative_access` checks real-ID search access on each current parent
before opening its successor, and on the final parent before the final O_PATH
operation. macOS relative-access refusal remains before filesystem observation.

This retains linear path processing. It removes the component-vector reservation;
it does not remove the existing native byte copy or claim a kernel zero-copy path.
Measure private requested storage and the actual tree-summary workload before and
after. Do not claim a latency improvement without results beyond baseline spread.

## Implementation closure matrix

| Invariant / affected path | Implementation and acceptance owner |
| --- | --- |
| Complete grammar precedes I/O, with unchanged validation order | abi_beneath_path_impl; existing fs_beneath validation and retained-tree malformed/native-output owners |
| Special roots, absolute and relative paths, raw bytes, component order and terminators | New private component owner crosses all forms, forward/backward mixed iteration and input immutability; existing root/native raw-name owners |
| The byte owner survives every native pointer use; no offset allocation | BeneathPath iterator and unchanged caller-local owner; private storage owner asserts exactly one Rust allocation during parsing and zero during iteration with alloc-count enabled, plus Linux/macOS native execution |
| Retained parent and final component are the same ordered names | fs_retained_tree parent, directory_open and native_open_beneath; existing retained_tree, fs_beneath race/type/single-link and driver filesystem owners |
| Exclusive create and private-temp root behavior | native_open_beneath/create_private_temp_dir_with; existing exclusive-race, collision, allocation-before-mutation and temp cleanup owners |
| Absolute removal excludes empty/special-root paths before I/O | native_remove_empty_dir; existing private-temp/non-directory/nonempty/race removal owners |
| Real-ID search/final access order and macOS refusal | relative_access; existing plan54 access ordering, identity/race and platform-refusal owners |
| Partial acquisition, error, replacement and Drop preserve cleanup | Existing RAII BeneathFd and native failure/race/descriptor-cycle owners; tree_summary repeated success and recursive error probes |
| Formation, generics, source nulling, controls, interface and whole/per-unit ABI | No compiler or owner change; existing fs_retained_tree/fs_observation_extensions driver owners and tree_summary whole/per-unit execution |
| Allocation/resource evidence and unchanged common workload | Private component-storage observations plus paired local tree-summary measurement; no benchmark is a correctness gate |

Record the baseline before changing production code. Complete the author matrix
pass and one independent inspection of this native pointer/traversal boundary
before implementation, then one fresh full-diff inspection on the candidate.
Reuse cumulative owners rather than duplicate the established race/failure matrix.

## Closure and local evidence

The independent pre-code inspection found no actionable issue. The author
matrix-to-diff pass covers all seven production component loops, all three parser
constructors and their adjacent native calls. The private byte owner and RAII
descriptor owners remain in scope through each syscall; the public runtime
exports, compiler formation, source nulling, interface/cache formats and ABI
are unchanged. No specification or library mirror changes are required.

Linux ARM64 (Docker, Rust 1.96.1, LLVM 22.1.8) storage observations recorded 32
offset bytes for one through three ordinary components before the change and
zero afterward. The private path shell shrank from 56 to 40 bytes; the terminated
byte capacity is unchanged. The allocation owner additionally rejects an extra
parser allocation or an allocating component iterator.

Paired production-runtime measurements execute the same tree summary over 64
directories and 1,024 regular files in an explicit executable tmpfs. Both
executables completed 45 walks with checked output: 1,089 entries, 65 directories,
1,024 regular files and 31,744 logical bytes. After four warm-ups, the 41-sample
medians were 1.803 ms before and 1.760 ms after, with median absolute deviations
of 0.045 and 0.055 ms. The difference is within observed spread; this capability
claims reduced transient storage, not a latency improvement. Timing precedes
counter-enabled builds and never enables a resource probe.

The native beneath-path, private-temp, empty-directory removal and retained-tree
owners exercise admission, raw names, races, access order and cleanup on Linux
and macOS. Driver retained-tree, observation-extension, existing filesystem and
tree-summary owners cover whole/per-unit compilation and native execution.
The retained-tree cleanup probe failed identically on the Linux baseline when
its inherited cwd was the read-only source mount: its native symlink creation
failed before reaching the cleanup witnesses. Run the compiled owner from
writable /tmp without changing its assertions or omitted-Drop negative controls.
Qualification uses child-scoped short canonical TMPDIR on macOS and executable
/tmp on Linux. Environment and baseline failure logs are retained separately
from review findings.
