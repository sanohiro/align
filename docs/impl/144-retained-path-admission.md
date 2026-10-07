# Validate retained-path grammar before its private copy

Status: implemented. Independent of deferred K1/plan61 and the parked
aggregate-provenance repair. Plan94 removed the component-offset allocation;
this follow-up avoids its remaining private byte allocation for invalid grammar.

## Scope and resource contract

`abi_beneath_path_impl` already checks native extent, optional UTF-8 and NUL before
copying. Its component grammar then scans the borrowed input, not the copy. Move
that existing grammar scan before acquisition of the private terminated bytes.
No extra input scan, accepted spelling, normalization, error status, native call,
symbol, signature, compiler record, effect or language admission is introduced.

| Surface | Exact admission, result and ownership | Owner, identity and acceptance |
| --- | --- | --- |
| Private `abi_beneath_path_impl(ptr: *const u8, len: i64, root: bool, utf8: bool) -> Result<BeneathPath, i32>` | Existing unsafe readable immutable byte-range precondition. In order: positive representable length/non-null/checked address extent, checked len+1 capacity, conditional UTF-8 and NUL, complete root or relative component grammar, then one private len+1 byte allocation/copy. Invalid input returns AL_INVALID without this parser's allocation. Valid input returns the existing owning Vec plus component_start/absolute metadata; copied bytes live through every component pointer use and Drop unchanged. OOM remains terminal when an admitted path actually allocates. Flags have no new meaning/default. | Runtime owns validation and storage; no new persistent/cache/ABI identity. Existing compiler/runtime content identity invalidates artifacts normally. Native actual-allocation owner and existing filesystem owners run on macOS/Linux. |
| Existing retained-root and retained-directory callers | Preserve every output-slot, handle, option and mode check outside the parser. Root still completes before relative starts; all paths complete before traversal. An earlier admitted path may already own its copy when a later path rejects. This is a per-parser resource guarantee, not allocation-free failure for a complete multi-path operation. Public signatures, errors, ownership, input lifetime, native side effects and platform refusals remain the contracts of plans29/34/36/45/54. | No new package surface or consumer migration. Reuse native same-final, race, no-follow, raw-name, access-order, allocation-before-mutation and cleanup owners plus whole/per-unit filesystem consumers. |

Root alone accepts `.` and `/`; they still store their exact source bytes plus
one terminator, set component_start to the input length, and yield no components.
Ordinary roots may start with one slash. Strict relative paths never do. Every
ordinary component is nonempty and neither `.` nor `..`; trailing slash is invalid.
Backslash and other non-NUL bytes retain their meaning, including arbitrary raw
non-UTF-8 names when utf8 is false. Only ordinary paths rewrite separators in the
private copy after validation. The caller's input is never changed or passed to
filesystem syscalls. Do not combine root and relative admission into a new pass.

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Complete admission before allocation | Keep all existing extent/text checks and their order. Compute validated component_start/absolute before Vec::with_capacity. A parameterized native owner crosses root/relative and UTF-8/raw modes with empty, special roots, absolute/relative names, Unicode/raw bytes, NUL, repeated/trailing separators and early/late dot components. Observe actual thread-local allocator calls/requested bytes with a positive witness; every private invalid case has zero, each valid case has exactly one len+1 allocation. No process-global allocation probe is activated or needed by this parser. |
| Borrowed input and owned native representation | The same stable immutable input is used for validation and copying. Existing fs_beneath_component_storage pins component order, mixed forward/back iteration, pointer containment/terminators and caller immutability. Extend it or its focused sibling for exact special-root bytes and post-input-mutation independence. Invalid inputs produce no path owner or native pointer. |
| Validation/error ordering and malformed metadata | Existing fs_beneath_validation_types_and_same_final_matrix retains output/root/relative precedence and null outputs. Add representable negative/null/overflow metadata controls to the private admission owner without dereferencing deliberately invalid extents. Only valid pointers/ranges may reach content scanning. Existing earlier handle/mode/platform checks remain untouched. |
| Construction, return, transfer, replacement and Drop | BeneathPath remains one local Vec owner; iterator/native caller lifetimes and RAII BeneathFd do not change. Existing native descriptor-cycle, partial-acquisition, root/ancestor race, exclusive creation, private-temp/removal and retained-tree owners close acquisition/failure/Drop. No new cleanup path or half-published state. |
| Whole/per-unit, control, generic/interface/ABI | No HIR/MIR/compiler representation or new source operation. Existing fs_retained_tree, fs_observation_extensions, tree_summary and m9_fs driver targets retain source borrowing, Result/control and owner cleanup. No new driver fixture/harness. |
| Resource discrimination | A long valid first component with a final `/..` shows the removed input-length-plus-one allocation without a timing claim. Restoring allocation before grammar must fail the actual zero-allocation assertion. Valid paths retain one allocation and identical owned bytes/components; the change claims no throughput/RSS or filesystem-work reduction. |
| Platform and test lifecycle | Both supported native OS implementations share this parser. Reuse the shared Linux Cargo cache; run Linux native binaries from writable /tmp, as plan94 requires. Existing fixture guards own temporary directories, descriptors and child processes. No new process or artifact owner is needed for the pure private parser test. |

The private native admission strategy changes, so obtain one fresh independent
adversarial review of this matrix and capability boundary before implementation.
Then complete one author matrix-to-diff pass and one fresh inspection-only code
review, followed by owner checks, bounded gate and Clippy on the final SHA.
One capability contains reordered admission, its discriminating allocation owner
and synchronized ordering prose. Expected handwritten changes remain below 1,000.

## Documentation closure

Plan29 section1.1 and its closure-row wording, plan34's exact validation order,
runtime ABI ledger20's retained-root row, plan94's representation section, and
the ordering sentence in English and Japanese std-design/fs now complete each
path's validation/grammar before its copy, retaining root-before-relative and
the possibility of an earlier root copy on relative rejection.
Other specification/design/settled records already require complete lexical
admission before I/O without fixing the private copy order; their promises do not
change and should not be restated. Record status once in HANDOFF at completion.
No external align-llm source, adoption state or request status changes are needed.

## Acceptance evidence

The fresh independent strategy review found no actionable issue. The new native
`tests::fs_beneath_complete_admission_precedes_private_allocation` owner covers
all four root/UTF-8 flag combinations, literal grammar expectations, malformed
metadata, actual allocation calls/bytes and exact independently owned output.
It ran against the original parser first: a 65,539-byte path ending in `/..`
failed the new zero-allocation assertion with one 65,540-byte allocation. The
same owner passes after the change; every invalid parser row now allocates zero,
and every admitted row retains exactly one input-length-plus-one allocation.

On macOS ARM64 and Linux ARM64, native fs_beneath, fs_private_temp,
fs_remove_empty_dir and fs_retained_tree::tests owners pass with alloc-count.
The four driver targets named above also pass, including their whole/per-unit,
native cleanup and omitted-Drop controls. Retain plan94's fixture conditions:
macOS uses a short canonical child-scoped TMPDIR; Linux native and driver
binaries run from writable /tmp. Existing process-global allocation-counter
owners run serially. No timing or RSS claim is made.
