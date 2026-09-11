# R69–R76 implementation handoff

Status: design reviewed; both findings resolved. Capability A is implemented;
local verification and independent code review precede publication. B and C remain planned. This document owns the
complete eight-request batch, including the three nonblocking requests. Provider
baseline: `8c9e24ee094b107e00c6685f3a034e5843eac6a4`; consumer register read at
`e39120fce311094c84e84cc281a37ba3953b03f1` on 2026-09-11. The user requested
design first and subsequently authorized implementation, PRs and merges for the
complete batch.

## 1. Scope and capability boundaries

| Request | Current evidence at provider baseline | Selected answer | Capability |
| --- | --- | --- | --- |
| R69 | The registered `Pending { directory, path }` example still fails `check-per-unit` at `inspect(items[0])`. | Extend existing shared place projection to both retained filesystem owner leaves, with the same storage-generation proof. | C |
| R70, blocking | The closed `FsTreeKind` table has no link read or followed metadata operation. | Bounded exact raw link bytes and explicit final-link-following metadata. | A |
| R71 | Direct borrowed `Option<string>` slicing passes `check-per-unit` but fails codegen; the `str` helper prints PASS. | Normalize the borrowed descriptor before string byte/boundary operations; preserve the physical-type validator. | C |
| R72, blocking | No retained real-credential access operation. Mode metadata is not an access decision. | Explicit real-ID permission observations using native access checks, including ACLs. | A |
| R73, blocking | `std.os` exposes host facts, not current real UID/GID. | One ordinary Copy identity record. | A |
| R74 | No retained symlink creation; currently nonblocking. | Exclusive final-entry creation with exact target bytes. | A |
| R75, blocking | Strict byte validation and private SSE decoding do not supply a generic owned replacement decoder. | One pure bytes-to-owned-string transform with maximal-subpart substitution. | B |
| R76, blocking | Closed one-shot hash discriminator has SHA-256/SHA-512 only. | Add SHA-1 through the same EVP engine and owned-array result. | B |

Deliver **one coordinated batch in three complete capability PRs**, in A, B, C
order. Each PR includes source admission, native/compiler implementation, trust
validation and its owners; never split a dormant producer from its consumers.
A groups retained filesystem/credential observations and creation; B groups the
two ordinary owned byte transforms; C groups shared projection admission and
physical descriptor normalization. These isolate native authority, byte
transforms, and compiler borrowing as distinct failure domains. Eight separate
request PRs would repeat proof and gates; one giant PR would entangle unrelated
failure recovery. A and C may exceed 1,000 handwritten lines because exact
schema/operation validation spans compiler layers and owner tests. Their complete
boundaries avoid duplicated ABI/ownership proof. This is a planning division,
not permission for incomplete or special-case implementations.

All three consume already-shipped plans 28, 42, 44 and 45, the checked-HIR/runtime
ledgers, and ordinary string/hash ownership. No later milestone is required.
R69 is an unfinished shared-projection capability: plan 28 excludes opaque leaves
because their projection contracts were undefined, while plan 45 already admits
these owners in records and ordinary collections. Defining their shared-only
projection does not reopen Move extraction, mutable indexed borrowing or another
locked language restriction. R71 implements existing string-view semantics.

Application traversal, suffix predicates, deleted-file deduplication, permissions
policy, supervisor/namespace argv, byte caps, redaction, Git object framing and
change admission remain Align application code. No native runner, decoder-policy
shim, subprocess hash helper, implicit clone or descriptor integer is selected.
Consumer code, pin adoption and integration are outside this repository's work.

## 2. Authoritative public-contract ledger

The following are planned additions, not available APIs at the baseline. There
are no optional arguments, defaults, environment configuration or retained input
lifetimes. Method receiver rules remain plan 45's named owner/shared parameter
rules. `bytes` input admission matches existing encoding/hash/fs byte views:
`slice<u8>`, `str`, or an owned `string` borrowed for the call; an owning byte
array must supply its existing view explicitly. Inputs evaluate once in source
order; a terminating operand skips later operands and native work.

| Exact surface | Effect, result and ownership | Errors and limits |
| --- | --- | --- |
| `directory.read_link(path: bytes, max_bytes: i64) -> Result<array<u8>, Error>` | `import std.fs`; Impure shared receiver. Owned exact final symlink target bytes, no UTF-8 conversion, normalization or logical NUL. Independent of directory/path/arena lifetime. | Complete relative grammar first, then positive cap with representable `cap + 1`, then retained traversal and one readlinkat. Over-cap is Invalid, never truncated success. Section 3 fixes allocation and failure order. |
| `directory.metadata_follow(path: bytes) -> Result<fs.metadata, Error>` | `std.fs`; Impure shared receiver. Existing complete Copy metadata record; follows the final entry's link chain, including Linux procfs magic links. Opens no target contents. | Plan 45 grammar/ancestor admission; normal native stat errors. Final traversal may escape the retained root. No descriptor, mutation or identity-stable pair with read_link is returned. |
| `fs.access_mode { read: bool, write: bool, execute: bool }` | Qualified-only ordinary Copy record. Exact three fields in declaration order, all required; ordinary user construction. | No source integer mask, existence-only mode, inferred policy or effective-ID option. |
| `directory.access(mode: fs.access_mode) -> Result<bool, Error>` | `std.fs`; Impure shared receiver. Real-credential observation anchored at the retained directory, using native `.` lookup semantics. | Empty mode Invalid. Native directory search required by `.` lookup is part of this observation, even for a read-only mask. Section 3 fixes false versus Error. |
| `directory.access_at(path: bytes, mode: fs.access_mode) -> Result<bool, Error>` | Same effect/borrowing. Relative real-ID access observation with retained, no-follow ancestors and final symlink refusal. No content open. | Complete path then mode, then ancestor admission/access and final observation. A retained root replaces ancestors above that root; this is not a replay of access on its original absolute pathname. |
| `os.identity_info { real_uid: i64, real_gid: i64 }` | Qualified-only ordinary Copy record, fields in this order. All fields present on success, nonnegative; ordinary construction and transport. | No account name, effective ID, authentication token or unavailable sentinel. |
| `os.identity() -> Result<os.identity_info, Error>` | `import std.os`; Impure. Native getuid then getgid; no heap allocation, credential mutation, account lookup or subprocess. | Checked conversions in field order; unrepresentable values or unsupported platform Invalid. Pair is not an atomic credential snapshot. |
| `directory.create_symlink(path: bytes, target: bytes) -> Result<(), Error>` | `std.fs`; Impure shared receiver. Call-scoped exact raw target bytes. Create only an absent final entry; no returned owner. | Validate complete relative destination before nonempty, NUL-free target; then retain ancestors and symlinkat. Existing file/directory/link is unchanged and yields native existing-entry Error. No overwrite or rollback unlink. |
| `encoding.utf8_decode_lossy(data: bytes) -> string` | `import std.encoding`; Pure. Independently heap-owned UTF-8 text, including when input is valid or arena-backed; no input retention. | Every byte string accepted. Preserve valid scalar encodings and NUL; each maximal ill-formed subpart becomes U+FFFD. Terminal length/OOM policy, no recoverable/partial result. Section 4 fixes exact partition. |
| `crypto.sha1(data: bytes) -> array<u8>` | `import std.crypto`; Impure like existing EVP hashes. Independently owned exactly 20 digest bytes in standard order. | Existing one-shot terminal allocation/provider-failure policy. No Git header, hex formatting, algorithm fallback or streaming/deadline promise. SHA-1 serves existing object-format interoperability, not collision-resistant security identity. |

On supported 64-bit Linux/macOS targets every successful record has every field;
there is no detail level, optional inspection field or output row ordering.
These additions define no persisted/wire format. Normal record canonicalization
includes qualified nominal identity and the complete reachable definition graph.
No new Ty/Scalar leaf tag or ownership class is introduced.

## 3. Retained native operations and error precedence

### 3.1 Shared admission and publication

Reuse plan 45's complete raw relative grammar and retained ancestor walk; never
resolve through cwd, reconstruct a pathname or change credentials/umask. The
root grammar is unchanged. New operations do not reinterpret metadata_at or
open_read. Temporary ancestor descriptors close on every return, preserving the
primary error. Read-only inputs may overlap each other. They may not overlap
the live directory shell or any writable output.

Native order: validate every output numeric extent/alignment; directory shell
extent; input byte extents in argument order, checked signed lengths and checked
terminator capacities; output/output, output/input/shell and input/shell disjointness. Reject
numerical failures before dereference or writes. Then zero all output scratch,
validate complete path contents, then cap/mode/target contents in argument order,
then perform native work. For create_symlink, validate *both* byte extents before
reading either input, then destination grammar before target contents. Bool ABI
values outside 0/1 reject with Invalid at mode validation. Zero-length byte
inputs do not require a readable pointer, but lexical empty inputs still fail.
Positive extents and owner liveness remain native caller preconditions; numeric
checks cannot authenticate arbitrary addresses. No error overwrites/frees an
already initialized caller owner: output scratch must be fresh and exclusive.

No retry loop hides native failures or turns them into absence. Native heap OOM
uses existing terminal behavior. Unsupported required native operations preserve
their native Error (`Code(ENOSYS)` or the mapped unsupported-flag error); do not
fall back to mode-bit emulation, /proc pathname reconstruction or content opens.

### 3.2 Link bytes and followed metadata

For read_link, admit exactly `1 <= max_bytes <= i32::MAX - 1` on both hosts.
After complete path validation, check conversions to usize/i64 and
`cap + 1 <= i32::MAX`, before retained traversal or allocation. Linux's syscall
count is signed int; macOS rejects a buffer size above INT32_MAX. A size_t/isize
check alone is insufficient and can turn a narrowed Linux count into false
untruncated success. Allocate exactly cap+1 bytes of detection scratch
after path/cap validation and retained-parent admission. One `readlinkat` fills
that buffer; no st_size estimate, retries or UTF-8 validation. Return Invalid
when native count exceeds cap (including count==scratch capacity); zero is an
owned empty array if the native platform returns it. Otherwise allocate one
exact-length Align array, copy the observed bytes once and free scratch. Empty
output uses the existing empty-array representation. The explicit bound covers
scratch cap+1 plus result at most cap, excluding path marshalling/allocator
overhead. There is no unbounded grow loop and no silent truncation. A regular
file is native EINVAL/Invalid; dangling links can be read normally.

metadata_follow uses `fstatat(retained_parent, final_name, ..., 0)` followed by
plan 45's exact metadata conversion. It follows ordinary and procfs final links
without opening their contents, so FIFO observation must complete without a
writer. Link resolution may leave the root, and separate read_link/stat calls
can observe different objects. Missing/dangling is NotFound; denial is Denied;
loops and other native failures keep the shared mapping. No fabricated record.

create_symlink privately NUL-terminates the target only for symlinkat; logical
target bytes exclude that terminator. Relative, absolute, dangling and invalid
UTF-8 targets are data. There is no target resolution/admission and no chmod.
Validate both complete inputs before the first filesystem action. syscall
success is ordinary entry creation, not durability or conditional identity
publication. A racing occupied final entry remains untouched by failed symlinkat.

### 3.3 Real-ID access

Do not use `AT_EACCESS`, temporarily switch credentials, infer from mode bits,
or use glibc's historical faccessat emulation: that emulation can omit ACLs.
Use a direct Linux faccessat2 syscall and native macOS faccessat. All requested
bits are conjunctive. Native supplementary groups, ACLs, mount restrictions,
root execute rules and credential semantics are preserved. Real credentials
can change between calls; there is no atomic permission snapshot or reservation
for a later operation.

Self: query `(directory_fd, ".", requested_mode, 0)` on both hosts. This anchors
the observation after pathname replacement, opens no new content descriptor,
and includes the kernel's `.` search semantics stated in the public ledger.

Relative: retain each ancestor as in plan 45. Before selecting each next name,
observe real-ID X_OK on the current retained directory using the self syscall.
A negative search decision returns Ok(false); a failed effective-credential
ancestor admission remains its ordinary Error. Keep the last parent retained
through the final operation. No obligation is invented to recheck directory
ancestors above the caller's retained root.

Linux final: `openat(parent, name, O_PATH|O_NOFOLLOW|O_CLOEXEC)`, then fstat;
reject a symlink as Invalid, close on every path. This is an identity-only
descriptor, never a content open or FIFO/device activation. Query the admitted
descriptor with faccessat2 `(fd, "", mode, AT_EMPTY_PATH)`. A final substitution
cannot redirect this descriptor check. Linux 5.8+ supplies the required syscall;
older/unavailable kernels refuse instead of emulating a weaker answer.

macOS final: `faccessat(parent, name, mode, AT_SYMLINK_NOFOLLOW_ANY)` rejects
symlinks in the kernel lookup, including a racing final link; ELOOP maps to
Invalid for this operation. The flag is 0x0800 in the public XNU fcntl header
and is present in macOS 15's XNU implementation, matching current CI. Prefer
the SDK/libc constant; if a Rust binding lacks it, use one named platform-local
constant with the source reference and native owner. Do not replace it with
AT_SYMLINK_NOFOLLOW plus stat-before/stat-after: that can accept a link swapped
only during the access call. An unavailable flag refuses explicitly. Neither
host opens target contents or promises identity continuity into a later action.

Exact false/error partition is **stage-specific**. Only a native access query's
EACCES, EPERM, EROFS or ETXTBSY becomes Ok(false); native zero becomes Ok(true).
All other query errno values use the existing shared Error mapping, except the
operation's symlink ELOOP refusal above. Missing final entry is NotFound. Any
lexical/shape/kind failure is Invalid. Errors in path admission, descriptor
acquisition, fstat or conversion remain Errors, even when their errno is EACCES
or EPERM. This prevents a failed observation from masquerading as a completed
negative. Cache no answer. A negative parent search stops before final lookup;
it need not reveal whether the inaccessible final name exists.

### 3.4 Native feasibility evidence

At baseline, Linux native probes confirmed O_PATH admission/fstat of a FIFO
without a content open and faccessat2 AT_EMPTY_PATH support. The same probe
showed a symlink O_PATH descriptor can itself pass an access check: the explicit
fstat/symlink refusal is therefore mandatory. macOS behavior is grounded in
source inspection, not claimed as an executed host test. Required platform
owners below must run before that implementation is pushed.

References: [Linux access/faccessat2](https://man7.org/linux/man-pages/man2/access.2.html),
[Apple macOS 15 faccessat implementation](https://github.com/apple-oss-distributions/xnu/blob/xnu-11215.1.10/bsd/vfs/vfs_syscalls.c),
[Apple fcntl flags](https://github.com/apple-oss-distributions/xnu/blob/xnu-11215.1.10/bsd/sys/fcntl.h),
[readlinkat](https://man7.org/linux/man-pages/man2/readlink.2.html),
[Linux syscall count type](https://github.com/torvalds/linux/blob/v6.12/fs/stat.c), and
[getuid](https://man7.org/linux/man-pages/man2/getuid.2.html).

## 4. Exact byte transforms

UTF-8 replacement follows Unicode maximal-subpart substitution: preserve each
well-formed scalar; at an invalid offset consume the longest prefix which
could begin a valid scalar, or one byte when no such prefix exists, and output
EF BF BD. Continue at the next unconsumed byte. A truncated suffix is invalid
at the end of this call. A joined input and separately decoded chunks can differ.
There is no streaming state, normalization, BOM removal, redaction or cap.

Use the existing strict UTF-8 validator/error partition (Rust `from_utf8` plus
valid_up_to/error_len) in two passes: checked exact output sizing, then one
Align allocation and direct filling. Do not allocate a whole intermediate
String/Cow/Vec and then clone it. Length greater than isize::MAX/i64::MAX or
checked arithmetic overflow uses the encoding terminal failure path before
output allocation. Empty output follows the ordinary owned-empty convention;
valid input is still copied into independent ownership. Native malformed pointer
inputs retain the shared encoding byte-view ABI preconditions; this source API
does not legitimize forged memory. No new panic is permitted from malformed
UTF-8; that is normal accepted data.

These independent CPython replacement vectors were checked during planning:

| Input hex | Exact output hex | Inserted replacements |
| --- | --- | --- |
| empty | empty | 0 |
| 00 | 00 | 0 |
| c0 af | efbfbd efbfbd | 2 |
| e1 80 | efbfbd | 1 |
| ed a0 80 | efbfbd efbfbd efbfbd | 3 |
| f4 90 80 80 | efbfbd efbfbd efbfbd efbfbd | 4 |
| e1 80 41 | efbfbd 41 | 1 |
| f0 9f 92 a9 | f09f92a9 | 0 |
| f0 9f 92 | efbfbd | 1 |

Acceptance extends these with all byte-class boundaries and every split of
representative 2/3/4-byte and malformed sequences. Check literal expected bytes
independently of the runtime implementation; a Python fixture oracle is test
tooling only. Count *inserted* replacements separately from literal valid U+FFFD.
[Unicode substitution definition](https://www.unicode.org/versions/Unicode16.0.0/core-spec/chapter-3/)
owns the algorithm; the frozen CPython consumer pins interoperability.

SHA-1 reuses `crypto_digest(c"SHA1", 20, ...)` and EVP_Q_digest, including the
exact provider-failure abort and returned-length assertion. No handwritten SHA
engine, provider fallback or new dependency. Keep capability-driven libcrypto
linking. RFC 3174 empty/abc/long vectors, binary/NUL input and 55/56/63/64/65-byte
padding boundaries are independent owner data. A separate application-level
fixture forms `blob ` + decimal byte length + NUL + payload and compares to Git
configured explicitly for SHA-1. No Git-specific byte transformation enters
std.crypto. See [RFC 3174](https://www.rfc-editor.org/rfc/rfc3174) and
[OpenSSL EVP digest API](https://docs.openssl.org/3.0/man3/EVP_DigestInit/).

## 5. Compiler representation and native ABI

Extend `align_sema::fs_tree::FsTreeKind` with exactly DirectoryReadLink,
DirectoryMetadataFollow, DirectoryAccess, DirectoryAccessAt and
DirectoryCreateSymlink. Keep the existing single closed signature table. Add
input kinds for i64 bound and the exact `fs.access_mode` record; add output
kinds OwnedBytes and Bool. The new HIR FsTree ordered operands are exactly the
public receiver/arguments; validate the reserved access record globally even
without a call. Shared receiver effect/escape/move handling is unchanged.

MIR extends FsTreeOutput with `Bytes(Slot)` and `Bool(Slot)`: exact DynArray(u8)
and Bool slots, respectively, distinct from owner handles and metadata. Reuse
the existing status-to-Result reconstruction; outputs publish only on status
zero. The Bool native output is a single i8 0/1 byte, never a Rust bool pointer.
An error may leave zero scratch but never creates a source false/empty result.
The access record remains a typed MIR operand; LLVM only extracts its three
Bool fields and zero-extends each to the native i8 argument. Native code maps
the three flags to platform R_OK/W_OK/X_OK. No native platform mask leaks into
source types or opaque LLVM semantic inference.

Add nullary HIR OsIdentity and MIR `OsIdentity { out: Slot }`, following OsHost
without owned fields. Globally certify qualified record name and ordered fields
at checked HIR and every MIR/codegen entrypoint. Identity scratch is exactly
16 bytes/alignment 8: real_uid i64 at 0, real_gid i64 at 8. Access record has
three Bool fields (compiler layout offsets 0/1/2); no pointer to that record
crosses FFI. Runtime validates identity output numeric extent/alignment before
zeroing/querying, converts uid then gid, and publishes both only on success.

R75 extends the existing owned-byte-to-string HIR/MIR transform with a closed
`EncodingKind::Utf8Lossy` case. `EncodingDecode { kind: Utf8Lossy }` is invalid,
as for the existing encode-only Html case. Public dispatch explicitly maps
utf8_decode_lossy into that owned-text transform; do not let the current fallback
map it to Hex. R76 adds `HashAlgo::Sha1` with exact output length 20. Update every
closed discriminator table, capability, print/hash/canonical-body path and
validator; no new HIR node is needed for either transform.

| Runtime key / exact exported symbol | Exact C/LLVM ABI |
| --- | --- |
| FsDirectoryReadLink / `align_rt_fs_directory_read_link` | `i32(ptr directory, ptr path, i64 len, i64 max_bytes, ptr out_bytes)` |
| FsDirectoryMetadataFollow / `align_rt_fs_directory_metadata_follow` | `i32(ptr directory, ptr path, i64 len, ptr out_metadata)` |
| FsDirectoryAccess / `align_rt_fs_directory_access` | `i32(ptr directory, i8 read, i8 write, i8 execute, ptr out_bool)` |
| FsDirectoryAccessAt / `align_rt_fs_directory_access_at` | `i32(ptr directory, ptr path, i64 len, i8 read, i8 write, i8 execute, ptr out_bool)` |
| FsDirectoryCreateSymlink / `align_rt_fs_directory_create_symlink` | `i32(ptr directory, ptr path, i64 path_len, ptr target, i64 target_len)` |
| OsIdentity / `align_rt_os_identity` | `i32(ptr out_identity)` |
| Utf8DecodeLossy / `align_rt_utf8_decode_lossy` | `{ptr,i64}(ptr data, i64 len)` |
| CryptoSha1 / `align_rt_crypto_sha1` | `{ptr,i64}(ptr data, i64 len)` |

Byte headers are exactly plan 45's 16-byte/alignment-8 pointer+length; metadata
remains its unchanged 64-byte schema. Transform return headers transfer ordinary
heap ownership; fs out scratch is zeroed before content/native failure. There
is no new free symbol: existing array/string cleanup applies. No new noalias,
readonly, nonnull or dereferenceability attributes are inferred. Add only absent
ABI shapes to the registry in this table's order and retain its alphabetical
RuntimeKey order; ordinal/declaration/export owners recompute the complete set.
These internal ordinals are not a newly persisted public format. Do not invent
hardcoded ordinals in consumers.

Interfaces reconstruct std.fs/std.os/std.encoding/std.crypto imports and exact
qualified schemas. Generic bodies are rechecked after import. Complete nominal
definitions and all operation discriminators enter current MIR/body/cache
identity; changed compiler/runtime fingerprints invalidate artifacts. No native
permission/identity/text result becomes a compile-cache input. No reflection,
runtime source I/O, external schema cache, or compatibility path is introduced.

## 6. Shared projection contract (R69 and R71)

Extend the single finite, cycle-safe shared-payload classifier with **exactly
FsDirectory and FsDirCursor** as leaves. The recursive structure remains its
existing structs, Option, Result, user sums and admitted dynamic scalar/AoS
record arrays; include previously admitted Buffer/Writer/ProcessMember siblings
unchanged. Concrete type formation still runs first. The new leaf permission
does not admit direct opaque arrays, new array element representations, fixed
arrays in the payload grammar, boxes, tuples, globals, capture, parallel
transport or arbitrary resource handles.

This is uniform across the classifier's consumers: borrowed sum payloads,
indexed shared calls and existing borrowed projection forwarding. Arrays of
ordinary records owning these handles, their already-admitted fixed-array view
sources, and `slice<Record>` use the same shared place contract. A named slice
header and its backing generation are reserved independently through eager
index and later argument evaluation. Every nested owner stays with its original
collection/sum; the callee receives only a call-scoped shared pointer. No Move
load, extraction, nulling, owner temporary, clone, allocation or Drop occurs.
Directory shared methods can run; shared cursor parameters cannot call next.
Forward a field through an explicitly shared helper using the existing receiver
rules rather than adding arbitrary field-path method syntax.

All existing returned-view and mutable-retention summaries substitute the full
source generation and contained regions, including imported/indirect fallback.
Whole Move-value reads, exclusive indexed calls, replacement, capture/escape,
owner destruction and source/header changes during eager operands still reject.
Broader opaque leaves require their own contract, not a wildcard default.

R71's baseline MIR reaches UTF-8 boundary SliceIndex with the borrowed Some
payload physically typed String; the helper produces a canonical Str value.
`lower_view_retype` alone is insufficient: it only changes BorrowedPlace.ty,
while `checked_borrowed_place_ty` correctly returns the physical path type.
In `lower_slice_range`, normalize a string receiver to a fresh Copy **Str
descriptor value** before SliceLen, UTF-8 boundary byte loads and SubSlice.
For a borrowed operand, emit `Let(v: Str, Use(BorrowedPlace { ty: Str, ... }))`,
retaining the exact original root/path/cleanup and `inherit_borrow_owners` facts.
Inspect the actual operand even when HIR recv.ty is already Str. An ordinary
owned String value uses the existing one-way value retype. This reads only the
pointer/length header; it creates no owning temporary, heap allocation, byte
copy or source nulling.

Complete the corresponding **narrow read-materialization proof** in the LLVM
producer validator's Use case: result exactly Str, logical borrowed place
exactly Str, authenticated physical path leaf exactly String or Str. Route this
case through existing `read_source`/`add_read_operand`, with the complete live
root/path/active-arm/cleanup check and founded storage dependency. The generic
`source()` function currently rejects BorrowedPlace and must continue doing so;
do not make arbitrary borrowed descriptors into owner-transfer evidence. Keep
physical type recovery unchanged. The view can carry Shared access but cannot
mint writable backing. Audit direct indexing and sibling borrowed string
consumers for this same physical/logical mismatch using invariant-level owners.
An unknown path, stale arm, missing initialization, circular Use/store proof,
String owning result, reverse Str-to-String retype or write laundering must reject.

No new HIR/MIR variant, interface field, canonical tag, runtime function or
allocation is selected for capability C. Preserve strict source/result physical
checks, one-way String-to-Str compatibility, active sum-arm proof, complete
projection paths and readonly backing authority. Plan 53's founded initialization
proof and plan 52's write-authority restrictions remain intact.

## 7. Implementation closure and exact owner checklist

Names marked new are implementation targets, not tests claimed to exist. Reuse
existing parameterized owners when they would detect the defect; do not create
one fixture per paper cell. No speed/resource benchmark promise is added. The
read_link bound and no-copy/no-extra-owner rules are allocation/IR correctness
owners. Each command remains within the repository's 30-minute test budget.

| Axis | A: retained/native observations | B: byte transforms | C: shared projections |
| --- | --- | --- | --- |
| Formation / validation | New driver `fs_observation_extensions`, `m11_os_identity`: exact import/arity/modes/reserved schemas, wrong fields/order/types, empty mode, bytes admission. | Extend `m10_encoding`, `m11_crypto`: imports, byte-view forms, wrong args, ordinary owned results, effect checks (lossy Pure, SHA-1 Impure). | Extend `move_record_slices`, `fs_retained_tree`, `return_provenance`: directory/cursor leaves, nested struct/Option/Result/sum/array carriers and excluded siblings. |
| Native values / errors | Extend runtime `fs_retained_tree::tests`: raw link, bound±1, dangling/missing, procfs open-unlinked identity; cap i32::MAX boundary tested by pure admission and allocation/syscall refusal witnesses without gigabyte allocation; stat FIFO nonblocking; exclusive symlink occupied matrix; self/path/all seven masks; query error partition versus admission errors. New runtime os_identity exact getuid/getgid oracle. | Runtime exact UTF-8 vectors/splits and checked sizing; SHA-1 published digests/padding/binary vectors; provider-failure subprocess and independent framed Git test. | Driver direct/helper Some/None and owned/borrowed String twins, empty/in-bounds/multibyte boundaries; invalid bounds in isolated subprocess; owner queue shared metadata and independently owned cursors after rename. |
| Authority / platform | Linux/macOS native access oracle; parent path replacement; symlink substitution at final access; ACL/group/real-vs-effective fixture; no content open/FIFO/device activation; no credential/cwd/environment mutation. | Borrowed data unchanged; no retained arena/source pointers; no unintended libcrypto linkage for lossy-only code, SHA-1 links the existing EVP capability. | Original owner remains sole owner; reject eager index/later-argument invalidation, same-source slice-header rebind, backing replacement, shared cursor.next, exclusive indexed access and escaped views. |
| Construction / move / Drop / replace / return | Bytes result in records/Option/Result/arrays; temporary descriptor release on every failure; read_link output survives directory/arena end. Copy mode/identity have no Drop. Reuse `fs_retained_tree::ownership_cleanup_and_negative_controls`; add omitted-output-Drop and failure descriptor witnesses. | Existing string/array owner path: move-in/out, source nulling, replacement, return, arena expiry; allocation witness detects a second complete output copy and omitted Drop. | Existing collection handles close once, borrowed element creates no cleanup; repeated projection and shared call remain allocation-free; omitted source Drop must fail the cleanup witness. |
| Control paths / ordering | Receiver, path, cap/mode/target once left-to-right; if/match/else/?/map_err/loop joins and early return/break/abort before native side effects. | Owned result flow through all ordinary control paths; eager divergence does not allocate; invalid UTF-8 is accepted data, not Error control flow. | Same control paths for view lifetimes; active-arm and source-generation proof; receiver/index/bounds/later args once; discriminator reachability, fallthrough vs divergence, expected type/source-order and already-invalid subtrees. |
| Checked HIR | Reserved schemas globally, exact FsTree kind/args/result, OsIdentity and every child/effect/move/escape/depth/replay visitor; variant-sweep tripwire. | Closed transform/hash cases; forged lossy decode direction and wrong result/effect; no unknown-kind Hex/SHA-512 fallback. | Classifier producer/replay agreement; malformed ids/cycles, wrong root/path/index/mode/active arm/retention and later-operand facts fail closed. |
| MIR / ABI / provenance | Extend `retained_tree_mir_gate` and new identity gate: every discriminator, exact ordered inputs, output case/type/alias/initialization; bytes and Bool slots checked even without owner-demand. Runtime output/overlap/numeric canaries and layout/declaration/export/ordinal owners. | Wrong transform return/input/hash kind rejects whole/per-unit/prelink; owned-byte proof handles SHA-1 like SHA-256, shared source cannot mint writable provenance. | Extend `move_slice_mir_gate`: full root/path + String/Str physical twins, forged owning loads, wrong fields, missing bounds/header reservation and circular initialization reject at publication/whole/per-unit/ThinLTO entrypoints. |
| Generics / interface / cache | Direct/imported/generic calls and returned records; cold/hit/body edit/revert; reserved types reconstructed by qualified name, no lookalikes. | Imported helpers and generic byte/result carriers; cold/hit/edit/revert; ABI capability set includes new hash and excludes unrelated modules. | Whole/per-unit local/imported/function-value helpers, generic owner records and returned views; cold/hit/edit/revert. No persisted shape change. |

Platform credential fixtures run in **disposable subprocesses**, never mutate the
test harness credentials. Linux privileged/root/ACL fixtures use a disposable
container with the narrowly required permissions, and must compare real and
effective identities plus supplementary groups. macOS native owners use its ACL
facilities; privileged identity separation may use an explicitly available
isolated fixture. Record unsupported privileged fixtures as unavailable, never
as passing; ordinary macOS mask/no-follow/rename/FIFO owners are mandatory.
Test the false partition with a pure errno mapping owner in addition to actual
native denial. Linux syscall-unavailable and macOS flag-unavailable paths need
injected refusal controls. Do not claim a macOS host run from XNU inspection.

Before a new required CI/platform job, ship and run a matching local script.
Use existing service/container and platform verification tools, not CI as the
first behavior test. If the implementer lacks the needed host, complete local
portable/native ownership work and record that precise platform-validation
prerequisite before publication; do not silently weaken the contract.

## 8. Execution instructions for the implementation model

1. Read this ledger, then the narrow owner plan/source before editing. Reconfirm
   baseline and carry forward user changes. Reproduce R69/R71 from the register;
   these are source-valid safety probes, not malformed native execution.
2. Implement capability A across `align_runtime/src/fs_retained_tree.rs`, its
   identity module, `align_sema/src/fs_tree.rs`, HIR/schema/import inventories,
   MIR and LLVM validators/ABI. Follow OsHost and existing FsTree patterns; keep
   all signature/schema cases closed. A alone must be usable after merge.
3. Implement B through existing EncodingEncode/CryptoHash machinery, including
   native owned allocation and all closed discriminators. Do not generalize
   unrelated transform/crypto APIs.
4. Implement C through the one shared-payload classifier and canonical borrowed
   descriptor producer. Audit the complete root-cause class, including nested
   projection and eager-generation paths, without weakening malformed-IR gates.
5. For each candidate, run its owner checklist and align-self-review before the
   one fresh full-diff independent review. Combine valid findings in one coherent
   fix. Complete the matrix-to-diff pass, final-SHA preflight, wrapper-opened PR,
   CI and merge before starting the next capability. Reopen the matrix for a
   strategy/IR-shape/P1 redesign, not repeated one-line review patch rounds.
6. Owner commands: A uses new driver `fs_observation_extensions` and
   `m11_os_identity` plus existing `fs_retained_tree`, and their focused runtime
   owners; B uses `m10_encoding`, `m11_crypto` and the focused runtime transform
   owners; C uses `move_record_slices`, `fs_retained_tree`, `return_provenance`
   and relevant sema/MIR/LLVM lib owners. Use `scripts/cargo.sh`, then the normal
   `scripts/pre-pr.sh` owner/gate/isolated-Clippy flow. No mandatory full nightly
   or unrelated DB suite; respect the actual DB-scope classifier.
7. Once all three are merged, run exactly `cargo build --release --workspace`.
   Update only the sibling request register with shipped signatures, limits,
   ownership and PRs, leaving that edit uncommitted. No version/tag/release is
   requested. Consumer adoption must not be performed to make the register green.

The user has authorized implementation and PRs through merge. The batch
completion condition is all eight provider requirements closed, not merely the
first blocker. A reproduced additional compiler defect is recorded and assessed
against the owning matrix; speculative API expansion is not another batch item.

## 9. Source-of-truth propagation and consumer handoff

This plan is the exact ledger. Current specification sections must distinguish
these selected additions from shipped APIs. Propagate its concise public contract
to draft.md, docs/language-spec.md, docs/design-notes.md and Settled; fs/os/crypto
designs and their Japanese mirrors link here. There is no standalone encoding
design file at baseline: the existing draft/spec encoding sections remain its
public owner. Plans 28/44 reference the selected shared-leaf extension without
rewriting historical shipped grammar; plan 45 references its five additions.
HIR/runtime ledgers record planned additions without corrupting the implemented
inventory counts. HANDOFF records this one batch boundary, not per-PR status.

Align acceptance is separate from consumer integration. R70/R72/R73/R75/R76
remain blockers until implemented and adopted. R69/R71/R74 are included without
inventing new cutover gates. Consumer-owned focused owners are exactly the
register's pending queue, deleted-open-file/link-byte, access, identity, symlink
copy, diagnostic-decode and Git-blob tests. R71 also names the existing verifier
smoke. Final A1 text consumers and A2/A3/A4 functional/containment/no-Python
commands remain that application's responsibilities; the latter three are
planned, not currently passing evidence. Preserve every capture/redaction/decode/
truncation order, including FIXTURE_PATCH and the second bounded-text decode.
SHA-1 blob framing uses byte length and a literal NUL; mode changes are separate
from content identity. One final consumer pin/materialization may adopt the
whole provider batch. Its absence is not permission to edit align-llm code.

Author ledger pass: every new result has exact fields, ownership, effect and
error precedence; native layouts and operation order are fixed; no public wire
format, runtime inspection, global native mutation or later milestone is added.
Normative signatures use declaration notation. Existing R69/R71 Align examples
were checked and their current failure/helper control recorded above. New API
syntax/execution owners necessarily wait for implementation; no invented runnable
example is claimed checked.

Independent adversarial design review completed on 2026-09-11 with two P2
findings, resolved together before handoff: the portable read_link cap now obeys
the signed native count, and R71 selects both Str descriptor materialization and
its narrow read-provenance certification instead of relying on a type-only
relabel. The author also closed input/shell overlap wording in the validation
order. The owner matrix includes integer-boundary no-allocation witnesses and
the R71 malformed/owning/uninitialized/write-laundering controls. Review covered
all eight requests and found no other unresolved design issue; it was static
inspection, not implementation or native macOS execution evidence.


### Capability A implementation closure

The closed FsTree table now has 24 operations. Its five additions use existing
byte-view admission, status-to-Result construction and shared receiver reservation.
OsIdentity is nullary and Impure; both new Copy schemas are authenticated globally
in checked HIR/MIR and reconstructed by the interface nominal inventory.
`retained_tree_mir_gate`, `identity_mir_gate_rejects_forged_schema`,
`observation_extension_records` and `retained_tree_records` own malformed schemas,
ordered operands, output discriminators, slots and result types.
`fs_observation_extensions` and `m11_os_identity` own imported/generic whole/per-unit
execution and source rejection; `fs_retained_tree` retains its cache and lifecycle
owners. Its omitted-link-bytes-Drop control extends the existing allocation and
FD leak witness. Native fs tests own raw targets, cap boundaries, occupied entries,
all masks, denial/error partition, numerical/alias admission, rename, no-follow
ancestors, FIFO metadata and Linux open-unlinked procfs identity. `os_host` owns
identity layout, checked conversion and native credential equality.
The isolated unavailable-access owner injects ENOSYS/EINVAL at the native-call
boundary for self and final-relative queries. It asserts exact status, zero
output, query count and parent/target FD cleanup. Mutating refusal to false or
omitting final-FD Drop makes this owner fail. The driver fixture also observes
and checks access on a FIFO under its existing child deadline.

Author investigation found lexical arenas incorrectly assigned generation-release
boundaries to independent native allocations. The existing independent-storage
predicate now governs both Drop selection and generation release; the arena-return
link-byte owner fails without this correction. This is an existing allocation
contract repair, not a new allocation mode.

Platform verification prerequisite: the implementation environment is Linux.
macOS native execution has not yet been obtained; XNU source inspection is not
execution evidence. Privileged Linux credential/ACL fixtures are unavailable in
the current Docker setup. These are not recorded as passing. Ordinary macOS
mask/no-follow/rename/FIFO execution remains a publication prerequisite above.


Independent capability-A code review found one P2 owner gap: pure errno mapping
alone did not exercise unavailable native access through exported operations.
The isolated native-call injection owner above closes that finding; no production
strategy or public contract changed. Author gate corrections also parenthesize
the access mask and synchronize the second exhaustive ExprKind inventory owner.
