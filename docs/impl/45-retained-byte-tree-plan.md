# Retained byte-path filesystem operations

Status: exact F2/R64 contract selected by plan 40; implemented with local owner
verification. Consumer adoption is separate. The unrelated candidate objects reported in the
external register are unavailable here and do not certify this implementation.

## Public contract ledger

All operations require import std.fs and are Impure. Arguments are required;
there are no ambient options, defaults or environment expansion. A method borrows
its receiver for the call, shared unless marked exclusive below. As with existing
opaque-handle methods, bind the receiver to a named local (owned or a matching
borrow parameter) before calling a method; temporary and field-path receivers
are not added. Owned local handles may advance internally; an explicitly shared
borrow parameter cannot call exclusive next and must use borrow mut. No method
retains a path, another receiver, or a caller buffer. Constructors allocate
ordinary Move owners; no descriptor integer is exposed. Linux and macOS are the
supported targets. Directory Drop closes its descriptor, never removes entries.

| Surface | Exact signature and effect |
| --- | --- |
| Root | `fs.open_directory(path: str) -> Result<fs.directory, Error>` opens a retained directory using the root grammar and no-follow admission below. |
| Independent enumeration | `directory.cursor() -> Result<fs.dir_cursor, Error>` opens a fresh independently positioned directory stream for the retained object. The cursor survives directory Drop. |
| Next | Exclusive `cursor.next() -> Result<Option<fs.dir_entry>, Error>` returns one owned raw basename, EOF None, or a terminal cached Error. Repeating after EOF returns None; repeating after error returns the same error category/code without another OS read. |
| Directory observation | `directory.metadata() -> Result<fs.metadata, Error>` observes the descriptor. `directory.metadata_at(path: bytes) -> Result<fs.metadata, Error>` observes the final named entry without following it. |
| Relative directory | `directory.open_dir(path: bytes) -> Result<fs.directory, Error>` retains an existing directory after no-follow admission. |
| Relative reader | `directory.open_read(path: bytes) -> Result<reader, Error>` admits only a regular file and returns the existing reader owner. `directory.open_read_single_link(path: bytes) -> Result<reader, Error>` additionally requires descriptor link count exactly one before publication. |
| Relative writer | `directory.create_new(path: bytes) -> Result<writer, Error>` exclusively creates a new regular file, requested mode 0666 filtered by umask/ACL, and returns the existing writer. Existing names fail; no replace, truncate or rollback unlink. |
| Directory creation | `directory.create_dir(path: bytes, mode: u32) -> Result<(), Error>` creates exactly the final directory, with existing ancestors. Mode is a permission request filtered by current umask/ACL; no process-global umask change. It returns no retained child; a later open is a separate admission. |
| Removal | `directory.remove_file(path: bytes) -> Result<(), Error>` unlinks a final non-directory entry, including a symlink itself. `directory.remove_dir(path: bytes) -> Result<(), Error>` removes only an empty final directory. Neither follows the final symlink. No recursive removal or identity-conditional deletion is promised. |
| Descriptor mode | `directory.set_mode(mode: u32) -> Result<(), Error>` changes the retained object's mode through its descriptor. |
| File observations | `reader.metadata()`, `writer.metadata()`, and `file.metadata()` each return `Result<fs.metadata, Error>` from their existing descriptor without changing file position or flushing buffered data. |
| File modes | `reader.set_mode(mode: u32)`, `writer.set_mode(mode: u32)`, and `file.set_mode(mode: u32)` each return `Result<(), Error>` and apply mode through the existing descriptor. Opening read-only does not itself prove or deny chmod authority. |

Mode accepts 0 through 07777 inclusive; all other bits are Invalid before I/O.
Mode includes permissions and setuid/setgid/sticky bits, not file-kind bits.
The OS may restrict/clear privileged bits; success does not guarantee exact
post-operation bits under concurrent mutation or native policy. Named create
validates the entire path before validating mode, then traverses ancestors.
Descriptor mode validates mode before invoking fchmod. Mode queries are separate
metadata observations. Reader/writer/file buffered state remains unchanged.

The ordinary qualified record `fs.dir_entry` has exactly one field in declaration
order: `name: array<u8>`. The array owns the basename's exact bytes without a NUL
terminator in its logical length. Names are never decoded, normalized, sorted or
silently omitted. Only `.` and `..` are skipped. Entries survive cursor advances
and Drop. No metadata is implicitly attached to an entry.

The ordinary Copy sum `fs.entry_kind` has exactly four payload-free variants in
order: Regular, Directory, Symlink, Other. Its discriminants are 0, 1, 2, 3.
The ordinary Copy record `fs.metadata` has exactly these declaration-order fields:

| Field | Type | Meaning |
| --- | --- | --- |
| kind | fs.entry_kind | Regular file, directory, symbolic link, or every other native kind. |
| device | u64 | Native device identity, zero-extended from its native unsigned bit representation. |
| inode | u64 | Native inode identity, checked unsigned conversion. |
| links | u64 | Native hard-link count, checked unsigned conversion. |
| mode | u32 | Native mode masked with 07777; no file-kind bits. |
| size | i64 | Native st_size, checked signed conversion; interpretation is native for nonregular entries. |
| modified_seconds | i64 | Modification time, signed POSIX seconds. |
| modified_nanoseconds | u32 | Subsecond part, 0 through 999999999. |
| changed_seconds | i64 | Status-change time, signed POSIX seconds; not creation time. |
| changed_nanoseconds | u32 | Subsecond part, 0 through 999999999. |

All fields are present on success; there are no unavailable placeholders or
optional variants. Unrepresentable native fields or malformed nanoseconds are
Invalid, never truncated or fabricated. Negative seconds and sizes are preserved.
Device/inode are local observations, not portable persisted identities or deletion
authority. File metadata is not flushed-write acknowledgement. No atomic snapshot,
write permission prediction, stable ordering or immutable-source guarantee follows.

## Path admission, enumeration and error precedence

Root input is UTF-8 str: `.` and `/` are admitted specially; otherwise one optional
leading slash and nonempty components, with no trailing/repeated separators,
`.` or `..` components. Empty, NUL and malformed UTF-8 are Invalid. Relative input
is bytes: nonempty, no NUL, leading/trailing slash, empty component, `.` or `..`.
Every other byte is admitted, including invalid UTF-8. Complete lexical validation
precedes any filesystem operation. Marshal one private NUL-delimited copy; never
modify caller bytes. Existing UTF-8 retained-root functions keep their contract.

Root opens `.` or `/` as its start without changing cwd. Every ordinary root or
relative ancestor component follows plan 29: no-follow observation, reject
non-directory, open DIRECTORY|NOFOLLOW|CLOEXEC, revalidate descriptor kind and
observed device/inode, then advance. Relative traversal starts from the retained
descriptor, never reconstructs a pathname or falls back to cwd. Open_dir applies
that same sequence to its final component. Metadata_at uses fstatat NOFOLLOW on
the final name; no final kind is rejected merely by observation.

Open_read follows no-follow final observation, reject observed nonregular kind,
open RDONLY|NONBLOCK|NOFOLLOW|CLOEXEC, descriptor kind/device/inode revalidation,
restore blocking flags, optional single-link check, then publish reader. A name
that vanishes or changes kind between observation and open is Invalid under
plan 29's exact post-observation errno mapping. An already observed special entry
is never opened. A hostile device substitution after observation can still have
native open effects before rejection; no special descriptor or read bytes are
published. Side-effect-free hostile-device admission requires exclusive writers
or deployment isolation, as the existing retained APIs do.

Create_new uses WRONLY|CREAT|EXCL|NOFOLLOW|CLOEXEC at the retained parent. It then
fstats the opened descriptor and requires regular kind, separately observes the
created final name NOFOLLOW and requires matching kind/device/inode, then publishes.
Failure closes the descriptor and preserves the primary error; it never unlinks
what might now be a replacement. A created entry can therefore remain after a
post-create failure. Directory creation likewise has ordinary mkdirat success,
not retained-identity publication. Remove operations use unlinkat with the
operation's fixed directory flag, without a fabricated identity check.

Cursor construction uses openat(directory_fd, ".", RDONLY|DIRECTORY|NOFOLLOW|
CLOEXEC) and fdopendir, transferring the fd only on success. It does not dup a
shared enumeration position. Each next sets errno zero before readdir; a null
result with zero errno is EOF, nonzero is Error. Copy the exact returned basename
before the next native read. Never use d_type as metadata. An unchanged accessible
directory enumerates all names; concurrent mutation is not a snapshot. Native
macOS refusal to create an invalid-UTF-8 name is not permission to filter a name
returned by its OS. Linux tests create and enumerate actual raw names.

Lexical/kind/identity/mode/conversion failures are Invalid. Native failures use
existing io_error_to_status, including NotFound/Denied categories and Code(errno)
for other errors; reuse plan 29's post-observation race mapping unchanged.
Metadata_at vanished final name is ordinary NotFound. Error is never EOF, false,
or a partial record. Cleanup preserves the operation error. No retry loop masks
native errors. Directory/cursor Drop closes once and ignores close errors under
the existing Drop policy; there is no explicit close alias.

## Ownership, representation and native ledger

Directory and cursor are distinct opaque Move pointer types, Ty::FsDirectory and
Ty::FsDirCursor, with matching Scalar variants. Ordinary locals, move/return,
shared or exclusive helpers, records/sums/Option/Result, and existing heap-tree
record collection transport are supported through the same structural ownership
rules as crypto.digest. Direct opaque collections, tuples, box, globals, closure
capture, parallel transport and extern signatures remain excluded. No new owner
container family or region dependency is introduced. A cursor's native directory
stream is exclusively mutated by next; independent cursors can interleave.

A directory shell owns one fd. A cursor shell owns one DIR pointer and a terminal
state (Active, Eof, Error(status)). Each successful next owns one name allocation;
no full-tree native array, recursive traversal, manifest, cleanup loop or sorting.
Path marshalling is proportional to explicit path bytes. OOM follows the existing
hard-error policy. All constructor partial states have RAII cleanup before
publication. Null moved-source pointers have no-op Drop; a live owner closes once.

The native ABI uses pointers for both opaque owners, i64 byte lengths, i32 status,
u32 modes, and explicit caller-owned output scratch. All operation symbols use
align_rt_fs_ plus directory_open, directory_cursor, cursor_next,
directory_metadata, directory_metadata_at, directory_open_dir,
directory_open_read, directory_open_read_single_link, directory_create_new,
directory_create_dir, directory_remove_file, directory_remove_dir,
directory_set_mode, reader_metadata, writer_metadata, file_metadata,
reader_set_mode, writer_set_mode, file_set_mode. Drop symbols are directory_free
and cursor_free. Constructor scratch is one pointer; metadata scratch is 64 bytes
aligned to 8. Native metadata physical order is device, inode, links, size,
modified_seconds, changed_seconds (offsets 0..40 by 8), then kind, mode,
modified_nanoseconds, changed_nanoseconds (offsets 48, 52, 56, 60). The compiler
must certify the complete qualified schema independently before native writing.
Cursor next returns status and writes an existing owned-byte header plus a one-byte
presence flag; compiler constructs ordinary Option<dir_entry>, not a native tagged
record. The byte header is exactly 16 bytes/alignment 8: pointer at offset 0 and i64
length at offset 8; presence is a separate i8 byte, 0 or 1. These are native
scratch layouts, not a persisted format.

## Implementation closure matrix

| Axis | Required owner |
| --- | --- |
| Formation and carriers | `fs_retained_tree::{carriers_and_imported_generics,rejects_wrong_inputs_and_receiver_ownership,ownership_cleanup_and_negative_controls}`: both opaque owners, records/arrays/sums/Option/Result, imported generics, exact call types and borrowed authority. Structural exclusions reuse the Move-handle classifier and its existing owner inventory. |
| Identity and byte grammar | Native `paths_and_admission`, `retained_rename_raw_paths_and_special_entries`; unchanged shared traversal/open helpers retain `fs_beneath_validation_types_and_same_final_matrix`, `fs_beneath_observe_open_races_and_retained_ancestor`, and the single-link owner. Raw-name creation is native-filesystem dependent; invalid-UTF-8 fixture names run on Linux. |
| Cursor lifecycle | Native `cursor_states`, `output_admission_preserves_scratch_and_cursor`; driver cleanup/carrier tests: independent positions, parent Drop, owned name, cached EOF/error and invalid scratch with no advance. |
| Metadata and mode | Native `metadata_conversion_and_modes`, `mode_validation_and_umask`, `paths_and_admission`; driver `metadata_matches_native_fields` and `retained_tree_operations_round_trip`: all fields and four descriptor classes, no-follow final kind, position unchanged, explicit flush and isolated umask. |
| Mutation and cleanup | Native `constructor_failure_cleanup` and driver round trip: RAII closes every injected partial constructor, failed create publication leaves the name; raw retained rename and symlink removal preserve other entries. Driver `ownership_cleanup_and_negative_controls` independently detects omitted directory/cursor/name Drop through descriptor count and live requested bytes. |
| Control/ownership | Driver cleanup/carrier owners (including early_root/early_path/early_mode) cover move/nulling/replacement, ordinary aggregate carriers, if/match/else/?/map_err/loop and early return. Eager receiver consumption is rejected. HIR/MIR visit every ordered operand and terminate lowering before later operands/native action. |
| Trust and ABI | HIR `retained_tree_records`; LLVM `retained_tree_mir_gate`; runtime ABI row/ordinal/declaration owners and `test-runtime-abi-exports.sh`. Exact global schemas, both input/output discriminators, scratch types and native numerical/overlap precedence are independently checked. |
| Transport/cache | `carriers_and_imported_generics`, `interfaces_and_cache`, canonical-field owners: whole/per-unit parity, cold/hit/body-change/revert, reconstructed qualified imports and independently encoded/decoded leaf tags. |

One useful capability contains directory ownership, independent enumeration,
metadata and descriptor-relative operations. Splitting dormant native producers
from safe compiler consumers duplicates proof. More than 1,000 handwritten lines
are expected because two opaque owners, reserved records and nineteen operations
cross all compiler layers; one boundary keeps the carrier and native-output
certification proof coherent. No throughput/RSS promise or benchmark is added.

## Exact compiler and ABI closure

Every operation uses one closed HIR FsTree record with a FsTreeKind discriminator
and ordered expression operands matching the public signature: receiver first
when present, then path, then mode. FsTreeKind has exactly the nineteen operation
cases in public-ledger order, with separate cases for receiver-specific metadata
and mode methods. The fixed operation table specifies arity, each exact input
and result type, receiver borrowing mode, and scratch kind. No arbitrary runtime
symbol, class-erased owner or caller-supplied signature is accepted. HIR recomputes
these facts from the operation, globally certified schemas and child expressions;
all child, effect, lifetime, replay, depth and move sweeps visit each operand once.
Early termination skips later operands and native action. CursorNext reserves an
exclusive place through eager evaluation under the existing borrow rules.

MIR FsTree preserves the same closed kind, ordered operands and exact closed
output record `FsTreeOutput = None | Owner(Slot) | Metadata(Slot) |
CursorNext { entry: Slot, present: Slot }`. Each operation accepts only its
specified output case; no slot is inferred from adjacency or untyped metadata. Result is i32 status for every native operation; the producer forms normal
Result/Option values only on the matching success branch. A constructor writes a
slot of its exact opaque/reader/writer type; metadata writes the exact metadata
record. CursorNext writes the dir_entry slot and a distinct Bool presence slot.
None is required for mutations. MIR/LLVM independently reject missing, extra,
aliased or operation-inappropriate output slots and check discriminator,
arity, exact operand/result types, output slot type/exclusive authority, no overlap
and required owned-result initialization. Copy metadata, i64 and Bool leaves must
be validated unconditionally rather than relying only on ownership-graph demand.
Directory/cursor Drop uses their dedicated runtime keys through the existing
recursive owner machinery, with no native recursive entry removal.

| Native operation suffixes | Exact C ABI shape |
| --- | --- |
| directory_open | i32(ptr path, i64 len, ptr out_owner) |
| directory_cursor | i32(ptr directory, ptr out_cursor) |
| cursor_next | i32(ptr cursor, ptr out_name_header, ptr out_present) |
| directory_metadata, reader_metadata, writer_metadata, file_metadata | i32(ptr owner, ptr out_metadata) |
| directory_metadata_at, directory_open_dir, directory_open_read, directory_open_read_single_link, directory_create_new | i32(ptr directory, ptr path, i64 len, ptr out) |
| directory_create_dir | i32(ptr directory, ptr path, i64 len, i32 mode) |
| directory_remove_file, directory_remove_dir | i32(ptr directory, ptr path, i64 len) |
| directory_set_mode, reader_set_mode, writer_set_mode, file_set_mode | i32(ptr owner, i32 mode) |
| directory_free, cursor_free | void(ptr owner) |

u32 modes cross LLVM as i32 with the same bits, never sign extended. Runtime ABI
shape registry reuses structurally identical existing signatures and adds a shape
only for an absent signature. RuntimeKey is a closed discriminator for each of
the twenty-one listed symbols. No new LLVM attributes assert noalias, readonly,
non-null or dereferenceability beyond the established call-site proof.

Native validation order is output numeric extent/alignment (if any), owner
nonnull/alignment and numeric shell extent (if any), input length/address extent
(if any), including checked conversion to usize/isize and checked len + 1
terminator capacity no greater than isize::MAX, then output disjointness from other outputs, the native owner shell and
input bytes. Null, negative, misaligned or address-overflow fields are Invalid
without dereference or writes. Positive readable/writable extents and live owner
provenance remain caller preconditions; numerical checks do not authenticate
arbitrary addresses. A zero-length input reaches lexical Invalid without reading
its pointer. After these checks initialize outputs to all-zero bytes, then check
full path contents/grammar, mode, and finally native operations. Thus malformed
numeric/overlap inputs leave outputs untouched; content/mode/OS/conversion failures
leave them zero. Cursor invalid scratch does not advance or terminalize the
cursor. Its native read failure does terminalize it; the zero name/presence output
still represents no published entry, with the returned status determining Error.
All successful records publish only after full construction/conversion. No ABI
error path frees or overwrites caller-owned initialized payloads; scratch must be
fresh exclusive compiler-owned storage, independently certified at MIR entry.

Canonical leaf tags are Ty FsDirectory=74, FsDirCursor=75; Scalar FsDirectory=52,
FsDirCursor=53. Their canonical node encodings are exactly one byte [74], [75],
[52], [53], respectively, with no payload. Decoder and encoder golden vectors
independently pin these bytes, tag uniqueness and malformed extra/truncated data
under the existing whole-graph framing. Metadata/entry/kind use existing nominal
record/sum encodings with complete reachable definitions and declaration order;
no alias or alternate legacy schema is admitted. Global qualified-schema checks
reject lookalikes, wrong names, wrong field/variant count/order/type and forged
reserved graphs even when no FsTree operation occurs. Native metadata layout is
independently tested against normal compiler record layout, including permutation.
The existing compiler build fingerprint and full MIR body/type graph identity
invalidate changed artifacts; no ambient host fact or current directory is a
cache input to compiled code.

## Author pass and propagation

The ledger fixes all nineteen operations, input/result types, modes and errors,
all native outputs and precedence, owners/Drop, platform scope and canonical
identity. Scalar metadata fields are always present; cursor Active/EOF/Error
states have exhaustive publication rules. There are no text/wire decoding
fallbacks, runtime reflection tables, process-global mutations or later-milestone
prerequisites. Native syscall documentation is the implementation reference:
[fdopendir](https://man7.org/linux/man-pages/man3/fdopendir.3.html),
[readdir](https://man7.org/linux/man-pages/man3/readdir.3.html), and
[fchmod](https://man7.org/linux/man-pages/man2/fchmod.2.html).
Normative declarations are table notation; executable Align examples will be
syntax-checked in the owner fixtures, not mixed with signature notation.

Request one fresh independent adversarial strategy review before implementation.
Propagate the accepted normative contract to draft.md, language-spec,
design-notes, Settled, fs design EN/JA, HIR/runtime ledgers and plan40. Consumer
adoption and recursive cleanup policy remain external. Cleanup requires proved
child absence and exclusive control of the retained parent; mode 0700 does not
exclude same-UID writers. No stat-then-unlink sequence claims atomic deletion of
an identity. Application source immutability/quiescence is an explicit condition
of snapshot evidence, not a promise from metadata comparisons.

Independent strategy review: two P2 contract gaps resolved before implementation.
CursorNext now has an exact two-slot closed MIR output case with independently
certified types and disjointness. Checked terminator-capacity overflow is a numeric
Invalid failure before dereference/allocation or scratch writes. Native
paths_and_admission covers len=isize::MAX with a non-overflowing numeric address
as a negative control, keeping output untouched. No broader contract change.

The bounded follow-up review accepted the explicit named-local receiver boundary,
including owned-local advancement and shared-borrow refusal for next.

Author matrix-to-diff pass: native operations live in `align_runtime::fs_retained_tree`;
`align_sema::fs_tree` owns the closed signatures; HIR replay/flow/effect/ownership
sweeps enumerate FsTree; MIR reconstructs carriers and LLVM performs only ABI
lowering. Native-owner producer certification now carries a vector of exact outputs,
so CursorNext's two disjoint slots use the same proof as existing single outputs.
The exhaustive storage policy inventory advances to 334 variants. No safety
strategy or accepted public contract changed during implementation. The legacy
read_dir slice rejection fixture is updated to the already-shipped R67 contract.
