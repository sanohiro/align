# Call-boundary view authority

Status: implemented.

K1 must reject writes through read-only backing and uses of text after its bytes
change, including calls, captures, mutable outputs and imported functions. It
does not need to accept every safe pairing of a callback and a data descriptor.
The owner explicitly permits small structural restrictions after comparison with
the three current consumers. No new annotation, implicit copy, runtime check or
alternate ownership model is selected.

The existing investigation is preserved on local branch
`investigate/k1-feasibility`, checkpoint `d940fde2`. Its 153 multiple-Place
regressions pass. The width-sixteen model reaches a fixed summary but times out
during the required additional transfer. Earlier family, capture and typed
replay results retain their own source hashes. Those results inform this design;
the Python solver is not being ported by default. The research worktree and its
saved operations remain available at `/home/hiro/prj/align`.

## Consumer evidence and alternatives

The inspected working trees are align-llm `a7b67b3c`, align-media `25788911` and
align-voice `91805f4b`. Media and voice contain ongoing consumer edits; inspection
includes them without modifying their code, pins or Git state. A lexical
inventory of 397, 92 and 27 source files, respectively, found no declaration of a
slice-bearing record. Its one direct slice-returning function is LLM's
`prompt_git_config.trimmed`; its consumer reads the result. The only explicit
function-value parameter found is the string-only provider in `repair.drive`.
This inventory guides the comparison; compiler checks must establish the typed
program result. File hashes are retained with the research checkpoint evidence.

| Real source | Current spelling and required behavior | Effect of the call boundary |
| --- | --- | --- |
| LLM `src/constrained_grammar.align:199` | `copy_row(borrow source: slice<u8>, source_at: i64, borrow mut target: slice<u8>, target_at: i64)` copies bytes without rebinding either descriptor. | No source change. Keep distinct owned-buffer backing and the unchanged descriptors across calls and loop joins. Request160's retained-view and intrinsic-copy witnesses remain required precision, not optional flexibility. |
| LLM `src/native_cuda_prefix.align:138` and `native_cuda_ffi.align:359` | Explicit mutable slice parameters receive encoded IDs and native output. | No source change. Write authority is supplied by the existing parameter mode, not by discovering the concrete function target and caller profile together. Unsafe native ownership remains separately governed. |
| LLM `src/prompt_git_config.align:7,116` | `trimmed(raw: slice<u8>) -> slice<u8>` returns a subrange which `check` reads. | No source change or allocation. The result is a read-only view with its existing inferred lifetime. A caller wanting to modify the original bytes uses the owner or an explicit mutable slice argument. |
| Media `src/guide_raster.align:8,16` | `pixel` and `horizontal` mutate an explicitly borrowed owned array; color is a value slice. | No source change. Owned array storage remains writable through `borrow mut`; color carries no write permission. |
| Media `src/video_png.align:16,67` | Decode reads its value slice and fills a newly owned buffer through a local `bytes()` view. | No source change. The local view derives authority from the owned output; the input stays read-only. |
| Voice `src/tokenizer.align:12,110` | Table construction writes owned arrays; token emission borrows an output builder mutably. | No source change. Ownership and explicit mutation already supply the required distinction. |

The full relational alternative preserves writable returned views and mixed
callback/permission families. It requires selected mutable-Place identity,
family predicates, target-relative environments and correlated call replay. The
model demonstrates semantic feasibility for bounded cases, but these acceptance
features do not have a current consumer in the inspected sources.

The restriction uses the parameter modes already present in source.
A helper that currently writes a copied slice parameter changes from
`fn write(bytes: slice<u8>) { mut alias := bytes; alias[0] = 1 }` to
`fn write(borrow mut bytes: slice<u8>) { bytes[0] = 1 }`. This is a comparison of
the original K1 witness, not a claim that a consumer needs that rewrite. It adds
no wrapper, allocation or lifetime annotation. The signature now prevents mixing
that writer with a reader under a mode-erasing `fn(slice<u8>) -> ()` type.

Reject blanket invalidation of every field of a borrowed owner: Request160 uses
independent buffer fields, and refreshing views on every loop iteration is
precisely the repetitive workaround this work should avoid. Keep source timing,
selected backing, owner generation, disjoint fields and unchanged-descriptor
proofs. R132/R142 partial-owner cleanup and AM-R07 external writable-view formation
remain separate capabilities.

## Public-contract ledger

This ledger records the reviewed boundary for implementation. Existing
function types and parameter syntax are unchanged. No wire fields, runtime ABI,
allocation, copy, Drop action or command-line option are added.

Here a view leaf means a typed `slice<T>` or existing SoA view
(`Ty::Slice`, `Ty::Soa`, `Ty::SoaParam`, the current `StorageHeaderKind::View`
class), after tagged-type expansion. `str` remains intrinsically read-only.
Inspect contained elements through their declared types, including dynamic and
recursive storage; ownership-bearing headers are not view leaves. Resources,
`resource_ref`, `json.doc` and unsafe raw pointers retain their producer-specific
contracts and do not gain or lose authority merely from this classification.
The existing admitted `out` domain is unchanged.

Access restrictions belong to a descriptor and its projections, not to the
backing generation globally. Two rules apply independently. Access through a
read-only outer view stays read-only through every projection, including owned
storage reached through its elements. Separately, borrowed view leaves already
stored in transferred owning values enter read-only without restricting writes
to that owned container. Thus a value `array<Holder>` may replace elements and
write owned fields, but may not write through a pre-existing `Holder.view`;
a value `slice<Holder>` may do neither. Dynamic or recursive contents obey the
same rules even when the static storage-path walk stops at their header. A
returned read-only alias never downgrades the caller's retained writable alias
or owner.

| Surface | Exact rule | Producer and acceptance owner |
| --- | --- | --- |
| Value parameters and captures | Non-owning view leaves entering an ordinary function by value are read-only, recursively through aggregates and stored elements. Captured view leaves obey the same rule. Transferred owning storage remains owned and may be mutated; its borrowed contents do not acquire write authority from the container. | Sema parameter/storage formation and capture validation; direct, generic, named-function, lifted closure and nested-carrier negatives. |
| Shared parameters | `borrow T` preserves the existing shared-read rule, including non-owning leaves and projections. | Existing shared projection owners plus forwarded calls and copied headers. |
| Mutable view parameters | A top-level slice/SoA view parameter with `borrow mut` or the existing `out` mode receives only proved writable backing. The mode authorizes access to that backing, not writes through non-owning views stored inside its elements. Existing `out` initialization obligations remain. | Call admission and parameter formation; readonly/unknown backing rejects, independently owned backing passes, malformed checked HIR rejects. |
| Mutable owning/aggregate parameters | `borrow mut T` keeps exclusive access to the parameter place and owned storage reachable through it. Pre-existing non-owning view leaves inside the aggregate enter read-only. A caller can pass a proved writable selected view itself as a mutable argument. Local assignment of a newly formed view uses its actual local authority. | Structural storage paths; owned sibling writes pass, inherited nested view writes reject, explicit selected mutable view passes. No declaration-name exception. |
| Ordinary function results | Every returned non-owning view leaf is read-only, including nested aggregate, sum and collection contents. Ownership-bearing result storage remains independently writable. Lifetimes and cleanup are unchanged. Intrinsic projections from an owned buffer/array retain their existing producer rules; they are not ordinary function results. | One call-result policy for direct, indirect, generic and imported calls; read/explicit-copy positives and write/native-writer negatives. |
| Mutable outputs | A view descriptor proved unchanged by the existing no-rebind effect retains its caller authority. A replaced or possibly replaced view descriptor becomes read-only at the call boundary. Element writes alone do not replace a descriptor. Owning storage replacement keeps its ordinary ownership contract. | Existing mutable-output/effect inference and caller application; repeated copy loop, real rebind, branch/loop joins and returned aliases. |
| Validated observations | Every temporal dependency is valid at publication, including UTF-8 and codec observations, projections from validated carriers, and captured dependencies hidden by a function type. Publication includes complete returned values and all caller-visible mutable output contents at every normal exit, including `Err` and early returns. Validate them before establishing fresh caller observations at call completion. Mapped backing identities retain their precision; unknown identity or contents remains conservative. `json.doc` retains its input observation dependencies under its producer contract. A reached write invalidates old caller observations and cannot revive them when the callee validates new data. | Source completion/byte-observation machinery; original before/inside-index timing pair, hidden mutation, old/new text and codec carriers, returned/stored closures, dynamic mutable output contents, early exits and nonreturning calls. |
| Function values | Exact parameter modes already belong to the function type. Return/capture view policy follows the same boundary independent of concrete target discovery. Unknown targets cannot supply stronger permission. | Named and lifted call paths, higher-order forwarding, joined targets and captured views. |
| Declaration audit | Check every declared function under these structural input rules, including uncalled functions. A literal passed to a private writer is rejected without requiring a path from `main`. | Ordinary function analysis and checked-HIR rederivation. |
| Foreign operations | Explicit unsafe raw-pointer operations retain their existing contract. Safe native writers still require writable backing; `unsafe` does not create a safe writable slice from read-only data. No external-view producer is added. | Existing native-operation admission and actual-MIR owners. |
| Cache and interfaces | Existing modes, types, lifetime and no-rebind records remain the interface facts. Derive access policy from validated types/modes on both local and imported paths. The existing executable-byte compiler fingerprint invalidates previous unit and codegen entries; unavailable identity disables caching. No imported permission bit is trusted and no interface field/version change is proposed. | `cache::build_id_from`, `UnitKey::compiler_fingerprint`, interface/cache cold-hit owners and independently rederived checked HIR. |

Readonly restriction is a compile-time error at the attempted write or mutable
argument admission, before code generation. Diagnostics identify the read-only
destination and direct writers to an owner or explicit mutable parameter; they do not prescribe an
implicit or unnecessary copy. Invalid structure/type/mode is rejected before
access analysis. Within valid bodies, diagnostics follow source evaluation and
the existing completion order. Defaults, encodings and validation rules of native
operations are unchanged.

## Implementation closure matrix

| Cell | Implementation boundary | Required regression |
| --- | --- | --- |
| Formation, Copy, move-in/out, source nulling | Distinguish inherited read-only access from the read-only default for embedded borrowed views. Carry the former through all projections, including nested owners; apply the latter without tainting the containing owned allocation. Keep restrictions descriptor-local and reuse ownership/generation formation. | Paired owned-array/view-of-record cases, writable caller versus read-only returned alias, local owned array/buffer writes, captures, nested records/sums/arrays and unknown recursive content. Reuse readonly publication/conversion owners. |
| Replacement, Drop and return | Keep ordinary owner cleanup. Apply the function result boundary to complete values; preserve readonly through casts and native writers. Mutable no-rebind preserves the original descriptor. | Shared fixed-field return, returned text bytes, reinterpreted views, true descriptor replacement, loop-carried no-rebind and owned-result Drop. |
| `if`, `match`, `else`, `?`, `map_err`, loops and early exits | Existing source-ordered completion and conservative field joins remain. Every normal exit validates temporal dependencies in returned values, captured environments and mutable output contents before caller publication; reached writes survive absent normal returns. | Original K1 timing/hidden-write witnesses; stale stored text/codec/capture negatives and write-then-validate positives; nested/dynamic outputs, branch joins, Result paths and nonreturn. |
| Calls and mutable outputs | Modes bound incoming authority; result types bound outgoing authority. Use existing selected origin/no-rebind facts for precision, without importing arbitrary semantic-program bodies. | Direct/imported/generic/indirect calls, multiple mutable inputs, duplicate/overlapping argument negatives and Request160. |
| Malformed HIR and native admission | Clear/reinfer/compare the same facts, then retain actual-MIR/native checks. Never rely on a type/mode supplied by an unchecked caller. | Forged mode, missing readonly fact, forged result or effect; cold/hit interface consistency. |
| Consumer qualification | Copy current source into provider-owned scratch with source hashes. Consumer trees remain read-only. | LLM cursor copy and source parsers; media raster/PNG/tokenizer modules; voice tokenizer/array owners; whole and per-unit checks, native CPU witnesses where available. |
| Allocation and lifetime parity | Analysis changes no generated data representation or implicit operation. | Reuse exact byte-copy and ownership/Drop counters for touched paths. No new performance promise or benchmark gate. |

One compiler PR must connect formation, caller checks, effects, result publication
and independent replay. A dormant producer-only change would leave the original
holes open. If the implementation exceeds 1,000 hand-written lines, this boundary
avoids duplicated proof and mismatched local/imported authority; it does not
authorize unrelated native producers or additional type features.

The author pass maps each ledger rule to the matrix above. Both displayed writer
declarations syntax/type-check with the baseline compiler. The existing cache
identity was inspected: both cache layers include executable-byte identity and
refuse an enabled cache without that identity. The admission scope is the
existing type and mode grammar, not a new native or wire operation.

The strategy review identified two missing distinctions: inherited access versus
embedded-view defaults, and publication through mutable outputs or captured
validated readers. The ledger and matrix now include both, plus the paired
descriptor-local authority and validate-after-write cases. These checks retain
the existing safety model; they do not require importing function bodies or the
prototype's relational permission solver.

The contract is recorded in `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, the Settled ownership/call decisions in
`docs/open-questions.md`, `docs/impl/03-types.md` and the checked-HIR contract in
`docs/impl/19-hir-validation-ledger.md`. The memory chapters of `docs/guide/` and `docs/little-aligner/`, together with their Japanese
mirrors, describe the same rules. Examples
that intentionally exercise writable value parameters must use the explicit
mutable mode or become refusal tests; preserved research negatives remain intact.

## Implementation and qualification

The compiler derives the boundary in `MoveCheck::call_boundary_view_fact` and
`restrict_boundary_headers`. `StorageHeaderLeaf::readonly_contents` carries the
embedded-view default separately from inherited read-only access. Projection and
materialization preserve both through `MoveValueFact::through_reference` and
`collection_generation_content`. A direct local field replacement installs its
actual authority; unknown indexed contents retain the conservative default.

`record_value_completion` applies ordinary result authority and establishes new
validation observations. `collect_borrow_mut_exit_roots` checks all mutable
outputs before return; `publish_mutable_output_observations` maps the retained
backing into the caller. `pipeline_validation_backing` covers callback-produced
readers, including `map_into`; `map_err` preserves its untouched Ok branch.
Existing source completion freezes arguments before later effects, and existing
checked-HIR replay independently reruns these rules. Escaping closures remain
subject to the existing admission restrictions; this work adds no new closure
escape form.

`view_has_copy_elements` keeps unchanged byte-copy backing live across loop
joins. `check_byte_copy_disjoint` uses completed backing facts and completed
origin places, so a later operand cannot rewrite an earlier operand's identity.
Owned sibling fields remain distinct; shared view fields do not acquire that
proof.

| Contract cells | Regression owners |
| --- | --- |
| Parameter/result authority, nested owners, projections, local replacement, retained caller alias | `call_boundary_view_authority_matrix`; existing `readonly_origin_*` and `readonly_view_publication_*` matrices |
| Normal/early/error exits, fresh and stale text, mutable static/dynamic outputs | `call_boundary_mutable_output_observation_matrix`, `call_boundary_returned_observation_matrix`, `call_boundary_published_output_observation_matrix`; driver `imported_publication_domains` |
| Callback results/captures, `map_err`, mapped output storage | `call_boundary_callback_observation_matrix`, `readonly_origin_accumulator_matrix`; existing task callback owners |
| Exclusive modes, disjoint inputs, overlapping peers | `call_boundary_exclusive_input_matrix`, `disjoint_field_borrows` |
| Forged modes/effects and independent rederivation | `mutable_access_checked_hir_rejects_forged_facts`, existing readonly checked-HIR replay owners |
| Retained-copy loop, source timing, imports/generics, whole/per-unit, cache rehydration | `call_boundary_retained_sibling_copy_loop`; driver `call_boundary_views` |
| Native byte result and unchanged allocation/cleanup | `retained_copy_native_and_cache` checks helper/intrinsic copies at counts 0, 1 and 8 in both frontends; existing `bytes_ops` and ownership owners |

The original negative packet rejects in both frontends. Its source-timing pair
also runs with an explicit mutable writer: mutation before the receiver is read
rejects; mutation inside the index after that receiver completes passes. The
consumer snapshot checks seven modules in both frontends without source edits:
LLM constrained grammar, Git configuration parser and CUDA prefix encoder;
media raster, PNG and tokenizers; voice tokenizer. These provider-side checks do
not claim consumer adoption or an end-to-end consumer release.

Further Python work requires a named remaining question that compiler evidence
cannot already answer. The original solver and its checkpoints remain preserved.
