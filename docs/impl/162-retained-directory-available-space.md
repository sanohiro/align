# Retained-directory available storage

Request144 supplies a concrete advisory admission use: model downloads of
0.56–17.73 GB into an already retained directory. The application owns its
reserve, refusal policy, streaming writes and failed-staging cleanup. This
capability adds one observation to the existing std.fs owner; K1/plan61,
exclusive sum projections and fixed Move-array elements remain separate.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Signature | `directory.available_space() -> Result<i64, Error>`, requiring `import std.fs`. No arguments, options, defaults, environment configuration or alternative path-based entry point. The receiver follows the existing named-local rule (no temporary or field-path receiver) and is an ordinary shared borrow of a live `fs.directory`; the call is Impure. |
| Value | Nonnegative available bytes on the filesystem containing the retained directory, as reported by the native filesystem's non-privileged available-block count. Zero is a valid observation. The ordinary Copy result retains no owner, view, region, descriptor or cleanup obligation. It remains usable after the directory is dropped. |
| Linux/macOS producer | One `fstatvfs` on the directory's existing descriptor. Compute `f_bavail * f_frsize`, using the fundamental allocation-block size, not the preferred I/O size `f_bsize`. No pathname reconstruction, cwd lookup, file open, subprocess, mount enumeration, quota query or fallback. Rename or removal of the original pathname does not retarget the retained descriptor. |
| Admission and errors | Preserve plan45's native numerical admission: validate the output extent/alignment, then the directory-shell extent/alignment, then their disjointness, before dereference or writes. Numerical rejection is Invalid and leaves output untouched. Then clear the output, query the descriptor and preserve the existing errno-to-Error mapping on failure, including native unsupported-operation errors. After native success, reject zero `f_frsize`, checked multiplication overflow or a value above i64::MAX with Invalid. No clamp or successful fabricated zero. Native failure precedes interpretation of native fields. |
| Native ABI | `align_rt_fs_directory_available_space`: `unsafe extern "C" fn(owner: *mut c_void, out: *mut i64) -> i32`, keyed as `FsDirectoryAvailableSpace`, shape A19 `i32(ptr, ptr)`, HostState effect. Output is fresh exclusive alignment-8, eight-byte scratch; status zero publishes the count, later failures leave zero scratch. The directory remains live and borrowed. No new pointer attributes or ownership transfer. Arbitrary addresses and expired owners remain native-caller precondition violations; numerical checks do not prove liveness. |
| Ownership and allocation | No acquisition, duplication, retention, mutation, source nulling, replacement or Drop of the directory. Use fixed local native statistics storage and ordinary scalar Result transport; the native query introduces no Align/Rust heap allocation. Native libc/kernel/filesystem-provider storage is outside that claim. Independent calls use independent scratch and require no process-global exclusion or restoration protocol. |
| Observation limits | Not a reservation, atomic multi-call snapshot, writable-directory test or promise that any subsequent write succeeds. Native accounting may omit or differ from user/project quotas, container limits, remote-server policy, shared-volume reservations, reclaimable space and privileged reserve access. No extra adjustment is made. Read-only filesystems can still report available blocks. Values may change immediately; callers keep fallible writes and cleanup. Native filesystem queries may block; there is no deadline or latency guarantee. |
| Platform boundary | Same shipped native platform scope as the retained-directory owner: 64-bit Linux and macOS. This adds no unsupported compiler/runtime target. A supported host whose filesystem or native query refuses the operation returns its mapped native Error; there is no emulation or command fallback. |
| Compiler representation | Extend the existing closed `FsTreeKind` table with `DirectoryAvailableSpace`, whose only input is the existing FsDirectory owner and whose output is signed i64. Add the matching closed scalar-scratch discriminator to `FsTreeOutput`. HIR and MIR authenticate exact receiver, arity, Result/error identity, status width and fresh exclusive i64 output before LLVM lowering. No new language type or ownership class. |
| Interfaces and cache | Generic/imported bodies serialize the existing FsTree operation family with its new selector. Existing exact compiler/source/native-runtime fingerprints invalidate affected artifacts. No public persisted/wire format, type-fingerprint rule, cache format, external source lookup or milestone dependency changes. Whole-program, per-unit and cached compilation have identical semantics. |
| Source agreement | This ledger owns the extension. Update draft.md, docs/language-spec.md, docs/design-notes.md, the Settled filesystem record in docs/open-questions.md, std-design/fs.md and its ja mirror, guide13 English/ja, checked-HIR/runtime ABI ledgers, declaration/export inventories, and the external Align provider answer. Plan45 remains the unchanged base owner contract. Consumer adoption and download measurements remain external. |

The public declaration above describes the builtin method, not a new source
declaration form. A call within an existing Result-returning function is:

```align
directory := fs.open_directory(".")?
available := directory.available_space()?
print(available)
```

No new encoded text, optional field, row order, detail mode, canonical byte
format or discriminator-dependent public record is introduced. The only native
output is an eight-byte count, consumed only on status zero. A performance
benchmark is not required: this is a new observation with no speed or resource
reduction promise. Allocation parity is a correctness owner for the fixed-storage
contract, not an RSS claim.

## Implementation closure matrix

| Invariant and control path | Implementation and discriminating owner |
| --- | --- |
| Formation and effect | Closed fs_tree signature/method/result table; extend `fs_observation_extensions` with exact arity, wrong receiver/result, missing import and imported impurity/parallel negatives. Existing FsTree visitors handle the sole evaluated receiver; malformed HIR owners reject wrong selector arguments and noncanonical Result/error types. |
| Construction, transport and lifetime | Existing scalar Result formation and FsDirectory shared-call admission. One parameterized driver composition crosses direct/imported/generic calls, if/match/else/?/map_err/loop/early return and a result surviving directory Drop in whole/per-unit execution. Move/replacement/return of the complete directory keep the existing owner machinery; shared queries never null it. Existing retained-owner cleanup and borrow negatives remain authoritative. |
| Native arithmetic and error precedence | `fs_retained_tree::tests` available-space owners inject native records/status: zero/one/max accepted bytes, distinct f_bsize/f_frsize, zero block size, oversized products and counts, denied/unsupported/I/O failures and multi-invalid inputs. Assert exactly one query, no interpretation/publication after failure, exact successful output and zero failed output. |
| Geometry and owner provenance | Native available-space owners reject null/misaligned/overflowing/overlapping output and shell extents before query or write. Real descriptor owners query after rename with the original pathname absent, verify the owner remains usable and exercise failed queries without acquiring/closing a descriptor. Existing Directory RAII/Drop owners cover final release; no new cleanup path is added. |
| Allocation | An isolated feature-enabled native owner uses the established allocation-probe child mechanism, a positive allocating control, then successful and failed calls with zero Align/Rust allocations. Set up fixtures and injection state before measurement. |
| MIR and LLVM ABI | Extend the existing FsTree producer/scratch validator and exact kind sweep. Malformed status/output width, mismatched discriminator, nonexistent/aliased/unauthenticated scratch and wrong owner fail before LLVM. Independent raw-LLVM assertions pin A19, eight-byte alignment-eight storage and success-only payload use. Update runtime-key effects, ABI declaration golden and native export inventory together. |
| Serialization and cache | `fs_observation_extensions` imported generic/helper whole/per-unit owner plus cold/hit compiler-cache replay. Result counts are runtime observations, never constant-folded or persisted as available storage. Existing compiler identity changes make old implementation artifacts ineligible. |
| Platform and existing behavior | Live Linux/macOS owners require success/nonnegative bytes without comparing separately changing observations for exact equality. Synthetic records independently fix multiplication and field choice. Run native available-space owners, complete fs_observation_extensions, affected HIR/MIR/ABI owners, existing retained-owner regressions, bounded gate and separate Clippy. Existing fs metadata/access semantics are unchanged. |

One capability PR keeps native query, typed scalar publication, compiler
validation and source documentation together. Splitting those layers would leave
an unusable producer or unchecked consumer and duplicate the same ABI proof.
No new ownership strategy, generalized observation framework or helper process
is needed.

## Primary platform evidence and author pass

The [POSIX statvfs fields](https://pubs.opengroup.org/onlinepubs/007904975/basedefs/sys/statvfs.h.html)
distinguish available blocks for a non-privileged process and the fundamental
block size. [Apple's fstatvfs implementation](https://github.com/apple-oss-distributions/Libc/blob/main/emulated/statvfs.c)
queries the descriptor and maps native allocation-block size into f_frsize;
f_bsize instead carries the preferred I/O size. The implementation and synthetic
owners must preserve that distinction.

Before implementation, check this ledger against plan45 admission/ownership,
the closed FsTree signature and output validators, current native error mapping,
and Linux/macOS field widths. Verify the example's existing syntax and complete
source-propagation inventory. Request one independent adversarial review of this
contract and matrix after that author pass, and resolve its complete finding set
before production edits. After implementation, bind every matrix row to the
actual code and regression owners before the ordinary preflight review.

Author pass (2026-10-10): checked the existing native prepare/descriptor helpers,
FsTree receiver/result table, MIR producer/output discriminator and A19
declaration shape. The installed libc definitions and Apple's descriptor wrapper
agree on the selected fields. The example parses through the released compiler's
formatter. This extension adds no owned output, input encoding, callback,
process-global state, cache-format change or later prerequisite. The propagation
and regression owners are listed above; production code is unchanged at review.

Independent adversarial plan review (2026-10-10): CLEAN. Inspection covered
native admission and errno mapping, shared owner lifetime, closed HIR/MIR
signatures and scratch validation, success-only publication, A19 effects and
the proposed regression owners. No production edits preceded that review.

## Matrix-to-diff closure

The author-side extraction of `must`, `exact`, `every`, `before`, `reject` and
`required` obligations maps to these implementation and regression owners:

- Admission, precedence and arithmetic: `align_rt_fs_directory_available_space`
  calls existing `prepare` before `directory_space_stat`; checked arithmetic
  precedes the sole output write. `available_space_arithmetic_errors_and_admission`
  supplies all field, errno, numerical-geometry and one-query controls.
- Retained identity and allocation: `available_space_retains_descriptor_identity`
  crosses rename and pathname removal and exercises real EBADF;
  `available_space_allocation_parity` isolates positive allocator controls and
  synthetic/live success and failure. Existing retained-tree RAII owners close
  final Drop; the operation neither creates nor closes a descriptor.
- Formation and transport: `fs_tree::{inputs,output,from_method,result_type}`,
  existing `check_fs_tree`/HIR validation and `lower_fs_tree` preserve shared
  named-local admission and ordinary Copy Result transport.
  `available_space_formation_and_effects` checks rejection and imported impurity;
  `available_space_composition_and_cache` covers the matrix's control paths,
  owner expiry, generic imports, whole/per-unit execution and cold/hit replay.
- Exact IR and native ABI: `retained_tree_records` checks selector/receiver,
  arity, scalar width and Error identity. `retained_tree_mir_gate` sweeps all 25
  selectors and rejects wrong status, argument, discriminator, scratch type,
  missing output, independent stores and duplicate producers.
  `available_space_scalar_layout_and_publication` pins eight-byte alignment-eight
  LLVM scratch and success-branch-only loading. Registry key/effect/cardinality,
  declaration golden and source-export owners cover the new A19 row.
- Platform and documentation: both shipped hosts use the same POSIX fields;
  synthetic records distinguish preferred I/O size from fundamental block size.
  Local verification runs on Linux x86_64; the supported macOS runtime uses the
  inspected libc field definitions and descriptor API. The authoritative prose,
  English/Japanese filesystem designs and guides, and HIR/ABI ledgers carry the
  same observation limits. No benchmark or consumer adoption is claimed.

All newly added driver compile/link/run paths use `owned_fixture::run`, including
transitive native helper waits, with exclusively acquired scratch. Existing
legacy driver helpers receive no new callers.
