# Mutable Copy-view call effects

Status: implementation plan for the four `return_provenance` nightly failures
after the A1 correction. The two false-acceptance witnesses and the reopened
closure matrix are in `self-audit-2026-09-26.md`. This plan consumes the existing
Copy-view/backing distinction in `19-hir-validation-ledger.md`; it does not add
syntax, an ownership kind, a runtime operation, or a new alias model.

## 1. Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Source call | `borrow mut x: T` continues to require a writable stable place and exclusive, nonoverlapping arguments. For a single Copy-view header, the existing pre-call observation invalidation remains unless one specific descriptor or original-backing change is guaranteed on every returning path. A definitely changed descriptor or backing element does not end the addressed allocation. A pre-call alias keeps its captured descriptor and observes the backing's post-call contents. Owned collections and resources still end displaced generations. The call has its existing return/error behavior and no allocation beyond its body. |
| Eligible destination | An exact `BorrowMut` parameter whose concrete `storage_type_paths(T)` is valid, has exactly one root-path `StorageHeaderKind::View` header, and has no carriers. This includes a direct slice view and excludes an owned header or a product with multiple headers. Noneligible types keep the existing conservative exclusive transition; no source-visible exception is keyed on a package or name. `out` retains its existing separate transition. |
| HIR and interface record | `MutableViewEffectSummary = Option<Vec<Option<MutableViewEffect>>>` is stored as `mutable_view_effect` on `hir::Fn`, `hir::ImportedFn`, and `align_interface::IFnSig`; one outer position exists per logical parameter including captures. Function-level `None` means unavailable. An eligible `BorrowMut` position in a producer-certified, non-generic function has `Some(effect)`; every other position has `None`. The existing `MutableRetentionSummary` remains authoritative for ordinary retained roots and unchanged-destination classification. The new record partitions only the single-view effects that those roots cannot distinguish. Its two source lists need not be subsets of the old retention roots: a write followed by a rebind leaves roots in the old backing after they have disappeared from the destination. Producer replay authenticates that relation from the body instead of imposing a false structural equality. |
| Effect | `MutableViewEffect { may_write_original: bool, must_write_original_element: bool, may_rebind: bool, must_rebind: bool, write_roots: Vec<MutableRetentionRoot>, rebind_roots: Vec<MutableRetentionRoot> }`. Each root has the existing exact `Contained(u32)` or `Storage(u32)` meaning. `write_roots` overapproximates sources stored through **any** view reached by the destination's header flow on any returning path, including copies of the original header, temporary rebind targets, and nested calls even if a later rebind removes that target from the exit value. `may_write_original` separately says whether any such store can address the entry header. `rebind_roots` overapproximates sources of a new exit descriptor. Old backing contents and the old header are retained independently when a returning path may keep them. `must_write_original_element` means every returning path performs at least one successful **whole-element** store through the entry header before any possible rebind; a projected struct/SoA field write never establishes it. `must_rebind` means every returning path replaces that header. Both imply their corresponding `may` bit. Empty roots mean no new borrowed source, not an all-element overwrite. |
| Ownership and allocation | The HIR record and its two root vectors are compiler-owned, cloned only with the containing checked program or interface, and live until that program/interface is dropped. The caller consumes borrowed references to validated facts during checking and publishes no pointer, thunk, runtime table, or source-visible allocation. Encoding owns its output byte buffer; decoding owns bounded vectors after count validation. No text, view, embedded-NUL, native, process-global, or FFI boundary is added. |
| Transfer | At the one post-eager call action, an unavailable or unresolved effect has both `may` bits, neither `must` bit, and every compatible source in both root sets. The caller joins substituted `write_roots` into the content of **every** completed compatible backing candidate reachable from the call's actual arguments and captures, including the original backing, transient rebind sources, and the post-call destination. This deliberately loses target precision when an intermediate descriptor is no longer the exit value; it cannot lose a source installed there. Empty roots never clear old content. Only a guaranteed whole-element entry-header write and an authenticated, known single-cell backing with no unknown candidate can strongly replace that cell with the substituted write roots. The whole-element requirement excludes projected field writes even for a one-row SoA. For every other backing, old content remains possible. `may_rebind` builds the destination header from substituted `rebind_roots`; `must_rebind` removes its old-header candidate, while a possible but nonmandatory rebind joins old and new candidates. A selected source with a region/storage root but no known header, such as `clone_in(out)`, yields a region-rooted unknown candidate; an empty descriptor remains empty. With neither guaranteed change, the existing exclusive old-observation invalidation remains. With `must_write_original_element` or `must_rebind`, the exact eligible view's backing release root is not ended; content-owner roots still end when their real owners move, replace, or Drop. Other destination leaves retain their existing transitions. |
| Control | May bits and root sets join by union over every returning edge; must bits join by intersection. A function with no reachable returning edge publishes all bits false and empty root sets. An error return from `?` or `map_err` is a returning edge. Abort, divergence, and a terminated later eager argument contribute no post-call state. A store after a possible rebind cannot establish `must_write_original_element`. Direct, imported-certified, concrete generic, and target-resolved function-value summaries compose through one least fixed point across recursion and forwarding. An unavailable imported effect, unresolved indirect target, or opaque nested call is conservative and cannot establish a must bit; a malformed record rejects before analysis rather than becoming unavailable. |
| Validation | The shared HIR/interface validator checks outer arity, mode/type eligibility, reserved flag bits, `must ⇒ may`, empty `rebind_roots` when `may_rebind` is false, sorted unique root pairs, in-range source ordinals, and absence on generic templates. `write_roots` may be nonempty while `may_write_original` is false because a transient rebind target was written. The interface reader checks byte syntax, tags, bounded counts, flags, root order/range, and outer arity in wire order while reading each function. Because function records precede nominal definitions, resolved mode/type eligibility runs after the complete interface and type graph are read. A producer-certified imported record is validated before MoveCheck or EscapeCheck consumes it. Checked-HIR replay re-infers local effects from bodies and compares the exact record; imported records remain only if valid and producer-certified. A malformed `Some` record rejects before MIR, LLVM, artifact or cache publication; only absent `None` uses conservative fallback. Source diagnostics stay the existing invalidated-borrow/escape classes; malformed compiler records use the existing fail-closed validation result. |
| Wire and cache | Interface format 17 appends this field immediately after the existing mutable-retention field in each function record. `None` is byte `00`. `Some` is byte `01`, then outer `u32` little-endian count; each position is `00` for ineligible or `01` followed by one flag byte, a `u32` write-root count and its `(u8 tag,u32 ordinal)` entries, then a `u32` rebind-root count and entries. Flags are bit 0 `may_write_original`, bit 1 `must_write_original_element`, bit 2 `may_rebind`, bit 3 `must_rebind`; bits 4–7 reject. Root tags remain 0 Contained and 1 Storage. Nested sequences retain this exact order with no padding or trailing bytes. The new bytes enter the canonical interface hash, which already includes complete reachable nominal type definitions used for eligibility. Format 16 rejects as an unknown version; there is no compatibility decoder. A private-body edit changes a consumer key iff the exported effect bytes change. The MIR/LLVM ABI, runtime allocation, implementation hash policy, and nominal type identity are unchanged. |
| Prerequisite and acceptance | The existing checked-HIR producer, mutable-retention transport, generation directory, and source-order call action are prerequisites. A single producer/interface/consumer PR is the smallest useful boundary: a serialized effect without the caller transition, or vice versa, leaves the nightly defect or a safety hole. Acceptance requires the four nightly positives, exact old-alias rebind and partial-write negatives, one-cell strong replacement, direct/imported/indirect/generic parity, malformed records, independent wire goldens and interface-hash changes. There is no performance/resource promise or benchmark. |

The canonical field vectors, independent of any surrounding function record,
are fixed as follows (`u32` words are little-endian):

```text
None:                                     00
Some([None]):                             01 01000000 00
Some([Some(no-op)]):                      01 01000000 01 00 00000000 00000000
Some([Some(must-write, no roots)]):       01 01000000 01 03 00000000 00000000
Some([Some(must-rebind, Contained(1)), None]):
                                          01 02000000 01 0c 00000000 01000000 00 01000000 00
```

The last vector requires two logical parameters: destination 0 is the eligible
mutable view and parameter 1 supplies its contained root. Owners independently
encode each semantic record and decode each literal; a round trip alone is not
an independent golden.

Decoder failures retain the existing `DecodeError` variants. Version rejection
precedes every record. A missing byte returns `Truncated`; an option or root tag
outside its closed set returns `BadTag` with `what` equal to
`"mutable-view-effect option"` or `"mutable-view-effect root"`. An outer count
different from the parameter count, or a root count above twice that count,
returns `InvalidSummary("mutable-view-effect count")` before allocating its
sequence. At each position the reader checks flag implications and reserved
bits before `rebind_roots` presence, then checks root range before strict
`(tag, ordinal)` increase in wire order. These return
`InvalidSummary("mutable-view-effect flags")` or
`InvalidSummary("mutable-view-effect roots")`. A nonempty `write_roots` list
does not require `may_write_original`.

The reader finishes the complete interface and checks `TrailingBytes` before
the surface hash. `InterfaceHashMismatch` precedes resolved-type eligibility;
tests recompute a valid hash when checking semantic rejection. The resolved
type graph then checks position eligibility in function and parameter order,
returning `InvalidSummary("mutable-view-effect eligibility")` through
`deserialize`'s new import-validation error mapping. Checked-HIR replay uses
the same semantic eligibility validator.
No import-validation error for this field is ignored. The producer uses the
same semantic validator on its in-memory record before hashing or writing.

## 2. Implementation closure

The reopened matrix in the audit is the implementation checklist. Its concrete
owners are the `return_provenance` target for source and alias behavior,
`imported_mutable_retention` for cross-unit effects, `align_interface` codec
goldens, checked-HIR replay and malformed-record owners, and the existing
`array_truncate`/resource owners for unchanged owned behavior. The author-side
matrix-to-diff pass must point every cell at code and a failing-before/passing-
after regression or an existing owner that would fail for the changed defect.

```text
return_provenance::mutable_copy_view_effect_matrix
return_provenance::non_borrowing_mutable_calls_do_not_require_region_backing
return_provenance::borrow_mut_places_and_numeric_storage_use_argument_completion_snapshots
return_provenance::indexed_str_store_requires_a_retaining_parameter_mode_for_caller_backing
return_provenance::storage_generation_move_replacement_escape_matrix
imported_mutable_retention::mutable_copy_view_effect_transport
align_interface::codec::mutable_view_effect_goldens_and_rejects_malformed_records
align_sema::mutable_view_effect_replay_rejects_stale_facts
```

The new matrix owner covers the two false-acceptance witnesses, single/two-cell
and ranged backing, written versus untouched owner roots, header-only and
write-then-rebind calls, transient rebind-write-rebind with a second actual
backing, full-element versus projected-field writes in one-cell SoA, source-root
partitioning, fresh `clone_in(out)` unknown headers, and control-flow must-bit
joins. The transport owner covers direct/imported/concrete function-value and
generic forwarding, unavailable fallback, and whole/per-unit parity. The codec
owner checks both directions of every literal vector and multi-invalid error
precedence; the replay owner mutates each field and each eligibility condition.

The effect is analysis metadata, not an Align-visible promise that every valid
program gets exact alias precision. Strong replacement requires the single-cell
proof above; unknown offsets, multiple cells, joined backing candidates, and
partial or conditional writes retain old owner dependencies. This is the one
uniform rule for all eligible view types. It prevents the descriptor-only and
partial-write use-after-free witnesses while admitting the nightly's proved
single-cell and scalar-view cases.

The handwritten diff may exceed roughly 1,000 lines. The interface producer,
codec, checked-HIR replay, MoveCheck, EscapeCheck, and call consumer form one
strict safety chain; splitting at a dormant field or consumer would duplicate
the same malformed-record and whole/per-unit proof without a useful stable
capability. No Rust implementation begins until one fresh adversarial review
of this ledger, the reopened matrix, and the capability boundary is resolved.

## 3. Source-of-truth propagation

The final ledger decision must agree with `draft.md` and
`docs/language-spec.md` on which generation an old Copy-view alias observes;
the current blanket statement that every `borrow mut` ends every old backing
generation is too broad for the already-shipped view/owned distinction.
`docs/design-notes.md` and the Settled ownership entry in
`docs/open-questions.md` must describe the same distinction without creating
a second ownership model. `08-memory-model-v2.md`,
`17-library-boundary-prerequisites.md` (mutable-retention transport and format
inventory), `19-hir-validation-ledger.md` (mutation-to-observer and call action),
and the audit must use this field's exact names and conservative fallback.
There is no Japanese mirror for these compiler design sources.
