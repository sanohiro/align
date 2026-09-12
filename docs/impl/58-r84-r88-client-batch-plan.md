# R84–R88 client admission and composition

Status: implemented; consumer adoption remains external.
Baseline: `31280502`. Consumer register: Requests 84–88.

## Capability and public contract ledger

Deliver R84/R85/R86/R88 as one provider batch. R87 requests reopening the
explicit bare owned-record-array JSON restriction in core-design/json.md and the Settled J3b v1 limits in docs/open-questions.md;
its one consumer witness supplies no plan-23 threshold. Keep R87 PROPOSED,
with its existing rejection and embedded-record control. No consumer code or
pin is changed. Consumer P8/A2/A4/A5 acceptance remains external.

| Surface | Exact contract | Owner and acceptance |
| --- | --- | --- |
| R84 | Shared matching of `Option<command>` and existing recursively admitted carriers projects a shared command. Existing `start_scope` borrows it; no implicit clone, ownership extraction, mutable method permission or new native operation. | Extend `borrowed_sum_payload_is_admissible`, preserving checked-HIR replay and native producer validation. None/Some, repeated use, consuming/mutable/escaping rejection, whole/per-unit and existing native scope lifecycle owners. |
| R85 | Existing `json.encode` and `json.encode_bounded` borrow the active record of a shared optional carrier and return independently owned `Result<string, Error>`. Existing grammar, canonical bytes, numeric errors and bounds remain unchanged. | MIR owned_json_piece must use the ordinary Local lowering path, including its authenticated borrowed binding. Existing template input certification reads projected inputs; output remains founded on all actual inputs. None/Some, bounded error, source expiry, nested fields and malformed producer controls. |
| R86 | Record initializer expressions evaluate eagerly in their written order, once each. Values are captured before the next initializer; layout and JSON field order remain declaration order. Moving a source before a subsequent read rejects. Divergence prevents later evaluation and existing Drop releases completed owned temporaries once. | Sema normalizes source initializers to existing local bindings in a block, then constructs declaration-ordered fields from those locals. Apply to ordinary and generic records without new HIR/MIR variants or serialization. Source/whole/per-unit owners cross field permutations, nested records/Option/string, side effects, early exits and already-invalid subtrees. |
| R88 | `fs.open_regular(path: str) -> Result<reader, Error>`, Impure, Linux/macOS. No defaults. Ordinary relative/absolute/dot/ancestor and final symlink resolution; read-only descriptor open with `O_NONBLOCK`, descriptor metadata must prove regular, then clear nonblocking before publishing that same fd as existing owned reader. Nonregular returns `Error.Invalid`; ordinary OS errors retain existing mapping. No entry creation or content mutation. Transient NUL-terminated path scratch and existing reader allocation/Drop, no retained path lifetime. | Extend existing ReaderOpen HIR/MIR operation with an explicit regular admission discriminator, a distinct runtime key and exact ABI `i32(ptr, i64, ptr)`. All analyses preserve existing path-input/reader-output contracts. Runtime owns open/stat/flag/close; caller owns policy. Native and driver acceptance below. |

R88 validates output alignment/address extent, input length/address extent and
input/output disjointness before any dereference or write. Malformed numerical
ranges or overlap leave output untouched and return Invalid. After this preflight
it clears the output, then validates UTF-8 and embedded NUL before any fd operation. An empty path uses ordinary OS
open error mapping. Acquisition immediately arms RAII. Each fallible descriptor
operation retries EINTR; on every failure the fd closes before returning status.
There is no stat-then-reopen, pathname identity comparison or global state.
Valid input bytes must be live and immutable, and the output slot live and writable;
numerical checks cannot authenticate arbitrary native addresses. Zero-length input
may have a null pointer; positive-length null, negative length and path-scratch
length overflow reject before reading bytes. The fd is close-on-exec.
Close follows existing single-close ownership (never retry a possibly released fd).
The primary open/stat/flag error wins; Drop ignores a close error, matching reader
Drop. Test each post-acquisition failure with descriptor cleanup evidence.
A pathname replacement can select either object at open; only the opened object's
regular kind authorizes publication. No remote-filesystem interruptibility,
filesystem sandbox, stable file contents or final consumer-cutover promise.

The new runtime key joins the ABI inventory and declaration golden. Compiler
cache identity continues to bind compiler artifacts; no persisted analysis field
or interface version change. Reader and JSON runtime layouts are unchanged.
The source contract is recorded in draft.md, docs/language-spec.md, design-notes,
open-questions, the runtime ABI ledger and English/Japanese fs design. R84 updates the shared payload inventory in the specification and process design
(with Japanese mirror); R86 and R88 add the ordering/constructor prose. R85
repairs existing composition without changing the JSON grammar.

## Implementation closure matrix

| Axis | Implementation requirement | Owner |
| --- | --- | --- |
| Formation / validation | Exact Command shared grammar; ReaderOpen discriminator across all HIR/MIR visitors and type replay; reject malformed path/output; preserve JSON schema exclusion | sema classifier and checked-HIR tests; MIR producer mutation tests; runtime ABI golden |
| Construction / move-in/out / source nulling | Snapshot initializers once in source order; transfer snapshot locals to layout fields with existing Move/nulling; shared matches never own payload | `client_admission_composition` driver target, generic/nested/Option/string permutations and negative moves |
| Drop / replacement / return | Intermediate snapshots release once on error or branch return; independent encoded output and reader survive input expiry; native failure guard closes fd | driver source-expiry/early-exit owners and native regular-reader descriptor-count tests |
| Control / order | if/match/else/?/map_err/loop joins reuse existing block/local machinery; source-order eager snapshots stop on divergence; clean and invalid initializer subtrees receive existing diagnostics | parameterized initializer owner with side-effect trace, None/Err and loop/branch paths; existing control-flow owner coverage |
| Generic / interface / whole / per-unit | Normalize both ordinary and generic constructors; generic source transport reparses same normalization; no new ownership metadata | imported and generic driver fixtures in both compilation modes |
| JSON certification | Readable borrowed template pieces found owned output; missing/inactive/wrong-type input cannot certify output | borrowed optional/nested encode driver fixture; producer forged-source negative owner |
| Native regular admission | Same fd is statted, flags restored, published; semantic/native error leaves null; malformed ABI preflight leaves output untouched; no FIFO writer needed | runtime regular/empty/readonly, relative/dot/symlink/dangling, directory/device/FIFO, invalid input, replacement/descriptor binding, cleanup; portable Linux/macOS CI |
| Allocation / provenance | No implicit source clone; snapshots use existing local transfer; reader allocates its existing handle only on admitted fd; encoder retains its existing explicit allocation | existing ownership/Drop owners plus source-expiry and negative borrowed escapes |

One capability may exceed 1,000 handwritten lines because the new reader admission
must travel through its existing compiler pipeline atomically, while the adjacent
composition owners share whole/per-unit setup and the same client adoption wave.
No dormant metadata producer or duplicate broad verification boundary is useful.
There is no performance/resource promise requiring a benchmark. FIFO responsiveness
is correctness and uses an externally bounded child owner, not a timing benchmark.

Before implementation, one independent strategy review checks these boundaries.
`scripts/test-client-admission.sh` is the same Linux/macOS CI and local owner:
workspace build, native regular-reader tests and the client driver target.
Before publication, the author maps each applicable row to concrete tests and
implementation, applies align-self-review, and obtains one fresh inspection-only
candidate review. Final preflight and CI precede merge; after the batch merges run
exactly `cargo build --release --workspace` and update only the external register.

## Strategy review resolution

The first collaboration inspection was interrupted as INCOMPLETE after no progress
checkpoints. Its later checkpoint was retained; a host-native continuation inspected
only R84/R86/R88 safety axes and completed. Evidence is retained in the local review
logs. R85 reused an existing representation-preserving lowering seam and its baseline,
fixed whole/per-unit and malformed-place owners already pass.

- R84: isolate mutation, consuming-call and escape negative witnesses so escape
  rejection cannot mask a receiver bug. Extend the exact shared-payload classifier;
  keep existing method receiver restrictions. A matched command binding can receive
  start_scope; admitting commands in recursive carriers does not introduce methods
  on previously unsupported nested field receivers. Check the downstream MoveCheck
  path, not only the early current-parameter receiver guard.
- R86: existing HIR StructLit means declaration-ordered evaluation of its children.
  Source normalization supplies source-ordered local snapshots only where needed
  before constructing that HIR value. Checked HIR must validate the normal locals,
  initialized reads, types, provenance and Drop; it does not authenticate arbitrary
  HIR against an unavailable original AST. A direct already-ordered StructLit remains
  valid, so requiring every HIR literal to have a synthetic wrapper would reject
  valid producers without improving safety. Retain malformed use-after-move owners
  and update the HIR ledger with this distinction.
- R88: add exact regular_only semantics to the HIR ledger and preserve it in MIR
  lowering and replays. A Boolean has exactly two valid alternatives; changing it
  selects a different valid operation, like changing another typed opcode. Exact
  whole/per-unit native behavior and runtime-key emission own this distinction.
  Validate numerical extents/overlap before output zeroing; do not use abi_str_view's
  permissive null/negative-length empty-input behavior. Specify primary error versus
  single-close cleanup precedence and test syscall failure/retry/cleanup cells.

## Author closure pass

The ledger-to-diff extraction covers the exact/reject/before/every obligations
above and the added source-contract paragraphs. Gates 1–6 of align-self-review
were applied to this diff: the existing ReaderOpen operation retains explicit
visitor/replay coverage, both constructors share path checking and ownership,
the new ABI uses the existing A08 shape, and source snapshots reuse existing
local Move/Drop rather than introducing another ownership representation.

| Extracted obligation | Concrete implementation and owner |
| --- | --- |
| Shared payload admission, no mutation/transfer/escape | `borrowed_sum_payload_is_admissible`, `require_exclusive_handle_receiver`; `borrowed_optional_command_shared_lifecycle` and `borrowed_optional_command_rejects_mutation_and_transfer` |
| Borrowed JSON reads, independent output, exact bounds | MIR `lower_local` / `owned_json_piece`; `borrowed_optional_json_encoding_owns_result`, `producer_json_borrowed_root_requires_initialized_exact_place` |
| Source order, one snapshot before each later initializer, declaration layout | sema `ordered_struct_literal` in ordinary/generic checking; `record_initializers_snapshot_before_owner_move`, `record_initializers_preserve_effect_order`, `record_initializers_preserve_copy_snapshot_and_nested_order` |
| Invalid Move/init replay and stop before later evaluation | `record_initializers_reject_read_after_move`, `reordered_record_snapshots_replay_ownership_and_initialization`, `record_initializers_stop_at_early_exit`; existing Block/Let cleanup and control-flow owners remain authoritative for unchanged joins |
| Exact constructor type, no retained path, owned result | ReaderOpen lowering and existing producer contract; `regular_reader_requires_import_and_exact_path_type`, `regular_reader_survives_path_owner_and_keeps_normal_resolution`, `hir_body_validator_native` for both discriminator values |
| Numerical preflight before output/bytes; UTF-8/NUL before fd operations | `fs_regular::open`; `regular_reader_abi_preflight` including overlap and multi-invalid input |
| Same opened descriptor, regular-only, restored flags, every failure closes, EINTR retries | `fs_regular::acquire` / `syscall`; `regular_reader_native_matrix` and its isolated child cover all four operation classes, fd closure, both pathname replacements, flags and FIFO deadline |
| Exact ABI inventory and independently compiled signature/export parity | `IoReaderOpenRegular`, A08 declaration golden, runtime export; runtime-ABI unit owners and `scripts/test-runtime-abi-exports.sh` (also refresh its stale preceding inventory counts) |
| Explicit deferred grammar | `owned_array_json_root_remains_explicitly_deferred`; R87 stays PROPOSED under plan 23 |

No compiler cache/schema field changes. Existing allocation/Drop behavior is
retained for synthetic locals, shared projections and reader storage. No new
benchmark or consumer-side acceptance is claimed.

## Candidate review resolution

The independent full-diff inspection found one P2: fixed Move-struct array HIR
validation still required a direct StructLit, so a source-normalized constructor
failed validation. The source witness reproduced the per-unit failure. The array
validator now unwraps validated Block tails and still requires construction of the
exact element struct; a Local/call remains outside that in-place admission.
MIR's existing guarded element materializer already owns block-valued temporaries,
so no ownership strategy or source grammar changes. The sibling sweep covered
Copy arrays, direct/nested StructLit stores, consumed aggregate materialization,
source nulling and per-element cleanup. `reordered_record_initializers_in_fixed_arrays`
covers ordinary/generic constructors, field replacement, successful completion and
later-element early exit in both compilation modes, with positive allocation/free
counts proving that completed owners are released. Existing malformed-array owners
retain rejection of copying pre-existing Move elements.
